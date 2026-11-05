//! Talks to the keyboard from a browser, through Web Serial. Same requests and
//! same messages as the desktop `backend`, in one queue that takes turns with the
//! page: a browser tab has no threads to block.

pub use crate::bg::*;
use crate::protocol::binary::{self as bp, cmd, KrResponse};
use crate::protocol::connection::{self, ConnectionKind};
use crate::protocol::{config_io, device, layout, parsers};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen(module = "/src/web_serial.js")]
extern "C" {
    fn serial_supported() -> bool;
    #[wasm_bindgen(catch)]
    async fn serial_granted() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn serial_request() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn serial_open() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn serial_write(bytes: &[u8]) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    async fn serial_read(ms: f64) -> Result<JsValue, JsValue>;
    async fn serial_close();
    async fn sleep(ms: f64);
    #[wasm_bindgen(catch)]
    async fn pick_text(accept: &str) -> Result<JsValue, JsValue>;
    fn download(name: &str, text: &str);
}

const READ_TIMEOUT_MS: u64 = 1500;
const MATRIX_POLL_MS: f64 = 50.0;
const DRAIN_MS: f64 = 30.0;

/// A browser has no list of ports to show: it asks when Connect is pressed.
pub fn list_ports() -> Vec<(String, String, bool)> {
    Vec::new()
}

pub fn list_prog_ports() -> Vec<String> {
    Vec::new()
}

pub const NO_PORT_LABEL: &str = "Chosen in the browser";
pub const CAN_FLASH: bool = false;

enum Job {
    Connect { ask: bool },
    Disconnect,
    LoadLayer { layer: usize, edit: u64 },
    LoadEverything { layers: usize, edit: u64 },
    WriteKeys(Vec<((usize, usize, usize), u16)>),
    RenameLayer { layer: usize, name: String },
    Heatmap,
    Stats,
    Advanced,
    Macros,
    Command { id: u8, payload: Vec<u8>, refresh: Refresh },
    Tama(u8),
    Autoshift,
    LayoutFromKeyboard,
    MatrixToggle,
    NvsReset(u8),
    Export { mask: u8, name: String },
    Import { config: config_io::KeyboardConfig, mask: u8 },
    DongleRefresh,
    DongleReset,
    PairStart,
    PairCheck { remaining: u8, baseline: u8 },
    Wpm,
    DonglePoll { slow: bool },
}

struct Link {
    open: bool,
    is_dongle: bool,
    kind: ConnectionKind,
    rows: usize,
    matrix_on: bool,
}

struct Shared {
    queue: RefCell<VecDeque<Job>>,
    running: Cell<bool>,
    link: RefCell<Link>,
    tx: Tx,
}

pub struct Backend {
    shared: Rc<Shared>,
    pub tx: Tx,
}

fn js_text(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::Reflect::get(&e, &"message".into()).ok().and_then(|m| m.as_string()))
        .unwrap_or_else(|| "unknown error".to_string())
}

fn now_ms() -> f64 {
    js_sys::Date::now()
}

impl Shared {
    fn kind(&self) -> ConnectionKind {
        self.link.borrow().kind
    }

    /// One request and its reply. Other frames arriving meanwhile are matrix events.
    async fn request(&self, id: u8, payload: &[u8]) -> Result<KrResponse, String> {
        serial_write(&bp::ks_frame(id, payload)).await.map_err(js_text)?;
        let wait = connection::read_timeout_ms(id, self.kind(), READ_TIMEOUT_MS) as f64;
        let end = now_ms() + wait;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let left = end - now_ms();
            if left <= 0.0 {
                let relayed = matches!(self.kind(), ConnectionKind::Dongle { .. }) && !connection::is_dongle_local(id);
                return Err(if relayed {
                    "Clavier injoignable via le dongle (eteint / hors portee)".to_string()
                } else {
                    "No answer from the keyboard".to_string()
                });
            }
            let chunk = serial_read(left.min(100.0)).await.map_err(js_text)?;
            bytes.extend(js_sys::Uint8Array::new(&chunk).to_vec());
            while let Ok((frame, used)) = bp::parse_kr(&bytes) {
                let start = bytes.windows(2).position(|w| w == [0x4B, 0x52]).unwrap_or(0);
                bytes.drain(..start + used);
                if frame.cmd == id {
                    return Ok(frame);
                }
                self.matrix_frame(&frame);
            }
        }
    }

    fn matrix_frame(&self, frame: &KrResponse) {
        if frame.cmd == cmd::MATRIX_TEST && frame.is_ok() && frame.payload.len() >= 3 {
            self.tx.send(BgMsg::MatrixEvent(frame.payload[0], frame.payload[1], frame.payload[2]));
        }
    }

    /// `request`, with any status but OK as an error.
    async fn send(&self, id: u8, payload: &[u8]) -> Result<KrResponse, String> {
        let r = self.request(id, payload).await?;
        if r.is_ok() {
            Ok(r)
        } else {
            Err(format!("Firmware error: {}", r.status_name()))
        }
    }

    async fn drain(&self) {
        loop {
            match serial_read(DRAIN_MS).await {
                Ok(chunk) if js_sys::Uint8Array::new(&chunk).length() > 0 => {}
                _ => return,
            }
        }
    }

    async fn keymap(&self, layer: u8) -> Result<Vec<Vec<u16>>, bp::CmdError> {
        let rows = self.link.borrow().rows;
        let r = self.request(cmd::KEYMAP_GET, &[layer]).await.map_err(bp::CmdError::Failed)?;
        if r.is_unsupported() {
            return Err(bp::CmdError::Unsupported);
        }
        if !r.is_ok() {
            return Err(bp::CmdError::Failed(format!("Firmware error: {}", r.status_name())));
        }
        parsers::parse_keymap(&r.payload, rows).map_err(bp::CmdError::Failed)
    }

    async fn layer_names(&self) -> Vec<String> {
        match self.send(cmd::LIST_LAYOUTS, &[]).await {
            Ok(r) => parsers::parse_layer_names(&r.payload),
            Err(_) => Vec::new(),
        }
    }

    async fn layout_json(&self) -> Result<Option<String>, bp::CmdError> {
        let r = self.request(cmd::GET_LAYOUT_JSON, &[]).await.map_err(bp::CmdError::Failed)?;
        if r.is_unsupported() {
            return Err(bp::CmdError::Unsupported);
        }
        if !r.is_ok() {
            return Err(bp::CmdError::Failed(format!("Firmware error: {}", r.status_name())));
        }
        Ok((!r.payload.is_empty()).then(|| String::from_utf8_lossy(&r.payload).to_string()))
    }

    async fn connect(&self, ask: bool) {
        let tx = &self.tx;
        if !serial_supported() {
            tx.send(BgMsg::ConnectError("This browser can't reach USB devices: use Chrome or Edge".into()));
            return;
        }
        if ask {
            if let Err(e) = serial_request().await {
                tx.send(BgMsg::ConnectError(format!("No device chosen ({})", js_text(e))));
                return;
            }
        } else {
            let known = serial_granted().await.map(|v| v.as_bool() == Some(true)).unwrap_or(false);
            if !known {
                tx.send(BgMsg::ConnectHint("Press Connect and choose your keyboard".into()));
                return;
            }
        }
        if let Err(e) = serial_open().await {
            tx.send(BgMsg::ConnectError(js_text(e)));
            return;
        }
        *self.link.borrow_mut() = Link {
            open: true,
            is_dongle: false,
            kind: ConnectionKind::Direct,
            rows: device::DEFAULT_ROWS,
            matrix_on: false,
        };
        self.drain().await;
        if self.send(cmd::PING, &[]).await.is_err() {
            serial_close().await;
            self.link.borrow_mut().open = false;
            tx.send(BgMsg::ConnectError("This device doesn't answer like a KaSe keyboard".into()));
            return;
        }
        let mut profile = device::DeviceProfile::default();
        if let Ok(r) = self.send(cmd::FEATURES, &[]).await {
            profile.features = device::parse_features_csv(&String::from_utf8_lossy(&r.payload));
        }
        let is_dongle = profile.has("RF_DONGLE");
        self.link.borrow_mut().is_dongle = is_dongle;
        let paired = if is_dongle {
            match self.send(cmd::RF_PAIR_LIST, &[]).await {
                Ok(r) => parsers::parse_pair_list(&r.payload).ok().map(|p| p.paired_count),
                Err(_) => None,
            }
        } else {
            None
        };
        let kind = connection::classify_connection(paired);
        self.link.borrow_mut().kind = kind;
        let fw = match self.send(cmd::VERSION, &[]).await {
            Ok(r) => String::from_utf8_lossy(&r.payload).to_string(),
            Err(_) => String::new(),
        };
        tx.send(BgMsg::RfMode(is_dongle));
        tx.send(BgMsg::ConnectionMode(kind));

        match self.layout_json().await {
            Ok(Some(json)) => {
                let (rows, cols) = device::resolve_geometry(layout::parse_geometry(&json), None);
                self.link.borrow_mut().rows = rows;
                profile.rows = rows;
                profile.cols = cols;
                if let Some(name) = layout::parse_name(&json) {
                    profile.name = name;
                }
                match layout::parse_json(&json) {
                    Ok(keys) => tx.send(BgMsg::LayoutJson(keys)),
                    Err(e) => tx.send(BgMsg::StatusError(format!("Layout parse error: {e}"))),
                }
            }
            Ok(None) | Err(bp::CmdError::Unsupported) => {}
            Err(e) => tx.send(BgMsg::StatusError(format!("Layout: {e}"))),
        }
        tx.send(BgMsg::Profile(profile));

        let names = self.layer_names().await;
        let km = match self.keymap(0).await {
            Ok(km) => km,
            Err(bp::CmdError::Unsupported) => Vec::new(),
            Err(e) => {
                tx.send(BgMsg::StatusError(format!("Keymap: {e}")));
                Vec::new()
            }
        };
        tx.send(BgMsg::KeyboardReachable(connection::keyboard_reachable(kind, !km.is_empty())));
        tx.send(BgMsg::Connected("USB".to_string(), fw, names, km));
    }

    async fn fetch_macros(&self) {
        if let Ok(r) = self.send(cmd::LIST_MACROS, &[]).await {
            self.tx.send(BgMsg::MacroList(parsers::parse_macros_binary(&r.payload)));
        }
    }

    async fn fetch_tap_dance(&self) {
        if let Ok(r) = self.send(cmd::TD_LIST, &[]).await {
            self.tx.send(BgMsg::TdList(parsers::parse_td_binary(&r.payload)));
        }
    }

    async fn fetch_combos(&self) {
        if let Ok(r) = self.send(cmd::COMBO_LIST, &[]).await {
            self.tx.send(BgMsg::ComboList(parsers::parse_combo_binary(&r.payload)));
        }
    }

    async fn fetch_leaders(&self) {
        if let Ok(r) = self.send(cmd::LEADER_LIST, &[]).await {
            self.tx.send(BgMsg::LeaderList(parsers::parse_leader_binary(&r.payload)));
        }
    }

    async fn fetch_key_overrides(&self) {
        if let Ok(r) = self.send(cmd::KO_LIST, &[]).await {
            self.tx.send(BgMsg::KoList(parsers::parse_ko_entries(&r.payload)));
        }
    }

    async fn fetch_heatmap(&self) {
        if let Ok(r) = self.send(cmd::KEYSTATS_BIN, &[]).await {
            let (data, max) = parsers::parse_keystats_binary(&r.payload);
            self.tx.send(BgMsg::HeatmapData(data, max));
        }
    }

    async fn fetch_tama_status(&self) {
        if let Ok(r) = self.send(cmd::TAMA_QUERY, &[]).await {
            if tama_present(&r.payload) {
                self.tx.send(BgMsg::TamaStatus(parsers::parse_tama_binary(&r.payload)));
            }
        }
    }

    async fn fetch_rf_status(&self) {
        if let Ok(r) = self.send(cmd::RF_STATUS, &[]).await {
            match parsers::parse_rf_status(&r.payload) {
                Ok(s) => self.tx.send(BgMsg::RfStatus(s)),
                Err(e) => self.tx.send(BgMsg::StatusError(format!("RF_STATUS parse: {e}"))),
            }
        }
    }

    async fn fetch_pair_list(&self) {
        if let Ok(r) = self.send(cmd::RF_PAIR_LIST, &[]).await {
            match parsers::parse_pair_list(&r.payload) {
                Ok(p) => self.tx.send(BgMsg::RfPairList(p)),
                Err(e) => self.tx.send(BgMsg::StatusError(format!("RF_PAIR_LIST parse: {e}"))),
            }
        }
    }

    async fn fetch_battery(&self) {
        if let Ok(r) = self.send(cmd::BATTERY, &[]).await {
            match parsers::parse_battery(&r.payload) {
                Ok(b) => self.tx.send(BgMsg::Battery(b)),
                Err(e) => self.tx.send(BgMsg::StatusError(format!("BATTERY parse: {e}"))),
            }
        }
    }

    async fn read_config(&self, mask: u8) -> config_io::KeyboardConfig {
        use config_io::*;
        let tx = &self.tx;
        let all = mask == 0;
        let mut c = KeyboardConfig {
            version: 1,
            layer_names: Vec::new(),
            keymaps: Vec::new(),
            tap_dances: Vec::new(),
            combos: Vec::new(),
            key_overrides: Vec::new(),
            leaders: Vec::new(),
            macros: Vec::new(),
        };
        if all || mask & 0x01 != 0 {
            tx.send(BgMsg::ConfigProgress(0.05, "Reading layer names...".into()));
            c.layer_names = self.layer_names().await;
            let n = c.layer_names.len().max(1);
            for l in 0..n {
                tx.send(BgMsg::ConfigProgress(0.05 + (l as f32 / n as f32) * 0.30, format!("Reading layer {l}...")));
                c.keymaps.push(self.keymap(l as u8).await.unwrap_or_default());
            }
        }
        if all || mask & 0x08 != 0 {
            tx.send(BgMsg::ConfigProgress(0.40, "Reading tap dances...".into()));
            if let Ok(r) = self.send(cmd::TD_LIST, &[]).await {
                c.tap_dances = parsers::parse_td_binary(&r.payload)
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.iter().any(|&x| x != 0))
                    .map(|(i, a)| TdConfig { index: i as u8, actions: *a })
                    .collect();
            }
            tx.send(BgMsg::ConfigProgress(0.50, "Reading combos...".into()));
            if let Ok(r) = self.send(cmd::COMBO_LIST, &[]).await {
                c.combos = parsers::parse_combo_binary(&r.payload)
                    .iter()
                    .map(|x| ComboConfig { index: x.index, r1: x.r1, c1: x.c1, r2: x.r2, c2: x.c2, result: x.result })
                    .collect();
            }
            tx.send(BgMsg::ConfigProgress(0.60, "Reading key overrides...".into()));
            if let Ok(r) = self.send(cmd::KO_LIST, &[]).await {
                c.key_overrides = parsers::parse_ko_entries(&r.payload)
                    .iter()
                    .map(|k| KoConfig {
                        trigger_key: k.trigger_key,
                        trigger_mod: k.trigger_mod,
                        result_key: k.result_key,
                        result_mod: k.result_mod,
                    })
                    .collect();
            }
            tx.send(BgMsg::ConfigProgress(0.70, "Reading leaders...".into()));
            if let Ok(r) = self.send(cmd::LEADER_LIST, &[]).await {
                c.leaders = parsers::parse_leader_binary(&r.payload)
                    .iter()
                    .map(|l| LeaderConfig {
                        index: l.index,
                        sequence: l.sequence.clone(),
                        result: l.result,
                        result_mod: l.result_mod,
                    })
                    .collect();
            }
        }
        if all || mask & 0x02 != 0 {
            tx.send(BgMsg::ConfigProgress(0.80, "Reading macros...".into()));
            if let Ok(r) = self.send(cmd::LIST_MACROS, &[]).await {
                c.macros = parsers::parse_macros_binary(&r.payload)
                    .iter()
                    .map(|m| {
                        let steps: Vec<String> =
                            m.steps.iter().map(|s| format!("{:02X}:{:02X}", s.keycode, s.modifier)).collect();
                        MacroConfig { slot: m.slot, name: m.name.clone(), steps: steps.join(","), keys: m.keys.clone() }
                    })
                    .collect();
            }
        }
        c
    }

    async fn write_config(&self, c: &config_io::KeyboardConfig, mask: u8) -> String {
        let tx = &self.tx;
        let all = mask == 0;
        let mut errors = 0usize;
        let mut imported = Vec::new();
        let mut frames: Vec<(u8, Vec<u8>, Option<(f32, String)>)> = Vec::new();
        if all || mask & 0x01 != 0 {
            for (i, name) in c.layer_names.iter().enumerate() {
                frames.push((cmd::SET_LAYOUT_NAME, bp::set_layout_name_payload(i as u8, name), None));
            }
            for (l, km) in c.keymaps.iter().enumerate() {
                let p = 0.05 + (l as f32 / c.keymaps.len().max(1) as f32) * 0.30;
                frames.push((cmd::SETLAYER, bp::setlayer_payload(l as u8, km), Some((p, format!("Writing layer {l}...")))));
            }
            imported.push(format!("{} layers", c.layer_names.len()));
        }
        if all || mask & 0x08 != 0 {
            for td in &c.tap_dances {
                frames.push((cmd::TD_SET, bp::td_set_payload(td.index, &td.actions), None));
            }
            for x in &c.combos {
                frames.push((cmd::COMBO_SET, bp::combo_set_payload(x.index, x.r1, x.c1, x.r2, x.c2, x.result as u8), None));
            }
            for (i, k) in c.key_overrides.iter().enumerate() {
                frames.push((cmd::KO_SET, bp::ko_set_payload(i as u8, k.trigger_key, k.trigger_mod, k.result_key, k.result_mod), None));
            }
            for l in &c.leaders {
                frames.push((cmd::LEADER_SET, bp::leader_set_payload(l.index, &l.sequence, l.result, l.result_mod), None));
            }
            imported.push(format!(
                "{} TD, {} combos, {} KO, {} leaders",
                c.tap_dances.len(),
                c.combos.len(),
                c.key_overrides.len(),
                c.leaders.len()
            ));
        }
        if all || mask & 0x02 != 0 {
            for m in &c.macros {
                if m.steps.is_empty() && !m.keys.is_empty() {
                    frames.push((cmd::MACRO_ADD, bp::macro_add_payload(m.slot, &m.name, &m.keys), None));
                } else {
                    frames.push((cmd::MACRO_ADD_SEQ, bp::macro_add_seq_payload(m.slot, &m.name, &m.steps), None));
                }
            }
            imported.push(format!("{} macros", c.macros.len()));
        }
        for (id, payload, progress) in frames {
            if let Some((p, text)) = progress {
                tx.send(BgMsg::ConfigProgress(p, text));
            }
            if self.send(id, &payload).await.is_err() {
                errors += 1;
            }
        }
        tx.send(BgMsg::ConfigProgress(0.95, "Refreshing...".into()));
        tx.send(BgMsg::LayerNames(self.layer_names().await));
        if let Ok(km) = self.keymap(0).await {
            tx.send(BgMsg::Keymap(0, km, None));
        }
        if errors > 0 {
            format!("Import done with {errors} errors")
        } else {
            format!("Imported: {}", imported.join(", "))
        }
    }

    async fn run(self: &Rc<Self>, job: Job) {
        let tx = &self.tx;
        match job {
            Job::Connect { ask } => self.connect(ask).await,
            Job::Disconnect => {
                serial_close().await;
                self.link.borrow_mut().open = false;
                self.link.borrow_mut().matrix_on = false;
                tx.send(BgMsg::Disconnected);
            }
            Job::LoadLayer { layer, edit } => match self.keymap(layer as u8).await {
                Ok(km) => tx.send(BgMsg::Keymap(layer, km, Some(edit))),
                Err(e) => tx.send(BgMsg::KeymapError(e.to_string())),
            },
            Job::LoadEverything { layers, edit } => {
                for layer in 1..layers {
                    if let Ok(km) = self.keymap(layer as u8).await {
                        tx.send(BgMsg::Keymap(layer, km, Some(edit)));
                    }
                }
                self.fetch_macros().await;
                self.fetch_tap_dance().await;
            }
            Job::WriteKeys(writes) => {
                for ((l, r, c), code) in writes {
                    let payload = bp::setkey_payload(l as u8, r as u8, c as u8, code);
                    if let Err(e) = self.send(cmd::SETKEY, &payload).await {
                        tx.send(BgMsg::KeyWriteFailed((l, r, c), code, e));
                    }
                }
            }
            Job::RenameLayer { layer, name } => {
                let payload = bp::set_layout_name_payload(layer as u8, &name);
                if let Err(e) = self.send(cmd::SET_LAYOUT_NAME, &payload).await {
                    tx.send(BgMsg::StatusError(e));
                }
                tx.send(BgMsg::LayerNames(self.layer_names().await));
            }
            Job::Heatmap => self.fetch_heatmap().await,
            Job::Stats => {
                self.fetch_heatmap().await;
                let lines = match self.send(cmd::BIGRAMS_TEXT, &[]).await {
                    Ok(r) => String::from_utf8_lossy(&r.payload).lines().map(str::to_string).collect(),
                    Err(_) => Vec::new(),
                };
                tx.send(BgMsg::BigramLines(lines));
                if let Ok(r) = self.send(cmd::TAMA_QUERY, &[]).await {
                    if r.payload.len() >= 22 && tama_present(&r.payload) {
                        tx.send(BgMsg::TamaStats(parse_tama_stats(&r.payload)));
                    }
                }
            }
            Job::Advanced => {
                self.fetch_tap_dance().await;
                self.fetch_combos().await;
                self.fetch_leaders().await;
                self.fetch_key_overrides().await;
                if let Ok(r) = self.send(cmd::BT_QUERY, &[]).await {
                    tx.send(BgMsg::BtStatus(parsers::parse_bt_binary(&r.payload)));
                }
                self.fetch_tama_status().await;
            }
            Job::Macros => self.fetch_macros().await,
            Job::Command { id, payload, refresh } => {
                if let Err(e) = self.send(id, &payload).await {
                    tx.send(BgMsg::StatusError(e));
                }
                match refresh {
                    Refresh::Macros => self.fetch_macros().await,
                    Refresh::Combos => self.fetch_combos().await,
                    Refresh::Leaders => self.fetch_leaders().await,
                    Refresh::KeyOverrides => self.fetch_key_overrides().await,
                    Refresh::Nothing => {}
                }
            }
            Job::Tama(id) => {
                if let Err(e) = self.send(id, &[]).await {
                    tx.send(BgMsg::StatusError(e));
                }
                self.fetch_tama_status().await;
            }
            Job::Autoshift => match self.send(cmd::AUTOSHIFT_TOGGLE, &[]).await {
                Ok(r) => {
                    let on = r.payload.first().copied().unwrap_or(0) != 0;
                    tx.send(BgMsg::AutoshiftStatus(if on { "Autoshift: ON" } else { "Autoshift: OFF" }.to_string()));
                }
                Err(e) => tx.send(BgMsg::AutoshiftStatus(format!("Error: {e}"))),
            },
            Job::LayoutFromKeyboard => match self.layout_json().await {
                Ok(Some(json)) => tx.send(BgMsg::LayoutPreview(json)),
                Ok(None) => tx.send(BgMsg::LayoutStatus("The keyboard sent no layout".into())),
                Err(e) => tx.send(BgMsg::LayoutStatus(format!("Error: {e}"))),
            },
            Job::MatrixToggle => match self.send(cmd::MATRIX_TEST, &[]).await {
                Ok(r) if r.payload.len() >= 3 => {
                    let on = r.payload[0] != 0;
                    self.link.borrow_mut().matrix_on = on;
                    tx.send(BgMsg::MatrixToggled(on));
                }
                Ok(_) => tx.send(BgMsg::MatrixError("Unexpected reply".into())),
                Err(e) => tx.send(BgMsg::MatrixError(e)),
            },
            Job::NvsReset(mask) => match self.send(cmd::NVS_RESET, &[mask]).await {
                Ok(_) => {
                    serial_close().await;
                    self.link.borrow_mut().open = false;
                    tx.send(BgMsg::Disconnected);
                    tx.send(BgMsg::NvsResetDone(Ok(mask)));
                }
                Err(e) => tx.send(BgMsg::NvsResetDone(Err(e))),
            },
            Job::Export { mask, name } => {
                let config = self.read_config(mask).await;
                tx.send(BgMsg::ConfigProgress(0.90, "Saving file...".into()));
                let done = config.to_json().map(|json| {
                    download(&name, &json);
                    format!("Exported to {name}")
                });
                tx.send(BgMsg::ConfigDone(done));
            }
            Job::Import { config, mask } => {
                let text = self.write_config(&config, mask).await;
                tx.send(BgMsg::ConfigDone(Ok(text)));
            }
            Job::DongleRefresh => {
                if self.link.borrow().is_dongle {
                    self.fetch_rf_status().await;
                    self.fetch_pair_list().await;
                    self.fetch_battery().await;
                }
            }
            Job::DongleReset => {
                if self.link.borrow().is_dongle {
                    match self.send(cmd::RF_PAIR_RESET, &[]).await {
                        Ok(_) => {
                            tx.send(BgMsg::Status("Paired halves cleared".into()));
                            self.fetch_pair_list().await;
                        }
                        Err(e) => tx.send(BgMsg::StatusError(format!("RF_PAIR_RESET: {e}"))),
                    }
                }
            }
            Job::PairStart => {
                let baseline = match self.send(cmd::RF_PAIR_LIST, &[]).await {
                    Ok(r) => parsers::parse_pair_list(&r.payload).map(|p| p.paired_count).unwrap_or(0),
                    Err(_) => 0,
                };
                match self.send(cmd::RF_PAIR_START, &bp::rf_pair_start_payload(false)).await {
                    Ok(r) => match parsers::parse_pair_start(&r.payload) {
                        Ok(p) => {
                            tx.send(BgMsg::Status(format!(
                                "Pairing window open (set_id={:04X}, paired={})",
                                p.set_id, p.paired_count
                            )));
                            self.pair_later(PAIR_WINDOW_S, baseline);
                        }
                        Err(e) => tx.send(BgMsg::PairResult(Err(format!("parse: {e}")))),
                    },
                    Err(e) => tx.send(BgMsg::PairResult(Err(e))),
                }
            }
            Job::PairCheck { remaining, baseline } => {
                if let Ok(r) = self.send(cmd::RF_PAIR_LIST, &[]).await {
                    if let Ok(list) = parsers::parse_pair_list(&r.payload) {
                        let count = list.paired_count;
                        tx.send(BgMsg::RfPairList(list));
                        if count > baseline {
                            tx.send(BgMsg::PairResult(Ok(count)));
                            return;
                        }
                    }
                }
                if remaining == 0 {
                    tx.send(BgMsg::PairResult(Err("Timeout — no half completed the handshake in 30 s".into())));
                } else {
                    self.pair_later(remaining, baseline);
                }
            }
            Job::Wpm => {
                if let Ok(r) = self.send(cmd::WPM_QUERY, &[]).await {
                    let wpm = if r.payload.len() >= 2 { u16::from_le_bytes([r.payload[0], r.payload[1]]) } else { 0 };
                    tx.send(BgMsg::Wpm(wpm));
                }
            }
            Job::DonglePoll { slow } => {
                self.fetch_rf_status().await;
                if slow {
                    self.fetch_battery().await;
                    self.fetch_pair_list().await;
                }
            }
        }
    }

    /// Tick the countdown and check again in a second, leaving the queue free.
    fn pair_later(self: &Rc<Self>, remaining: u8, baseline: u8) {
        self.tx.send(BgMsg::PairTick(remaining));
        let me = self.clone();
        spawn_local(async move {
            sleep(1000.0).await;
            enqueue(&me, Job::PairCheck { remaining: remaining - 1, baseline });
        });
    }

    /// Work through the queue. With the matrix test on, listen in the gaps.
    async fn work(self: Rc<Self>) {
        loop {
            let next = self.queue.borrow_mut().pop_front();
            if let Some(job) = next {
                self.run(job).await;
                continue;
            }
            if !self.link.borrow().matrix_on {
                break;
            }
            match serial_read(MATRIX_POLL_MS).await {
                Ok(chunk) => {
                    let mut bytes = js_sys::Uint8Array::new(&chunk).to_vec();
                    while let Ok((frame, used)) = bp::parse_kr(&bytes) {
                        let start = bytes.windows(2).position(|w| w == [0x4B, 0x52]).unwrap_or(0);
                        bytes.drain(..start + used);
                        self.matrix_frame(&frame);
                    }
                }
                Err(e) => {
                    self.link.borrow_mut().matrix_on = false;
                    self.tx.send(BgMsg::MatrixError(js_text(e)));
                }
            }
        }
        self.running.set(false);
    }
}

fn enqueue(shared: &Rc<Shared>, job: Job) {
    shared.queue.borrow_mut().push_back(job);
    if !shared.running.get() {
        shared.running.set(true);
        let me = shared.clone();
        spawn_local(me.work());
    }
}

impl Backend {
    pub fn new() -> (Self, mpsc::Receiver<BgMsg>) {
        let (tx, rx) = Tx::channel();
        let shared = Rc::new(Shared {
            queue: RefCell::new(VecDeque::new()),
            running: Cell::new(false),
            link: RefCell::new(Link {
                open: false,
                is_dongle: false,
                kind: ConnectionKind::Direct,
                rows: device::DEFAULT_ROWS,
                matrix_on: false,
            }),
            tx: tx.clone(),
        });
        (Self { shared, tx }, rx)
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        self.tx.set_ctx(ctx);
    }

    fn run(&self, job: Job) {
        enqueue(&self.shared, job);
    }

    /// Only when nothing else is going on: a poll must never make actions wait.
    fn poll(&self, job: Job) {
        let idle = self.shared.queue.borrow().is_empty() && !self.shared.running.get();
        if idle && self.shared.link.borrow().open {
            self.run(job);
        }
    }

    /// At start: only a port the person already allowed. Asking needs a click.
    pub fn connect(&self, _port: Option<String>) {
        self.run(Job::Connect { ask: false });
    }

    pub fn connect_chosen(&self, _port: Option<String>) {
        self.run(Job::Connect { ask: true });
    }

    pub fn disconnect(&self) {
        self.shared.link.borrow_mut().matrix_on = false;
        self.run(Job::Disconnect);
    }

    pub fn poll_wpm(&self) {
        self.poll(Job::Wpm);
    }

    pub fn poll_dongle(&self, slow: bool) {
        self.poll(Job::DonglePoll { slow });
    }

    pub fn load_layer(&self, layer: usize, edit: u64) {
        self.run(Job::LoadLayer { layer, edit });
    }

    pub fn load_everything(&self, layers: usize, edit: u64) {
        self.run(Job::LoadEverything { layers, edit });
    }

    pub fn write_keys(&self, writes: Vec<((usize, usize, usize), u16)>) {
        if !writes.is_empty() {
            self.run(Job::WriteKeys(writes));
        }
    }

    pub fn rename_layer(&self, layer: usize, name: String) {
        self.run(Job::RenameLayer { layer, name });
    }

    pub fn load_heatmap(&self) {
        self.run(Job::Heatmap);
    }

    pub fn refresh_stats(&self) {
        self.run(Job::Stats);
    }

    pub fn refresh_advanced(&self) {
        self.run(Job::Advanced);
    }

    pub fn refresh_macros(&self) {
        self.run(Job::Macros);
    }

    pub fn command_then(&self, id: u8, payload: Vec<u8>, refresh: Refresh) {
        self.run(Job::Command { id, payload, refresh });
    }

    pub fn command(&self, id: u8, payload: Vec<u8>) {
        self.command_then(id, payload, Refresh::Nothing);
    }

    pub fn save_text(&self, path: &str, text: &str) -> Result<(), String> {
        download(path, text);
        Ok(())
    }

    pub fn tama_action(&self, action: u8) {
        self.run(Job::Tama(action));
    }

    pub fn toggle_autoshift(&self) {
        self.run(Job::Autoshift);
    }

    pub fn load_layout_from_keyboard(&self) {
        self.run(Job::LayoutFromKeyboard);
    }

    pub fn toggle_matrix(&self) {
        self.shared.link.borrow_mut().matrix_on = false;
        self.run(Job::MatrixToggle);
    }

    pub fn nvs_reset(&self, mask: u8) {
        self.run(Job::NvsReset(mask));
    }

    pub fn ota(&self, _firmware: Vec<u8>) {
        self.tx.send(BgMsg::OtaDone(Err("Firmware updates need the desktop app".into())));
    }

    pub fn flash(&self, _port: String, _firmware: Vec<u8>, _offset: u32) {
        self.tx.send(BgMsg::FlashDone(Err("Firmware updates need the desktop app".into())));
    }

    pub fn export_config(&self, mask: u8, path: String) {
        self.run(Job::Export { mask, name: path });
    }

    pub fn import_config(&self, config: config_io::KeyboardConfig, mask: u8) {
        self.run(Job::Import { config, mask });
    }

    /// Files that are opened ask the browser. Files that are saved are downloads:
    /// they need no choice beforehand.
    pub fn pick(&self, target: PickTarget) {
        match target {
            PickTarget::ConfigExport => self.tx.send(BgMsg::PickedFile(target, "kesp_config.json".into())),
            PickTarget::LayoutExport => self.tx.send(BgMsg::PickedFile(target, "layout.json".into())),
            PickTarget::Ota | PickTarget::Firmware => {}
            PickTarget::ConfigImport | PickTarget::LayoutLoad => {
                let tx = self.tx.clone();
                spawn_local(async move {
                    if let Ok(v) = pick_text(".json").await {
                        let pair = js_sys::Array::from(&v);
                        let name = pair.get(0).as_string().unwrap_or_default();
                        let text = pair.get(1).as_string().unwrap_or_default();
                        tx.send(BgMsg::PickedText(target, name, text));
                    }
                });
            }
        }
    }

    pub fn dongle_refresh(&self) {
        self.run(Job::DongleRefresh);
    }

    pub fn dongle_reset_pairing(&self) {
        self.run(Job::DongleReset);
    }

    pub fn dongle_pair(&self) {
        self.run(Job::PairStart);
    }
}
