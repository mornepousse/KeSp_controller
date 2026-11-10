//! Talks to the keyboard away from the UI thread. Requests go through one
//! ordered queue, so a key written and a layer read right after can't swap
//! places. Replies come back as `BgMsg`. The frames and the order of the steps
//! are the ones of the previous version (see MECHANICS.md).

pub use crate::bg::*;
use crate::protocol::binary::{self as bp, cmd};
use crate::protocol::connection;
use crate::protocol::serial::SerialManager;
use crate::protocol::{config_io, device, flasher, layout, parsers};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

type Job = Box<dyn FnOnce(&mut SerialManager, &Tx) + Send>;

pub struct Backend {
    pub serial: Arc<Mutex<SerialManager>>,
    pub tx: Tx,
    jobs: mpsc::Sender<Job>,
    matrix_on: Arc<AtomicBool>,
}

fn lock(serial: &Mutex<SerialManager>) -> std::sync::MutexGuard<'_, SerialManager> {
    serial.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ports of the family, as (path, label, is dongle).
pub fn list_ports() -> Vec<(String, String, bool)> {
    SerialManager::list_kase_ports()
}

pub fn list_prog_ports() -> Vec<String> {
    SerialManager::list_prog_ports()
}

/// What the port list shows when there is nothing in it.
pub const NO_PORT_LABEL: &str = "No KaSe device found";
/// Firmware updates work from here.
pub const CAN_FLASH: bool = true;

impl Backend {
    pub fn new() -> (Self, mpsc::Receiver<BgMsg>) {
        let (tx, rx) = Tx::channel();
        let serial = Arc::new(Mutex::new(SerialManager::new()));
        let (jobs, job_rx) = mpsc::channel::<Job>();
        {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let mut ser = lock(&serial);
                    job(&mut ser, &tx);
                }
            });
        }
        (Self { serial, tx, jobs, matrix_on: Arc::new(AtomicBool::new(false)) }, rx)
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        self.tx.set_ctx(ctx);
    }

    fn run<F>(&self, f: F)
    where
        F: FnOnce(&mut SerialManager, &Tx) + Send + 'static,
    {
        let _ = self.jobs.send(Box::new(f));
    }

    /// Runs only if the port is free, off the queue: a poll must never make
    /// the person's actions wait.
    fn poll<F>(&self, f: F)
    where
        F: FnOnce(&mut SerialManager, &Tx) + Send + 'static,
    {
        let serial = self.serial.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if let Ok(mut ser) = serial.try_lock() {
                f(&mut ser, &tx);
            }
        });
    }

    pub fn connect(&self, port: Option<String>) {
        self.run(move |ser, tx| connect_and_load(ser, tx, port));
    }

    /// The person pressed Connect. The same here; a browser can only ask then.
    pub fn connect_chosen(&self, port: Option<String>) {
        self.connect(port);
    }

    pub fn poll_wpm(&self) {
        self.poll(fetch_wpm);
    }

    pub fn poll_dongle(&self, slow: bool) {
        self.poll(move |ser, tx| {
            fetch_rf_status(ser, tx);
            if slow {
                fetch_battery(ser, tx);
                fetch_pair_list(ser, tx);
            }
        });
    }

    pub fn disconnect(&self) {
        self.matrix_on.store(false, Ordering::SeqCst);
        self.run(|ser, tx| {
            ser.disconnect();
            tx.send(BgMsg::Disconnected);
        });
    }

    pub fn load_layer(&self, layer: usize, edit: u64) {
        self.run(move |ser, tx| match ser.get_keymap(layer as u8) {
            Ok(km) => tx.send(BgMsg::Keymap(layer, km, Some(edit))),
            Err(e) => tx.send(BgMsg::KeymapError(e.to_string())),
        });
    }

    /// Read every layer but the base one, and what keys can point to: the checks
    /// and the "used on" counts need the whole keymap.
    pub fn load_everything(&self, layers: usize, edit: u64) {
        self.run(move |ser, tx| {
            for layer in 1..layers {
                if let Ok(km) = ser.get_keymap(layer as u8) {
                    tx.send(BgMsg::Keymap(layer, km, Some(edit)));
                }
            }
            fetch_macros(ser, tx);
            fetch_tap_dance(ser, tx);
        });
    }

    pub fn write_keys(&self, writes: Vec<((usize, usize, usize), u16)>) {
        if writes.is_empty() {
            return;
        }
        self.run(move |ser, tx| {
            for ((l, r, c), code) in writes {
                let payload = bp::setkey_payload(l as u8, r as u8, c as u8, code);
                if let Err(e) = ser.send_binary(cmd::SETKEY, &payload) {
                    tx.send(BgMsg::KeyWriteFailed((l, r, c), code, e));
                }
            }
        });
    }

    pub fn rename_layer(&self, layer: usize, name: String) {
        self.run(move |ser, tx| {
            let payload = bp::set_layout_name_payload(layer as u8, &name);
            if let Err(e) = ser.send_binary(cmd::SET_LAYOUT_NAME, &payload) {
                tx.send(BgMsg::StatusError(e));
            }
            if let Ok(names) = ser.get_layer_names() {
                tx.send(BgMsg::LayerNames(names));
            }
        });
    }

    pub fn load_heatmap(&self) {
        self.run(fetch_heatmap);
    }

    pub fn refresh_stats(&self) {
        self.run(|ser, tx| {
            fetch_heatmap(ser, tx);
            fetch_bigrams(ser, tx);
            if let Ok(r) = ser.send_binary(cmd::TAMA_QUERY, &[]) {
                if r.payload.len() >= 22 && tama_present(&r.payload) {
                    tx.send(BgMsg::TamaStats(parse_tama_stats(&r.payload)));
                }
            }
        });
    }

    pub fn refresh_advanced(&self) {
        self.run(|ser, tx| {
            fetch_tap_dance(ser, tx);
            fetch_combos(ser, tx);
            fetch_leaders(ser, tx);
            fetch_key_overrides(ser, tx);
            if let Ok(r) = ser.send_binary(cmd::BT_QUERY, &[]) {
                tx.send(BgMsg::BtStatus(parsers::parse_bt_binary(&r.payload)));
            }
            if let Ok(r) = ser.send_binary(cmd::TAMA_QUERY, &[]) {
                if tama_present(&r.payload) {
                    tx.send(BgMsg::TamaStatus(parsers::parse_tama_binary(&r.payload)));
                }
            }
        });
    }

    pub fn refresh_macros(&self) {
        self.run(fetch_macros);
    }

    /// Send one command, then re-read the list it changed.
    pub fn command_then(&self, command: u8, payload: Vec<u8>, refresh: Refresh) {
        self.run(move |ser, tx| {
            if let Err(e) = ser.send_binary(command, &payload) {
                tx.send(BgMsg::StatusError(e));
            }
            match refresh {
                Refresh::Macros => fetch_macros(ser, tx),
                Refresh::Combos => fetch_combos(ser, tx),
                Refresh::Leaders => fetch_leaders(ser, tx),
                Refresh::KeyOverrides => fetch_key_overrides(ser, tx),
                Refresh::Nothing => {}
            }
        });
    }

    pub fn command(&self, command: u8, payload: Vec<u8>) {
        self.command_then(command, payload, Refresh::Nothing);
    }

    /// A file named by the person; `path` comes from `pick`.
    pub fn save_text(&self, path: &str, text: &str) -> Result<(), String> {
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    pub fn tama_action(&self, action: u8) {
        self.run(move |ser, tx| {
            if let Err(e) = ser.send_binary(action, &[]) {
                tx.send(BgMsg::StatusError(e));
            }
            if let Ok(r) = ser.send_binary(cmd::TAMA_QUERY, &[]) {
                tx.send(BgMsg::TamaStatus(parsers::parse_tama_binary(&r.payload)));
            }
        });
    }

    pub fn toggle_autoshift(&self) {
        self.run(|ser, tx| match ser.send_binary(cmd::AUTOSHIFT_TOGGLE, &[]) {
            Ok(r) => {
                let on = r.payload.first().copied().unwrap_or(0) != 0;
                tx.send(BgMsg::AutoshiftStatus(
                    if on { "Autoshift: ON" } else { "Autoshift: OFF" }.to_string(),
                ));
            }
            Err(e) => tx.send(BgMsg::AutoshiftStatus(format!("Error: {e}"))),
        });
    }

    pub fn load_layout_from_keyboard(&self) {
        self.run(|ser, tx| match ser.get_layout_json() {
            Ok(json) => tx.send(BgMsg::LayoutPreview(json)),
            Err(e) => tx.send(BgMsg::LayoutStatus(format!("Error: {e}"))),
        });
    }

    pub fn toggle_matrix(&self) {
        let serial = self.serial.clone();
        let flag = self.matrix_on.clone();
        // A running poller holds the port most of the time: stop it first so
        // the toggle reaches the keyboard.
        flag.store(false, Ordering::SeqCst);
        self.run(move |ser, tx| match ser.send_binary(cmd::MATRIX_TEST, &[]) {
            Ok(r) if r.payload.len() >= 3 => {
                let on = r.payload[0] != 0;
                tx.send(BgMsg::MatrixToggled(on));
                flag.store(on, Ordering::SeqCst);
                if on {
                    let tx = tx.clone();
                    std::thread::spawn(move || poll_matrix(serial, tx, flag));
                }
            }
            Ok(_) => tx.send(BgMsg::MatrixError("Unexpected reply".into())),
            Err(e) => tx.send(BgMsg::MatrixError(e)),
        });
    }

    pub fn nvs_reset(&self, mask: u8) {
        self.run(move |ser, tx| match ser.send_binary(cmd::NVS_RESET, &[mask]) {
            Ok(_) => {
                ser.disconnect();
                tx.send(BgMsg::Disconnected);
                tx.send(BgMsg::NvsResetDone(Ok(mask)));
            }
            Err(e) => tx.send(BgMsg::NvsResetDone(Err(e))),
        });
    }

    pub fn ota(&self, firmware: Vec<u8>) {
        self.run(move |ser, tx| {
            let r = run_ota(ser, tx, &firmware);
            tx.send(BgMsg::OtaDone(r));
        });
    }

    pub fn flash(&self, port: String, firmware: Vec<u8>, offset: u32) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let (ftx, frx) = mpsc::channel();
            let tx2 = tx.clone();
            let progress = std::thread::spawn(move || {
                while let Ok(flasher::FlashProgress::OtaProgress(p, msg)) = frx.recv() {
                    tx2.send(BgMsg::FlashProgress(p, msg));
                }
            });
            let result = flasher::flash_firmware(&port, &firmware, offset, &ftx);
            drop(ftx);
            let _ = progress.join();
            tx.send(BgMsg::FlashDone(result.map_err(|e| e.to_string())));
        });
    }

    pub fn export_config(&self, mask: u8, path: String) {
        self.run(move |ser, tx| {
            let r = read_config(ser, tx, mask).and_then(|config| {
                tx.send(BgMsg::ConfigProgress(0.90, "Saving file...".into()));
                let json = config.to_json()?;
                std::fs::write(&path, &json).map_err(|e| format!("Write error: {e}"))?;
                Ok(format!("Exported to {path}"))
            });
            tx.send(BgMsg::ConfigDone(r));
        });
    }

    /// Ask for a file away from the UI thread and the serial queue.
    pub fn pick(&self, target: PickTarget) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let d = rfd::FileDialog::new();
            let file = match target {
                PickTarget::Ota | PickTarget::Firmware => d.add_filter("Firmware", &["bin"]).pick_file(),
                PickTarget::ConfigImport => d.add_filter("KeSp Config", &["json"]).pick_file(),
                PickTarget::ConfigExport => d
                    .add_filter("KeSp Config", &["json"])
                    .set_file_name("kesp_config.json")
                    .save_file(),
                PickTarget::LayoutLoad => d.add_filter("Layout JSON", &["json"]).pick_file(),
                PickTarget::LayoutExport => d
                    .add_filter("Layout JSON", &["json"])
                    .set_file_name("layout.json")
                    .save_file(),
            };
            if let Some(path) = file {
                tx.send(BgMsg::PickedFile(target, path.to_string_lossy().to_string()));
            }
        });
    }

    pub fn import_config(&self, config: config_io::KeyboardConfig, mask: u8) {
        self.run(move |ser, tx| {
            let r = write_config(ser, tx, &config, mask);
            tx.send(BgMsg::ConfigDone(r));
        });
    }

    pub fn dongle_refresh(&self) {
        self.run(|ser, tx| {
            if ser.is_dongle {
                fetch_rf_status(ser, tx);
                fetch_pair_list(ser, tx);
                fetch_battery(ser, tx);
            }
        });
    }

    pub fn dongle_reset_pairing(&self) {
        self.run(|ser, tx| {
            if !ser.is_dongle {
                return;
            }
            match ser.send_binary(cmd::RF_PAIR_RESET, &[]) {
                Ok(_) => {
                    tx.send(BgMsg::Status("Pairing forgotten".into()));
                    fetch_pair_list(ser, tx);
                }
                Err(e) => tx.send(BgMsg::StatusError(format!("RF_PAIR_RESET: {e}"))),
            }
        });
    }

    /// Open the 30 s window. It runs off the queue and only holds the port for
    /// each poll, so the rest of the app stays usable while waiting.
    pub fn dongle_pair(&self) {
        let serial = self.serial.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || run_pairing(serial, tx));
    }
}

fn fetch_heatmap(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
        let (data, max) = parsers::parse_keystats_binary(&r.payload);
        tx.send(BgMsg::HeatmapData(data, max));
    }
}

fn fetch_bigrams(ser: &mut SerialManager, tx: &Tx) {
    let lines = match ser.send_binary(cmd::BIGRAMS_TEXT, &[]) {
        Ok(r) => String::from_utf8_lossy(&r.payload).lines().map(str::to_string).collect(),
        Err(_) => Vec::new(),
    };
    tx.send(BgMsg::BigramLines(lines));
}

fn fetch_tap_dance(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::TD_LIST, &[]) {
        tx.send(BgMsg::TdList(parsers::parse_td_binary(&r.payload)));
    }
}

fn fetch_combos(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
        tx.send(BgMsg::ComboList(parsers::parse_combo_binary(&r.payload)));
    }
}

fn fetch_leaders(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
        tx.send(BgMsg::LeaderList(parsers::parse_leader_binary(&r.payload)));
    }
}

fn fetch_key_overrides(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
        tx.send(BgMsg::KoList(parsers::parse_ko_entries(&r.payload)));
    }
}

fn fetch_macros(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::LIST_MACROS, &[]) {
        tx.send(BgMsg::MacroList(parsers::parse_macros_binary(&r.payload)));
    }
}

fn fetch_wpm(ser: &mut SerialManager, tx: &Tx) {
    if let Ok(r) = ser.send_binary(cmd::WPM_QUERY, &[]) {
        let wpm = if r.payload.len() >= 2 {
            u16::from_le_bytes([r.payload[0], r.payload[1]])
        } else {
            0
        };
        tx.send(BgMsg::Wpm(wpm));
    }
}

fn fetch_rf_status(ser: &mut SerialManager, tx: &Tx) {
    if !ser.connected {
        return;
    }
    if let Ok(r) = ser.send_binary(cmd::RF_STATUS, &[]) {
        match parsers::parse_rf_status(&r.payload) {
            Ok(s) => tx.send(BgMsg::RfStatus(s)),
            Err(e) => tx.send(BgMsg::StatusError(format!("RF_STATUS parse: {e}"))),
        }
    }
}

fn fetch_pair_list(ser: &mut SerialManager, tx: &Tx) {
    if !ser.connected {
        return;
    }
    if let Ok(r) = ser.send_binary(cmd::RF_PAIR_LIST, &[]) {
        match parsers::parse_pair_list(&r.payload) {
            Ok(p) => tx.send(BgMsg::RfPairList(p)),
            Err(e) => tx.send(BgMsg::StatusError(format!("RF_PAIR_LIST parse: {e}"))),
        }
    }
}

fn fetch_battery(ser: &mut SerialManager, tx: &Tx) {
    if !ser.connected {
        return;
    }
    if let Ok(r) = ser.send_binary(cmd::BATTERY, &[]) {
        match parsers::parse_battery(&r.payload) {
            Ok(b) => tx.send(BgMsg::Battery(b)),
            Err(e) => tx.send(BgMsg::StatusError(format!("BATTERY parse: {e}"))),
        }
    }
}

/// Same order as before: the layout first, since it carries the geometry that
/// the keymap needs to be cut into rows.
fn connect_and_load(ser: &mut SerialManager, tx: &Tx, port: Option<String>) {
    let result = match port {
        Some(p) => ser.connect(&p).map(|_| p),
        None => ser.auto_connect(),
    };
    let port_name = match result {
        Ok(p) => p,
        Err(e) => {
            tx.send(BgMsg::ConnectError(e));
            return;
        }
    };
    let fw = ser.get_firmware_version().unwrap_or_default();
    tx.send(BgMsg::RfMode(ser.is_dongle));
    tx.send(BgMsg::ConnectionMode(ser.connection_kind()));

    match ser.get_layout_json() {
        Ok(json) => {
            let (rows, cols) = device::resolve_geometry(layout::parse_geometry(&json), None);
            ser.profile.rows = rows;
            ser.profile.cols = cols;
            if let Some(name) = layout::parse_name(&json) {
                ser.profile.name = name;
            }
            match layout::parse_json(&json) {
                Ok(keys) => tx.send(BgMsg::LayoutJson(keys)),
                Err(e) => tx.send(BgMsg::StatusError(format!("Layout parse error: {e}"))),
            }
        }
        // A dongle has no layout: not an error.
        Err(e) if e.is_unsupported() => {}
        Err(e) => tx.send(BgMsg::StatusError(format!("Layout: {e}"))),
    }
    tx.send(BgMsg::Profile(ser.profile.clone()));

    let names = ser.get_layer_names().unwrap_or_default();
    let km = match ser.get_keymap(0) {
        Ok(km) => km,
        Err(e) if e.is_unsupported() => Vec::new(),
        Err(e) => {
            tx.send(BgMsg::StatusError(format!(
                "Keymap ({}x{}): {e}",
                ser.profile.rows, ser.profile.cols
            )));
            Vec::new()
        }
    };
    tx.send(BgMsg::KeyboardReachable(connection::keyboard_reachable(
        ser.connection_kind(),
        !km.is_empty(),
    )));
    tx.send(BgMsg::Connected(port_name, fw, names, km));
}


/// Unsolicited KR [0xB0] frames while the test runs. Bytes are kept between
/// reads, so a frame that arrives in two pieces isn't lost.
fn poll_matrix(serial: Arc<Mutex<SerialManager>>, tx: Tx, on: Arc<AtomicBool>) {
    let mut buf = [0u8; 256];
    let mut pending: Vec<u8> = Vec::new();
    while on.load(Ordering::SeqCst) {
        let read = {
            let Ok(mut ser) = serial.try_lock() else {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            };
            let Some(port) = ser.port_mut() else {
                on.store(false, Ordering::SeqCst);
                tx.send(BgMsg::MatrixError("Port disconnected".into()));
                break;
            };
            let _ = port.set_timeout(Duration::from_millis(50));
            std::io::Read::read(port, &mut buf)
        };
        match read {
            Ok(n) if n > 0 => {
                pending.extend_from_slice(&buf[..n]);
                while let Ok((frame, used)) = bp::parse_kr(&pending) {
                    let start = pending.windows(2).position(|w| w == [0x4B, 0x52]).unwrap_or(0);
                    pending.drain(..start + used);
                    if frame.cmd == cmd::MATRIX_TEST && frame.is_ok() && frame.payload.len() >= 3 {
                        tx.send(BgMsg::MatrixEvent(frame.payload[0], frame.payload[1], frame.payload[2]));
                    }
                }
                if pending.len() > 4096 {
                    pending.clear();
                }
            }
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

fn run_ota(ser: &mut SerialManager, tx: &Tx, firmware: &[u8]) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};
    const CHUNK: usize = 4096;
    let total = firmware.len();
    ser.send_command(&format!("OTA {total}"))
        .map_err(|e| format!("Send OTA cmd failed: {e}"))?;
    tx.send(BgMsg::OtaProgress(0.0, "Waiting for OTA_READY...".into()));
    let port = ser.port_mut().ok_or("Port not available")?;
    let old_timeout = port.timeout();
    let _ = port.set_timeout(Duration::from_secs(5));

    let clone = port.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(clone);
    let mut ready = false;
    for _ in 0..20 {
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() && line.contains("OTA_READY") {
            ready = true;
            break;
        }
    }
    drop(reader);
    if !ready {
        let _ = port.set_timeout(old_timeout);
        return Err("Firmware did not respond OTA_READY".into());
    }

    let chunks = total.div_ceil(CHUNK);
    for (i, chunk) in firmware.chunks(CHUNK).enumerate() {
        if let Err(e) = port.write_all(chunk) {
            let _ = port.set_timeout(old_timeout);
            return Err(format!("Write chunk {i} failed: {e}"));
        }
        let _ = port.flush();
        let progress = (i + 1) as f32 / chunks as f32;
        tx.send(BgMsg::OtaProgress(
            progress * 0.95,
            format!(
                "Chunk {}/{} ({} KB / {} KB)",
                i + 1,
                chunks,
                ((i + 1) * CHUNK).min(total) / 1024,
                total / 1024
            ),
        ));
        let mut ack_buf = [0u8; 256];
        let mut ack = String::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            match port.read(&mut ack_buf) {
                Ok(n) if n > 0 => {
                    ack.push_str(&String::from_utf8_lossy(&ack_buf[..n]));
                    if ack.contains("OTA_OK") || ack.contains("OTA_DONE") || ack.contains("OTA_FAIL") {
                        break;
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        if ack.contains("OTA_FAIL") {
            let _ = port.set_timeout(old_timeout);
            return Err(format!("Firmware error: {}", ack.trim()));
        }
        if ack.contains("OTA_DONE") {
            break;
        }
    }
    let _ = port.set_timeout(old_timeout);
    tx.send(BgMsg::OtaProgress(1.0, "OTA complete, rebooting...".into()));
    Ok(())
}

fn read_config(ser: &mut SerialManager, tx: &Tx, mask: u8) -> Result<config_io::KeyboardConfig, String> {
    use config_io::*;
    let all = mask == 0;
    let mut config = KeyboardConfig {
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
        config.layer_names = ser.get_layer_names().unwrap_or_default();
        let n = config.layer_names.len().max(1);
        for layer in 0..n {
            tx.send(BgMsg::ConfigProgress(
                0.05 + (layer as f32 / n as f32) * 0.30,
                format!("Reading layer {layer}..."),
            ));
            config.keymaps.push(ser.get_keymap(layer as u8).unwrap_or_default());
        }
    }
    if all || mask & 0x08 != 0 {
        tx.send(BgMsg::ConfigProgress(0.40, "Reading tap dances...".into()));
        if let Ok(r) = ser.send_binary(cmd::TD_LIST, &[]) {
            config.tap_dances = parsers::parse_td_binary(&r.payload)
                .iter()
                .enumerate()
                .filter(|(_, a)| a.iter().any(|&x| x != 0))
                .map(|(i, a)| TdConfig { index: i as u8, actions: *a })
                .collect();
        }
        tx.send(BgMsg::ConfigProgress(0.50, "Reading combos...".into()));
        if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
            config.combos = parsers::parse_combo_binary(&r.payload)
                .iter()
                .map(|c| ComboConfig { index: c.index, r1: c.r1, c1: c.c1, r2: c.r2, c2: c.c2, result: c.result })
                .collect();
        }
        tx.send(BgMsg::ConfigProgress(0.60, "Reading key overrides...".into()));
        if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
            config.key_overrides = parsers::parse_ko_binary(&r.payload)
                .iter()
                .map(|k| KoConfig { trigger_key: k[0], trigger_mod: k[1], result_key: k[2], result_mod: k[3] })
                .collect();
        }
        tx.send(BgMsg::ConfigProgress(0.70, "Reading leaders...".into()));
        if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
            config.leaders = parsers::parse_leader_binary(&r.payload)
                .iter()
                .map(|l| LeaderConfig { index: l.index, sequence: l.sequence.clone(), result: l.result, result_mod: l.result_mod })
                .collect();
        }
    }
    if all || mask & 0x02 != 0 {
        tx.send(BgMsg::ConfigProgress(0.80, "Reading macros...".into()));
        if let Ok(r) = ser.send_binary(cmd::LIST_MACROS, &[]) {
            config.macros = parsers::parse_macros_binary(&r.payload)
                .iter()
                .map(|m| {
                    let steps: Vec<String> =
                        m.steps.iter().map(|s| format!("{:02X}:{:02X}", s.keycode, s.modifier)).collect();
                    MacroConfig { slot: m.slot, name: m.name.clone(), steps: steps.join(","), keys: m.keys.clone() }
                })
                .collect();
        }
    }
    Ok(config)
}

fn write_config(
    ser: &mut SerialManager,
    tx: &Tx,
    config: &config_io::KeyboardConfig,
    mask: u8,
) -> Result<String, String> {
    let all = mask == 0;
    let mut errors = 0usize;
    let mut imported = Vec::new();
    let mut send = |ser: &mut SerialManager, id: u8, payload: Vec<u8>| {
        if ser.send_binary(id, &payload).is_err() {
            errors += 1;
        }
    };
    if all || mask & 0x01 != 0 {
        tx.send(BgMsg::ConfigProgress(0.0, "Setting layer names...".into()));
        for (i, name) in config.layer_names.iter().enumerate() {
            send(ser, cmd::SET_LAYOUT_NAME, bp::set_layout_name_payload(i as u8, name));
        }
        for (layer, km) in config.keymaps.iter().enumerate() {
            tx.send(BgMsg::ConfigProgress(
                0.05 + (layer as f32 / config.keymaps.len().max(1) as f32) * 0.30,
                format!("Writing layer {layer}..."),
            ));
            send(ser, cmd::SETLAYER, bp::setlayer_payload(layer as u8, km));
        }
        imported.push(format!("{} layers", config.layer_names.len()));
    }
    if all || mask & 0x08 != 0 {
        tx.send(BgMsg::ConfigProgress(0.40, "Setting tap dances...".into()));
        for td in &config.tap_dances {
            send(ser, cmd::TD_SET, bp::td_set_payload(td.index, &td.actions));
        }
        tx.send(BgMsg::ConfigProgress(0.50, "Setting combos...".into()));
        for c in &config.combos {
            send(ser, cmd::COMBO_SET, bp::combo_set_payload(c.index, c.r1, c.c1, c.r2, c.c2, c.result as u8));
        }
        tx.send(BgMsg::ConfigProgress(0.60, "Setting key overrides...".into()));
        for (i, k) in config.key_overrides.iter().enumerate() {
            send(ser, cmd::KO_SET, bp::ko_set_payload(i as u8, k.trigger_key, k.trigger_mod, k.result_key, k.result_mod));
        }
        tx.send(BgMsg::ConfigProgress(0.70, "Setting leaders...".into()));
        for l in &config.leaders {
            send(ser, cmd::LEADER_SET, bp::leader_set_payload(l.index, &l.sequence, l.result, l.result_mod));
        }
        imported.push(format!(
            "{} TD, {} combos, {} KO, {} leaders",
            config.tap_dances.len(),
            config.combos.len(),
            config.key_overrides.len(),
            config.leaders.len()
        ));
    }
    if all || mask & 0x02 != 0 {
        tx.send(BgMsg::ConfigProgress(0.80, "Setting macros...".into()));
        for m in &config.macros {
            if m.steps.is_empty() && !m.keys.is_empty() {
                send(ser, cmd::MACRO_ADD, bp::macro_add_payload(m.slot, &m.name, &m.keys));
            } else {
                send(ser, cmd::MACRO_ADD_SEQ, bp::macro_add_seq_payload(m.slot, &m.name, &m.steps));
            }
        }
        imported.push(format!("{} macros", config.macros.len()));
    }
    tx.send(BgMsg::ConfigProgress(0.95, "Refreshing...".into()));
    let names = ser.get_layer_names().unwrap_or_default();
    let km = ser.get_keymap(0).unwrap_or_default();
    tx.send(BgMsg::LayerNames(names));
    tx.send(BgMsg::Keymap(0, km, None));
    Ok(if errors > 0 {
        format!("Import done with {errors} errors")
    } else {
        format!("Imported: {}", imported.join(", "))
    })
}

fn run_pairing(serial: Arc<Mutex<SerialManager>>, tx: Tx) {
    let baseline = {
        let mut ser = lock(&serial);
        let baseline = match ser.send_binary(cmd::RF_PAIR_LIST, &[]) {
            Ok(r) => parsers::parse_pair_list(&r.payload).map(|p| p.paired_count).unwrap_or(0),
            Err(_) => 0,
        };
        match ser.send_binary(cmd::RF_PAIR_START, &bp::rf_pair_start_payload(false)) {
            Ok(r) => match parsers::parse_pair_start(&r.payload) {
                Ok(p) => tx.send(BgMsg::Status(format!(
                    "Pairing window open (set_id={:04X}, paired={})",
                    p.set_id, p.paired_count
                ))),
                Err(e) => {
                    tx.send(BgMsg::PairResult(Err(format!("parse: {e}"))));
                    return;
                }
            },
            Err(e) => {
                tx.send(BgMsg::PairResult(Err(e)));
                return;
            }
        }
        baseline
    };
    for remaining in (0..PAIR_WINDOW_S).rev() {
        tx.send(BgMsg::PairTick(remaining + 1));
        std::thread::sleep(Duration::from_secs(1));
        let mut ser = lock(&serial);
        if let Ok(r) = ser.send_binary(cmd::RF_PAIR_LIST, &[]) {
            if let Ok(list) = parsers::parse_pair_list(&r.payload) {
                let count = list.paired_count;
                tx.send(BgMsg::RfPairList(list));
                if count > baseline {
                    tx.send(BgMsg::PairResult(Ok(count)));
                    return;
                }
            }
        }
    }
    tx.send(BgMsg::PairResult(Err("Timeout — nothing completed the handshake in 30 s".into())));
}


#[cfg(test)]
mod tests {
    //! Runs the real serial code against a fake firmware on a pseudo-terminal:
    //! no keyboard is involved, and the bytes on the wire are the ones checked.
    use super::*;
    use serialport::{SerialPort, TTYPort};
    use std::io::{Read, Write};

    const ROWS: usize = 2;
    const COLS: usize = 3;

    fn kr(cmd_id: u8, status: u8, payload: &[u8]) -> Vec<u8> {
        let len = payload.len() as u16;
        let mut f = vec![0x4B, 0x52, cmd_id, status, (len & 0xFF) as u8, (len >> 8) as u8];
        f.extend_from_slice(payload);
        f.push(bp::crc8(payload));
        f
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Device {
        Keyboard,
        /// Runs the keymap itself, pair list empty: what a Niphargus dongle answers.
        DongleWithKeymap,
        /// Answers ERR_UNKNOWN to keymap commands.
        DongleAlone,
    }

    /// Answers like a small 2x3 keyboard and records every SETKEY it receives.
    fn fake_firmware(mut master: TTYPort, writes: Arc<Mutex<Vec<Vec<u8>>>>, stop: Arc<AtomicBool>, device: Device) {
        let _ = master.set_timeout(Duration::from_millis(20));
        let mut km = vec![0x04u16, 0x05, 0x06, 0x07, 0x08, 0x09];
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        while !stop.load(Ordering::SeqCst) {
            if let Ok(n) = master.read(&mut chunk) {
                buf.extend_from_slice(&chunk[..n]);
            }
            while buf.len() >= 6 {
                let Some(start) = buf.windows(2).position(|w| w == [0x4B, 0x53]) else {
                    buf.clear();
                    break;
                };
                buf.drain(..start);
                if buf.len() < 6 {
                    break;
                }
                let len = buf[3] as usize | (buf[4] as usize) << 8;
                if buf.len() < 6 + len {
                    break;
                }
                let id = buf[2];
                let payload = buf[5..5 + len].to_vec();
                assert_eq!(buf[5 + len], bp::crc8(&payload), "bad CRC from the app");
                buf.drain(..6 + len);
                let reply = match id {
                    cmd::PING => kr(id, 0, &[]),
                    cmd::FEATURES if device == Device::Keyboard => kr(id, 0, b"MT,LT,COMBO,"),
                    cmd::FEATURES => kr(id, 0, b"MT,LT,COMBO,RF_DONGLE,RF_STATUS,RF_PAIR,BATTERY"),
                    cmd::RF_PAIR_LIST if device != Device::Keyboard => kr(id, 0, &[0u8; 13]),
                    cmd::GET_LAYOUT_JSON | cmd::KEYMAP_GET if device == Device::DongleAlone => {
                        kr(id, bp::status::ERR_UNKNOWN, &[])
                    }
                    cmd::VERSION => kr(id, 0, b"fw-test"),
                    cmd::GET_LAYOUT_JSON => kr(
                        id,
                        0,
                        br#"{"name":"Fake Left","rows":2,"cols":3,"keys":[{"row":0,"col":0,"x":0,"y":0}]}"#,
                    ),
                    cmd::LAYER_NAME => kr(id, 0, &[]),
                    cmd::KEYMAP_GET => {
                        let mut p = vec![payload[0]];
                        for v in &km {
                            p.extend_from_slice(&v.to_le_bytes());
                        }
                        kr(id, 0, &p)
                    }
                    cmd::SETKEY => {
                        writes.lock().unwrap().push(payload.clone());
                        let code = u16::from_le_bytes([payload[3], payload[4]]);
                        km[payload[1] as usize * COLS + payload[2] as usize] = code;
                        kr(id, 0, &[])
                    }
                    _ => kr(id, bp::status::ERR_UNKNOWN, &[]),
                };
                let _ = master.write_all(&reply);
                let _ = master.flush();
            }
        }
    }

    fn wait_for<T>(rx: &mpsc::Receiver<BgMsg>, pick: impl Fn(BgMsg) -> Option<T>) -> T {
        let end = Instant::now() + Duration::from_secs(15);
        while Instant::now() < end {
            if let Ok(m) = rx.recv_timeout(Duration::from_millis(200)) {
                if let Some(v) = pick(m) {
                    return v;
                }
            }
        }
        panic!("no such message from the backend");
    }

    #[test]
    fn connecting_loads_the_keymap_and_edits_reach_the_keyboard_as_setkey_frames() {
        let (master, slave) = TTYPort::pair().expect("pseudo-terminal");
        let name = slave.name().expect("slave name");
        drop(slave);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let fw = {
            let (w, s) = (writes.clone(), stop.clone());
            std::thread::spawn(move || fake_firmware(master, w, s, Device::Keyboard))
        };

        let (be, rx) = Backend::new();
        be.connect(Some(name.clone()));
        let (port, km) = wait_for(&rx, |m| match m {
            BgMsg::ConnectError(e) => panic!("connect failed: {e}"),
            BgMsg::Connected(p, _, _, km) => Some((p, km)),
            _ => None,
        });
        assert_eq!(port, name);
        assert_eq!(km.len(), ROWS);
        assert_eq!(km[0], vec![0x04, 0x05, 0x06]);
        assert_eq!(km[1], vec![0x07, 0x08, 0x09]);

        be.write_keys(vec![((0, 1, 2), 0x512C)]);
        be.load_layer(0, 0);
        let after = wait_for(&rx, |m| match m {
            BgMsg::Keymap(0, km, _) => Some(km),
            BgMsg::KeyWriteFailed(_, _, e) => panic!("write refused: {e}"),
            _ => None,
        });
        assert_eq!(after[1][2], 0x512C, "the write must land before the read that follows it");
        assert_eq!(writes.lock().unwrap().as_slice(), &[vec![0, 1, 2, 0x2C, 0x51]]);

        // Writing a whole layer must stay quick: a reply ends the wait as soon as
        // it is complete, not after a port timeout of silence (300 ms each).
        let start = Instant::now();
        be.write_keys((0..20).map(|_| ((0, 0, 0), 0x04)).collect());
        be.load_layer(0, 0);
        wait_for(&rx, |m| matches!(m, BgMsg::Keymap(0, _, _)).then_some(()));
        assert!(start.elapsed() < Duration::from_secs(3), "20 writes took {:?}", start.elapsed());

        be.disconnect();
        wait_for(&rx, |m| matches!(m, BgMsg::Disconnected).then_some(()));
        stop.store(true, Ordering::SeqCst);
        fw.join().unwrap();
    }

    /// Connects to a fake device and returns what the app is told about it.
    fn connect_to(device: Device) -> (bool, Vec<Vec<u16>>) {
        let (master, slave) = TTYPort::pair().expect("pseudo-terminal");
        let name = slave.name().expect("slave name");
        drop(slave);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let fw = {
            let (w, s) = (writes.clone(), stop.clone());
            std::thread::spawn(move || fake_firmware(master, w, s, device))
        };
        let (be, rx) = Backend::new();
        be.connect(Some(name));
        let mut reachable = None;
        let end = Instant::now() + Duration::from_secs(15);
        let km = loop {
            assert!(Instant::now() < end, "no Connected from the backend");
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(BgMsg::KeyboardReachable(r)) => reachable = Some(r),
                Ok(BgMsg::Connected(_, _, _, km)) => break km,
                Ok(BgMsg::ConnectError(e)) => panic!("connect failed: {e}"),
                _ => {}
            }
        };
        be.disconnect();
        wait_for(&rx, |m| matches!(m, BgMsg::Disconnected).then_some(()));
        stop.store(true, Ordering::SeqCst);
        fw.join().unwrap();
        (reachable.expect("told whether the keyboard is reachable"), km)
    }

    #[test]
    fn a_dongle_with_its_own_keymap_loads_it_even_with_nothing_paired() {
        let (reachable, km) = connect_to(Device::DongleWithKeymap);
        assert!(reachable);
        assert_eq!(km[0], vec![0x04, 0x05, 0x06]);
    }

    #[test]
    fn a_dongle_without_a_keymap_reports_nothing_to_edit() {
        let (reachable, km) = connect_to(Device::DongleAlone);
        assert!(!reachable);
        assert!(km.is_empty());
    }

    #[test]
    fn a_pet_answering_only_zeros_counts_as_absent() {
        assert!(!tama_present(&[0u8; 22]));
        let mut p = [0u8; 22];
        p[10] = 3;
        assert!(tama_present(&p));
    }

    #[test]
    fn tama_stats_read_the_documented_offsets() {
        let mut p = vec![0u8; 22];
        p[0] = 1;
        p[2] = 10; // hunger
        p[4] = 20; // happiness
        p[6] = 30; // energy
        p[8] = 40; // health
        p[10] = 3; // level
        p[12] = 50; // xp
        p[14] = 60; // total keys
        p[18] = 70; // max kpm
        let t = parse_tama_stats(&p);
        assert!(t.enabled);
        assert_eq!((t.hunger, t.happiness, t.energy, t.health), (10, 20, 30, 40));
        assert_eq!((t.level, t.xp, t.total_keys, t.max_kpm), (3, 50, 60, 70));
    }
}
