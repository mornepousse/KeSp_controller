//! Application state, messages from the keyboard, window frame and dialogs.

use crate::backend::{self, Backend, BgMsg, PickTarget, TamaStats};
use crate::boards::{self, Board};
use crate::editor::{KeymapDoc, Pos};
use crate::keys::{self, KeyEntry};
use crate::protocol::connection::ConnectionKind;
use crate::protocol::layout::KeycapPos;
use crate::protocol::layout_remap::KeyboardLayout;
use crate::protocol::{binary::cmd, config_io, keycode, parsers, settings, stats};
use crate::theme;
use crate::ui_layout::LayoutEditor;
use eframe::egui::{self, Color32, Frame, RichText};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use web_time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tab {
    Keymap,
    Layout,
    Behaviors,
    Typing,
    Practice,
    Device,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Keymap, Tab::Layout, Tab::Behaviors, Tab::Typing, Tab::Practice, Tab::Device];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Keymap => "Keymap",
            Tab::Layout => "Layout",
            Tab::Behaviors => "Behaviors",
            Tab::Typing => "Typing",
            Tab::Practice => "Practice",
            Tab::Device => "Device",
        }
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn from_name(s: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.title().eq_ignore_ascii_case(s))
    }

    /// These need a keyboard that answers keymap commands.
    fn needs_keyboard(self) -> bool {
        matches!(self, Tab::Keymap | Tab::Behaviors | Tab::Typing)
    }
}

/// Pages of the Behaviors tab.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Behavior {
    Macros,
    TapDance,
    Combos,
    Overrides,
    Leader,
    Layers,
}

/// Pages of the Device tab.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DevicePage {
    Status,
    Backup,
    Firmware,
    MatrixTest,
    Reset,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Conn {
    Disconnected,
    Connecting,
    Connected,
}

/// Where the key selector sends what the person picks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Keymap,
    ComboResult,
    KoTrigger,
    KoResult,
    LeaderSeq,
    LeaderResult,
    TdAction(usize, usize),
    MacroStep,
}

impl Target {
    /// These slots are one byte in the protocol: a layer or Mod-Tap code would
    /// lose its upper half and silently become another key.
    pub fn basic_keys_only(self) -> bool {
        !matches!(self, Target::Keymap)
    }
}

pub enum DragItem {
    /// A key of the keyboard, by layout index.
    Key(usize),
    /// A keycode coming from the palette.
    Code(u16),
}

pub enum Action {
    /// A configuration file, written once the person confirms.
    ImportConfig(Box<config_io::KeyboardConfig>),
    NvsReset(u8),
    Flash { port: String, path: String, offset: u32 },
    Ota(String),
    ResetPairing,
    SendPending,
    RestoreFactoryLayer,
    ResetPractice,
}

pub struct Confirm {
    pub title: String,
    pub body: String,
    pub ok: String,
    pub action: Action,
}

#[derive(Default)]
pub struct MacroBuilder {
    pub name: String,
    pub slot: i32,
    pub steps: Vec<(u8, u8)>,
}

#[derive(Default)]
pub struct AdvancedState {
    pub tap_dances: Vec<[u16; 4]>,
    pub combos: Vec<parsers::ComboEntry>,
    pub leaders: Vec<parsers::LeaderEntry>,
    pub key_overrides: Vec<parsers::KoEntry>,
    pub bt_status: String,
    pub tama_status: String,
    pub autoshift: String,
    pub tri: [usize; 3],
    pub combo_keys: [Option<(usize, usize)>; 2],
    pub combo_result: Option<u16>,
    pub ko_trigger: Option<u16>,
    pub ko_result: Option<u16>,
    pub ko_trig_mods: [bool; 3],
    pub ko_res_mods: [bool; 3],
    pub leader_seq: Vec<u16>,
    pub leader_result: Option<u16>,
    pub leader_mod: usize,
}

#[derive(Default)]
pub struct StatsState {
    pub heat: Vec<Vec<u32>>,
    pub heat_max: u32,
    pub balance: Option<stats::HandBalance>,
    pub fingers: Vec<stats::FingerLoad>,
    pub rows: Vec<stats::RowUsage>,
    pub top: Vec<stats::TopKey>,
    pub dead: Vec<String>,
    pub bigrams: Option<stats::BigramAnalysis>,
    pub tama: Option<TamaStats>,
}

#[derive(Default)]
pub struct DongleState {
    pub rf: parsers::RfStatus,
    pub paired: u8,
    pub mac_left: String,
    pub mac_right: String,
    pub battery: Option<parsers::Battery>,
    pub pair_countdown: Option<u8>,
    pub pair_status: String,
}

#[derive(Default)]
pub struct ProgressState {
    pub path: String,
    pub progress: f32,
    pub status: String,
    pub busy: bool,
}

#[derive(Default)]
pub struct ToolsState {
    pub matrix_on: bool,
    pub matrix_status: String,
    pub matrix_log: VecDeque<String>,
    pub matrix_colors: HashMap<(usize, usize), Color32>,
    pub nvs: [bool; 6],
    pub nvs_status: String,
    pub prog_ports: Vec<String>,
    pub prog_port: String,
    pub flash_offset: usize,
}

pub struct Options {
    pub screenshot: Option<PathBuf>,
    pub auto_connect: bool,
    pub demo: bool,
    pub tab: Tab,
    pub selector: bool,
    pub check: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { screenshot: None, auto_connect: true, demo: false, tab: Tab::Keymap, selector: false, check: false }
    }
}

pub struct App {
    pub be: Backend,
    rx: mpsc::Receiver<BgMsg>,

    pub conn: Conn,
    pub ports: Vec<(String, String, bool)>,
    pub port_idx: usize,
    pub firmware: String,
    pub mode_label: String,
    pub reachable: bool,
    pub is_dongle: bool,
    pub status: String,
    pub wpm: u16,
    pub device_name: String,
    pub device_cols: usize,

    pub tab: Tab,
    pub behavior: Behavior,
    pub device_page: DevicePage,
    /// Where the combo keyboard is on screen this frame.
    pub combo_view: Option<crate::shape::View>,
    pub boards: Vec<Board>,
    /// Model shown: chosen by hand offline, read from the keyboard when connected.
    pub board: usize,
    pub layout_board: usize,
    /// Model of the connected keyboard, when its reported name is a known one.
    pub device_board: Option<usize>,
    /// The connected keyboard described its own shape.
    pub layout_from_device: bool,
    /// Where the keyboard is on screen this frame.
    pub kb_view: Option<crate::shape::View>,
    pub keys: Vec<KeycapPos>,
    pub layer_names: Vec<String>,
    pub layer: usize,
    pub kb_layout: KeyboardLayout,
    pub catalog: Vec<KeyEntry>,
    pub doc: KeymapDoc,
    /// The Practice tab's session, made on its first visit.
    pub practice: Option<crate::practice::Session>,

    pub selected: Option<usize>,
    pub capture: bool,
    pub tint: bool,
    pub heatmap_on: bool,
    pub find: String,
    pub palette_open: bool,
    pub check_open: bool,
    /// Counts edits sent to the keyboard, to spot a read that an edit overtook.
    edits: u64,
    pub macros_loaded: bool,
    pub tap_dances_loaded: bool,
    pub palette_section: usize,
    pub drag: Option<DragItem>,

    pub selector: Option<Target>,
    pub search: String,
    pub hex_input: String,
    pub mt_mod: usize,
    pub mt_key: usize,
    pub lt_layer: usize,
    pub lt_key: usize,
    pub osm_mod: usize,
    pub lm_layer: usize,
    pub lm_mod: usize,
    pub mk_mod: usize,
    pub mk_key: usize,

    pub rename: Option<String>,
    pub confirm: Option<Confirm>,
    pub sec_warning: bool,

    pub adv: AdvancedState,
    pub macros: Vec<parsers::MacroEntry>,
    pub mac: MacroBuilder,
    pub stats: StatsState,
    pub dongle: DongleState,
    pub tools: ToolsState,
    pub ota: ProgressState,
    pub flash: ProgressState,
    pub config: ProgressState,
    pub config_mask: [bool; 3],
    pub layout_ed: LayoutEditor,

    next_wpm: Instant,
    next_layout: Instant,
    next_dongle: Instant,
    /// While disconnected, the ports seen at the last look: a new one is connected to.
    watch_ports: bool,
    next_port_scan: Instant,
    seen_ports: Vec<String>,
    dongle_tick: u32,

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    screenshot: Option<PathBuf>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    frames: u32,
    /// 0 = waiting, 1 = saved, 2 = failed.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    shot_state: Arc<AtomicU8>,
    themed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
const SCREENSHOT_FRAME: u32 = 120;
const WPM_PERIOD: Duration = Duration::from_secs(5);
const LAYOUT_PERIOD: Duration = Duration::from_secs(5);
const DONGLE_PERIOD: Duration = Duration::from_secs(1);
/// How often the ports are looked at while disconnected, to catch a keyboard plugged in.
const PORT_SCAN_PERIOD: Duration = Duration::from_secs(2);

impl App {
    pub fn new(opt: Options) -> Self {
        let (be, rx) = Backend::new();
        let boards = boards::all();
        let saved = settings::load();
        let board = boards.iter().position(|b| b.id == saved.board).unwrap_or(0);
        let keys = boards[board].keys.clone();
        let kb_layout = KeyboardLayout::from_name(&saved.keyboard_layout);
        let prog_ports = backend::list_prog_ports();
        let mut app = Self {
            be,
            rx,
            conn: Conn::Disconnected,
            ports: backend::list_ports(),
            port_idx: 0,
            firmware: String::new(),
            mode_label: String::new(),
            reachable: true,
            is_dongle: false,
            status: "Disconnected".into(),
            wpm: 0,
            device_name: String::new(),
            device_cols: 13,
            tab: opt.tab,
            behavior: Behavior::Macros,
            device_page: DevicePage::Status,
            combo_view: None,
            layout_ed: LayoutEditor::new(keys.clone()),
            keys,
            layer_names: boards[board].layer_names.clone(),
            board,
            layout_board: board,
            device_board: None,
            layout_from_device: false,
            kb_view: None,
            boards,
            layer: 0,
            catalog: keys::catalog(&kb_layout),
            kb_layout,
            doc: KeymapDoc::default(),
            practice: None,
            selected: None,
            capture: false,
            tint: true,
            heatmap_on: false,
            find: String::new(),
            palette_open: true,
            check_open: opt.check,
            edits: 0,
            macros_loaded: false,
            tap_dances_loaded: false,
            palette_section: 0,
            drag: None,
            selector: opt.selector.then_some(Target::Keymap),
            search: String::new(),
            hex_input: String::new(),
            mt_mod: 1,
            mt_key: 0,
            lt_layer: 1,
            lt_key: 0,
            osm_mod: 1,
            lm_layer: 1,
            lm_mod: 0,
            mk_mod: 0,
            mk_key: 0,
            rename: None,
            confirm: None,
            sec_warning: false,
            adv: AdvancedState {
                tap_dances: vec![[0; 4]; parsers::TD_SLOTS],
                tri: [1, 2, 3],
                ..Default::default()
            },
            macros: Vec::new(),
            mac: MacroBuilder::default(),
            stats: StatsState::default(),
            dongle: DongleState {
                mac_left: "—".into(),
                mac_right: "—".into(),
                ..Default::default()
            },
            tools: ToolsState {
                prog_port: prog_ports.first().cloned().unwrap_or_default(),
                prog_ports,
                flash_offset: 1,
                ..Default::default()
            },
            ota: ProgressState::default(),
            flash: ProgressState::default(),
            config: ProgressState::default(),
            config_mask: [false; 3],
            next_wpm: Instant::now() + WPM_PERIOD,
            next_layout: Instant::now() + LAYOUT_PERIOD,
            next_dongle: Instant::now() + DONGLE_PERIOD,
            watch_ports: opt.auto_connect && !opt.demo && opt.screenshot.is_none(),
            next_port_scan: Instant::now() + PORT_SCAN_PERIOD,
            seen_ports: Vec::new(),
            dongle_tick: 0,
            screenshot: opt.screenshot,
            frames: 0,
            shot_state: Arc::new(AtomicU8::new(0)),
            themed: false,
        };
        app.layout_ed.cols = app.boards[board].cols;
        // Only a port that comes after this counts as plugged in: a Disconnect or a
        // port that won't answer must not be retried every few seconds.
        app.seen_ports = app.ports.iter().map(|p| p.0.clone()).collect();
        app.load_factory_keymap();
        if opt.demo {
            crate::demo::load(&mut app);
        } else if opt.auto_connect {
            app.status = "Scanning ports...".into();
            app.conn = Conn::Connecting;
            app.be.connect(None);
        }
        app
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn screenshot_failed(&self) -> bool {
        self.shot_state.load(Ordering::SeqCst) == 2
    }

    pub fn online(&self) -> bool {
        self.conn == Conn::Connected
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    // ---------------------------------------------------------------- models

    /// Offline, show the model's factory keymap as what the keyboard would hold.
    fn load_factory_keymap(&mut self) {
        self.doc.forget_device();
        if let Some(b) = self.boards.get(self.board) {
            for (i, km) in b.keymaps.iter().enumerate() {
                self.doc.set_layer(i, km.clone());
            }
        }
    }

    /// Draw another model while no keyboard is connected.
    pub fn show_board(&mut self, idx: usize) {
        let Some(b) = self.boards.get(idx) else { return };
        self.keys = b.keys.clone();
        self.layer_names = b.layer_names.clone();
        self.layout_ed.cols = b.cols;
        self.status = format!("{}: {} keys", b.name, b.keys.len());
        self.board = idx;
        self.layer = 0;
        self.selected = None;
        // Unsent edits point at this model's matrix: they mean nothing on another.
        self.doc.discard_pending();
        self.load_factory_keymap();
    }

    /// The person's choice of model, remembered for the next start.
    fn choose_board(&mut self, idx: usize) {
        self.show_board(idx);
        if let Some(b) = self.boards.get(idx) {
            let mut s = settings::load();
            s.board = b.id.clone();
            settings::save(&s);
        }
    }

    /// This layer of the firmware's default keymap. Connected, only if the keyboard
    /// is a known model: another model's keys must never be written to it.
    pub fn factory_layer(&self) -> Option<&Vec<Vec<u16>>> {
        let model = if self.online() { self.device_board? } else { self.board };
        self.boards.get(model)?.keymaps.get(self.layer)
    }

    /// Put this layer back to the firmware's default keymap: one undo step.
    pub fn restore_factory_layer(&mut self) {
        let Some(km) = self.factory_layer().cloned() else { return };
        let edits: Vec<(Pos, u16)> = self
            .keys
            .iter()
            .filter_map(|k| Some(((self.layer, k.row, k.col), *km.get(k.row)?.get(k.col)?)))
            .collect();
        self.apply_edits(&edits);
        self.status = format!("Layer {} back to the factory keymap", self.layer);
    }

    // ---------------------------------------------------------------- keymap

    pub fn pos_of(&self, key_idx: usize) -> Option<Pos> {
        let k = self.keys.get(key_idx)?;
        Some((self.layer, k.row, k.col))
    }

    /// Send edits to the keyboard, if there is one.
    fn send_writes(&mut self, writes: Vec<(Pos, u16)>) {
        if self.online() && !writes.is_empty() {
            self.edits += 1;
            self.be.write_keys(writes);
        }
    }

    /// Edit keys, one undo step. Connected, the writes go out in order.
    pub fn apply_edits(&mut self, edits: &[(Pos, u16)]) {
        let writes = self.doc.apply(edits, self.online());
        self.send_writes(writes);
    }

    /// Assign a keycode to the selected key.
    pub fn assign_selected(&mut self, code: u16) {
        if let Some(i) = self.selected {
            self.assign_key(i, code);
        } else {
            self.set_status("Select a key first");
        }
    }

    pub fn assign_key(&mut self, key_idx: usize, code: u16) {
        let Some(pos) = self.pos_of(key_idx) else { return };
        self.apply_edits(&[(pos, code)]);
        // Sec Confirm on the right half: the firmware ignores the press. Warn on
        // this exact code, never on the 0x3E00-0x3EFF range: the other chest keys
        // aren't restricted.
        if code == 0x3E00 {
            self.sec_warning = backend::sec_confirm_misplaced(&self.keys, key_idx, &self.device_name);
        }
        let sent = if self.online() { "" } else { " (not sent: no keyboard)" };
        self.status = format!("[{},{}] = 0x{:04X}{}", pos.1, pos.2, code, sent);
    }

    pub fn swap_keys(&mut self, a: usize, b: usize) {
        let (Some(pa), Some(pb)) = (self.pos_of(a), self.pos_of(b)) else { return };
        let writes = self.doc.swap(pa, pb, self.online());
        self.send_writes(writes);
        self.status = format!("Swapped R{}C{} and R{}C{}", pa.1, pa.2, pb.1, pb.2);
    }

    pub fn undo(&mut self) {
        let writes = self.doc.undo(self.online());
        self.send_writes(writes);
        self.set_status("Undo");
    }

    pub fn redo(&mut self) {
        let writes = self.doc.redo(self.online());
        self.send_writes(writes);
        self.set_status("Redo");
    }

    pub fn copy_layer(&mut self) {
        let mut grid: Vec<Vec<u16>> = Vec::new();
        for k in &self.keys {
            if let Some(code) = self.doc.get((self.layer, k.row, k.col)) {
                if grid.len() <= k.row {
                    grid.resize(k.row + 1, Vec::new());
                }
                if grid[k.row].len() <= k.col {
                    grid[k.row].resize(k.col + 1, 0);
                }
                grid[k.row][k.col] = code;
            }
        }
        if grid.is_empty() {
            self.set_status("Nothing to copy on this layer");
        } else {
            self.doc.layer_clipboard = Some(grid);
            self.status = format!("Copied layer {}", self.layer);
        }
    }

    pub fn paste_layer(&mut self) {
        let Some(grid) = self.doc.layer_clipboard.clone() else { return };
        let edits: Vec<(Pos, u16)> = self
            .keys
            .iter()
            .filter_map(|k| {
                let code = *grid.get(k.row)?.get(k.col)?;
                Some(((self.layer, k.row, k.col), code))
            })
            .collect();
        self.apply_edits(&edits);
        self.status = format!("Pasted onto layer {}", self.layer);
    }

    pub fn send_pending(&mut self) {
        let writes = self.doc.take_pending();
        self.status = format!("Sending {} key(s)...", writes.len());
        self.send_writes(writes);
    }

    pub fn switch_layer(&mut self, idx: usize) {
        self.layer = idx;
        if self.online() {
            self.status = format!("Loading layer {idx}...");
            self.be.load_layer(idx, self.edits);
        }
    }

    pub fn set_kb_layout(&mut self, l: KeyboardLayout) {
        self.kb_layout = l;
        self.catalog = keys::catalog(&l);
        let mut s = settings::load();
        s.keyboard_layout = l.name().to_string();
        settings::save(&s);
    }

    // ------------------------------------------------------------- selector

    pub fn open_selector(&mut self, target: Target) {
        self.selector = Some(target);
        self.search.clear();
    }

    pub fn pick_code(&mut self, target: Target, code: u16) {
        if target.basic_keys_only() && code > 0xFF {
            self.status = format!("This slot only stores a basic key, not 0x{code:04X}");
            return;
        }
        let name = keycode::decode_keycode(code);
        match target {
            Target::Keymap => self.assign_selected(code),
            Target::ComboResult => self.adv.combo_result = Some(code),
            Target::KoTrigger => self.adv.ko_trigger = Some(code),
            Target::KoResult => self.adv.ko_result = Some(code),
            Target::LeaderResult => self.adv.leader_result = Some(code),
            Target::LeaderSeq => {
                if self.adv.leader_seq.len() < 4 {
                    self.adv.leader_seq.push(code);
                }
            }
            Target::TdAction(idx, slot) => {
                if let Some(td) = self.adv.tap_dances.get_mut(idx) {
                    td[slot] = code;
                    let payload = crate::protocol::binary::td_set_payload(idx as u8, td);
                    self.be.command(cmd::TD_SET, payload);
                    self.status = format!("TD{idx} slot {slot} = {name}");
                }
            }
            Target::MacroStep => self.mac.steps.push((code as u8, 0)),
        }
    }

    // ------------------------------------------------------------- messages

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.capture = false;
        self.refresh_tab();
    }

    /// Read what the open tab shows. Also on connecting: a tab already open would
    /// otherwise stay empty until the person leaves it and comes back.
    fn refresh_tab(&mut self) {
        if !self.online() {
            return;
        }
        match self.tab {
            Tab::Behaviors => {
                self.be.refresh_advanced();
                self.be.refresh_macros();
            }
            Tab::Typing => self.be.refresh_stats(),
            Tab::Device if self.is_dongle => self.be.dongle_refresh(),
            _ => {}
        }
    }

    /// Every key of every layer known so far, for the checks and the "used on" counts.
    pub fn all_keys(&self) -> Vec<crate::checks::Key> {
        let mut out = Vec::with_capacity(self.keys.len() * self.layer_names.len());
        for layer in 0..self.layer_names.len() {
            for (i, k) in self.keys.iter().enumerate() {
                if let Some(code) = self.doc.get((layer, k.row, k.col)) {
                    out.push(crate::checks::Key {
                        layer,
                        row: k.row,
                        col: k.col,
                        code,
                        on_right_half: code == 0x3E00
                            && backend::sec_confirm_misplaced(&self.keys, i, &self.device_name),
                    });
                }
            }
        }
        out
    }

    pub fn findings(&self) -> Vec<crate::checks::Finding> {
        let keys = self.all_keys();
        let slots: Vec<u8> = self.macros.iter().map(|m| m.slot).collect();
        let dances: Vec<usize> = (0..self.adv.tap_dances.len())
            .filter(|&i| self.adv.tap_dances[i].iter().any(|&a| a != 0))
            .collect();
        crate::checks::run(&crate::checks::Input {
            keys: &keys,
            layer_names: &self.layer_names,
            macros: self.macros_loaded.then_some(slots.as_slice()),
            tap_dances: self.tap_dances_loaded.then_some(dances.as_slice()),
        })
    }

    /// Show a key on the Keymap tab.
    pub fn jump_to(&mut self, layer: usize, key: Option<(usize, usize)>) {
        self.tab = Tab::Keymap;
        if layer != self.layer {
            self.switch_layer(layer);
        }
        if let Some((row, col)) = key {
            self.selected = self.keys.iter().position(|k| k.row == row && k.col == col);
        }
    }

    /// (rows, columns) of the keyboard, from the freshest thing it told us.
    fn matrix_size(&self) -> (usize, usize) {
        // The keymap first: the firmware's counters can cover only one half.
        if let Some(first) = self.doc.layer(0).and_then(|km| km.first()) {
            return (self.doc.layer(0).map_or(0, Vec::len), first.len());
        }
        if let Some(first) = self.stats.heat.first() {
            return (self.stats.heat.len(), first.len());
        }
        (5, 13)
    }

    pub fn current_keymap(&self) -> Vec<Vec<u16>> {
        self.doc.layer(self.layer).cloned().unwrap_or_default()
    }

    pub fn apply_heat(&mut self, data: Vec<Vec<u32>>, max: u32) {
        let km = self.current_keymap();
        let balance = stats::hand_balance(&data);
        self.status = format!("Stats loaded ({} total presses, max {})", balance.total, max);
        self.stats.fingers = stats::finger_load(&data);
        self.stats.rows = stats::row_usage(&data);
        self.stats.top = stats::top_keys(&data, &km, 10);
        self.stats.dead = stats::dead_keys(&data, &km);
        self.stats.balance = Some(balance);
        self.stats.heat = data;
        self.stats.heat_max = max;
    }

    fn handle_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::Connected(port, fw, names, km) => {
                self.conn = Conn::Connected;
                self.firmware = fw;
                self.status = if self.reachable {
                    format!("Connected to {port}")
                } else {
                    "Aucun clavier joignable via le dongle".to_string()
                };
                if !names.is_empty() {
                    self.layer_names = names;
                }
                self.layer = 0;
                self.selected = self.selected.filter(|&i| i < self.keys.len());
                self.doc.forget_device();
                // Nothing came back: keep the model's factory keymap off the screen,
                // it isn't what this device holds.
                self.doc.set_layer(0, km);
                self.next_wpm = Instant::now() + WPM_PERIOD;
                if self.reachable {
                    self.be.load_everything(self.layer_names.len(), self.edits);
                }
                self.refresh_tab();
            }
            BgMsg::ConnectError(e) => {
                self.conn = Conn::Disconnected;
                self.status = format!("Error: {e}");
            }
            BgMsg::ConnectHint(text) => {
                self.conn = Conn::Disconnected;
                self.status = text;
            }
            BgMsg::Keymap(layer, km, edit) => {
                if edit.is_some_and(|e| e != self.edits) {
                    // An edit went out after this read began: read it again.
                    self.be.load_layer(layer, self.edits);
                } else {
                    self.doc.set_layer(layer, km);
                    if layer == self.layer {
                        self.status = "Keymap loaded".into();
                    }
                }
            }
            BgMsg::KeymapError(e) => self.status = format!("Error: {e}"),
            BgMsg::KeyWriteFailed(pos, code, e) => {
                self.doc.mark_unsent(pos, code);
                self.status = format!("Error: key R{}C{} not saved: {e}", pos.1, pos.2);
            }
            BgMsg::LayerNames(names) => self.layer_names = names,
            BgMsg::Disconnected => {
                self.conn = Conn::Disconnected;
                self.firmware.clear();
                self.status = "Disconnected".into();
                self.mode_label.clear();
                self.reachable = true;
                self.is_dongle = false;
                self.wpm = 0;
                self.tools.matrix_on = false;
                self.device_board = None;
                self.macros.clear();
                self.macros_loaded = false;
                self.adv.tap_dances = vec![[0; 4]; parsers::TD_SLOTS];
                self.tap_dances_loaded = false;
                // Its map and shape are gone with it: back to the chosen model.
                if let Some(b) = self.boards.get(self.board) {
                    self.keys = b.keys.clone();
                    self.layer_names = b.layer_names.clone();
                }
                self.layer = 0;
                self.selected = None;
                self.load_factory_keymap();
            }
            BgMsg::LayoutPreview(json) => self.layout_ed.load_json(&json),
            BgMsg::LayoutStatus(s) => self.layout_ed.status = s,
            BgMsg::LayoutJson(keys) => {
                self.status = format!("Layout loaded ({} keys)", keys.len());
                self.selected = None;
                self.keys = keys;
                self.layout_from_device = true;
                if !self.layout_ed.dirty {
                    self.layout_ed.set_keys(self.keys.clone());
                }
            }
            BgMsg::BigramLines(lines) => {
                let entries = stats::parse_bigram_lines(&lines);
                let (rows, cols) = self.matrix_size();
                self.stats.bigrams = Some(stats::analyze_bigrams(&entries, rows, cols));
            }
            BgMsg::FlashProgress(p, m) => {
                self.flash.progress = p;
                self.flash.status = m;
            }
            BgMsg::FlashDone(r) => {
                self.flash.busy = false;
                match r {
                    Ok(()) => {
                        self.flash.progress = 1.0;
                        self.flash.status = "Flash complete!".into();
                        self.status = "Flash complete!".into();
                    }
                    Err(e) => {
                        self.flash.status = format!("Error: {e}");
                        self.status = format!("Flash error: {e}");
                    }
                }
            }
            BgMsg::HeatmapData(data, max) => self.apply_heat(data, max),
            BgMsg::TdList(list) => {
                self.adv.tap_dances = list;
                self.tap_dances_loaded = true;
            }
            BgMsg::ComboList(list) => self.adv.combos = list,
            BgMsg::LeaderList(list) => self.adv.leaders = list,
            BgMsg::KoList(list) => self.adv.key_overrides = list,
            BgMsg::BtStatus(lines) => self.adv.bt_status = lines.join("\n"),
            BgMsg::TamaStatus(lines) => self.adv.tama_status = lines.join("\n"),
            BgMsg::TamaStats(t) => self.stats.tama = Some(t),
            BgMsg::AutoshiftStatus(s) => self.adv.autoshift = s,
            BgMsg::Wpm(w) => self.wpm = w,
            BgMsg::OtaProgress(p, m) => {
                self.ota.progress = p;
                self.ota.status = m;
            }
            BgMsg::OtaDone(r) => {
                self.ota.busy = false;
                match r {
                    Ok(()) => {
                        self.ota.progress = 1.0;
                        self.ota.status = "OTA complete!".into();
                    }
                    Err(e) => self.ota.status = format!("OTA error: {e}"),
                }
            }
            BgMsg::ConfigProgress(p, m) => {
                self.config.progress = p;
                self.config.status = m;
            }
            BgMsg::ConfigDone(r) => {
                self.config.busy = false;
                match r {
                    Ok(m) => {
                        self.config.progress = 1.0;
                        self.config.status = m;
                        // An import may have changed any layer.
                        if self.online() {
                            self.be.load_everything(self.layer_names.len(), self.edits);
                        }
                    }
                    Err(e) => {
                        self.config.progress = 0.0;
                        self.config.status = format!("Error: {e}");
                    }
                }
            }
            BgMsg::MatrixToggled(on) => {
                self.tools.matrix_on = on;
                if on {
                    self.tools.matrix_colors.clear();
                    self.tools.matrix_log.clear();
                    self.tools.matrix_status = "Matrix test active — press keys".into();
                } else {
                    self.tools.matrix_status = "Matrix test stopped".into();
                }
            }
            BgMsg::MatrixEvent(row, col, state) => {
                let color = if state == 1 { theme::GREEN } else { theme::PURPLE };
                self.tools.matrix_colors.insert((row as usize, col as usize), color);
                let action = if state == 1 { "pressed" } else { "released" };
                self.tools.matrix_log.push_front(format!("R{row}C{col} {action}"));
                self.tools.matrix_log.truncate(7);
            }
            BgMsg::MatrixError(e) => {
                self.tools.matrix_on = false;
                self.tools.matrix_status = format!("Error: {e}");
            }
            BgMsg::NvsResetDone(r) => {
                self.tools.nvs_status = match r {
                    Ok(0xFF) => "Factory reset done — keyboard rebooting".into(),
                    Ok(mask) => format!("Reset (0x{mask:02X}) done — keyboard rebooting"),
                    Err(e) => format!("Error: {e}"),
                };
            }
            BgMsg::MacroList(list) => {
                self.mac.slot = crate::keys::free_slot(list.iter().map(|m| m.slot)) as i32;
                self.macros = list;
                self.macros_loaded = true;
            }
            BgMsg::Profile(p) => {
                self.device_name = p.name.clone();
                self.device_cols = p.cols;
                self.layout_ed.cols = p.cols;
                self.device_board = boards::index_for_device(&self.boards, &p.name);
                if let Some(i) = self.device_board {
                    self.board = i;
                    // Silent about its shape: draw what the firmware says this model has.
                    if !self.layout_from_device {
                        self.keys = self.boards[i].keys.clone();
                    }
                }
            }
            BgMsg::RfMode(d) => self.is_dongle = d,
            BgMsg::ConnectionMode(kind) => {
                self.mode_label = match kind {
                    ConnectionKind::Direct => "USB direct".to_string(),
                    ConnectionKind::Dongle { paired_count: 0 } => "via dongle".to_string(),
                    ConnectionKind::Dongle { paired_count: n } => format!("via dongle ({n} clavier)"),
                };
            }
            BgMsg::KeyboardReachable(reachable) => {
                self.reachable = reachable;
                if !reachable {
                    self.mode_label = "via dongle (aucun clavier joignable)".to_string();
                    if self.tab.needs_keyboard() {
                        self.tab = Tab::Device;
                        self.device_page = DevicePage::Status;
                    }
                }
            }
            BgMsg::RfStatus(s) => self.dongle.rf = s,
            BgMsg::RfPairList(l) => {
                let fmt = |m: &[u8; 6], present: bool| {
                    if present {
                        m.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
                    } else {
                        "—".to_string()
                    }
                };
                self.dongle.paired = l.paired_count;
                self.dongle.mac_left = fmt(&l.mac_left, l.left_paired());
                self.dongle.mac_right = fmt(&l.mac_right, l.right_paired());
            }
            BgMsg::Battery(b) => self.dongle.battery = Some(b),
            BgMsg::PairTick(s) => self.dongle.pair_countdown = Some(s),
            BgMsg::PairResult(r) => {
                self.dongle.pair_countdown = None;
                self.dongle.pair_status = match r {
                    Ok(n) => format!("Paired ✓ (total: {n})"),
                    Err(e) => e,
                };
            }
            BgMsg::PickedFile(target, path) => self.file_picked(target, path),
            BgMsg::PickedText(target, name, text) => self.text_picked(target, name, text),
            BgMsg::Status(s) => self.status = s,
            BgMsg::StatusError(e) => self.status = format!("Error: {e}"),
        }
    }

    fn file_picked(&mut self, target: PickTarget, path: String) {
        match target {
            PickTarget::Ota => self.ota.path = path,
            PickTarget::Firmware => self.flash.path = path,
            PickTarget::ConfigExport => {
                self.config.busy = true;
                self.config.progress = 0.0;
                self.config.status = "Reading config...".into();
                self.be.export_config(self.config_bits(), path);
            }
            PickTarget::ConfigImport | PickTarget::LayoutLoad => match std::fs::read_to_string(&path) {
                Ok(text) => self.text_picked(target, path, text),
                Err(e) if target == PickTarget::LayoutLoad => self.layout_ed.status = format!("Read error: {e}"),
                Err(e) => self.config.status = format!("Error: Read error: {e}"),
            },
            PickTarget::LayoutExport => {
                let json = self.layout_ed.json().to_string();
                self.layout_ed.status = match self.be.save_text(&path, &json) {
                    Ok(()) => {
                        self.layout_ed.dirty = false;
                        format!("Exported to {path}")
                    }
                    Err(e) => format!("Write error: {e}"),
                };
            }
        }
    }

    /// The content of a file the person chose.
    fn text_picked(&mut self, target: PickTarget, name: String, text: String) {
        match target {
            PickTarget::ConfigImport => match config_io::KeyboardConfig::from_json(&text) {
                Ok(config) => {
                    self.confirm = Some(Confirm {
                        title: "Write this file to the keyboard?".into(),
                        body: format!(
                            "{name} replaces what is on the keyboard for the parts checked. \
                             Export a backup first if you may want the current setup back."
                        ),
                        ok: "Write it".into(),
                        action: Action::ImportConfig(Box::new(config)),
                    });
                }
                Err(e) => self.config.status = format!("Error: Parse error: {e}"),
            },
            PickTarget::LayoutLoad => {
                self.layout_ed.file_path = name;
                self.layout_ed.load_json(&text);
            }
            _ => {}
        }
    }

    pub fn config_bits(&self) -> u8 {
        (self.config_mask[0] as u8) | ((self.config_mask[1] as u8) << 1) | ((self.config_mask[2] as u8) << 3)
    }

    fn run_timers(&mut self) {
        let now = Instant::now();
        if self.online() && !self.tools.matrix_on && now >= self.next_wpm {
            self.next_wpm = now + WPM_PERIOD;
            self.be.poll_wpm();
        }
        if now >= self.next_layout {
            self.next_layout = now + LAYOUT_PERIOD;
            if self.layout_ed.auto_refresh && !self.layout_ed.file_path.is_empty() {
                if let Ok(json) = std::fs::read_to_string(&self.layout_ed.file_path) {
                    self.layout_ed.load_json(&json);
                }
            }
        }
        if self.watch_ports && self.conn == Conn::Disconnected && now >= self.next_port_scan {
            self.next_port_scan = now + PORT_SCAN_PERIOD;
            let ports = backend::list_ports();
            let names: Vec<String> = ports.iter().map(|p| p.0.clone()).collect();
            let arrived = names.iter().any(|n| !self.seen_ports.contains(n));
            if names != self.seen_ports {
                self.ports = ports;
                self.port_idx = 0;
            }
            self.seen_ports = names;
            if arrived {
                self.status = "Keyboard plugged in, connecting...".into();
                self.conn = Conn::Connecting;
                self.be.connect(None);
            }
        }
        if self.online() && self.is_dongle && now >= self.next_dongle {
            self.next_dongle = now + DONGLE_PERIOD;
            let slow = self.dongle_tick.is_multiple_of(5);
            self.dongle_tick = self.dongle_tick.wrapping_add(1);
            self.be.poll_dongle(slow);
        }
    }

    pub fn run_action(&mut self, action: Action) {
        match action {
            Action::NvsReset(mask) => self.be.nvs_reset(mask),
            Action::ResetPairing => self.be.dongle_reset_pairing(),
            Action::SendPending => self.send_pending(),
            Action::RestoreFactoryLayer => self.restore_factory_layer(),
            Action::ImportConfig(config) => {
                self.config.busy = true;
                self.config.progress = 0.0;
                self.config.status = "Importing config...".into();
                self.be.import_config(*config, self.config_bits());
            }
            Action::ResetPractice => {
                if let Some(s) = &mut self.practice {
                    s.reset_progress();
                    crate::ui_practice::store(s);
                }
            }
            Action::Ota(path) => match std::fs::read(&path) {
                Ok(firmware) => {
                    self.ota.busy = true;
                    self.ota.progress = 0.0;
                    self.ota.status = "Starting OTA...".into();
                    self.be.ota(firmware);
                }
                Err(e) => self.ota.status = format!("OTA error: Cannot read {path}: {e}"),
            },
            Action::Flash { port, path, offset } => match std::fs::read(&path) {
                Ok(firmware) => {
                    self.flash.busy = true;
                    self.flash.progress = 0.0;
                    self.flash.status = "Starting...".into();
                    self.be.flash(port, firmware, offset);
                }
                Err(e) => self.flash.status = format!("Error: Cannot read {path}: {e}"),
            },
        }
    }

    // ------------------------------------------------------------------- UI

    fn connection_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            let led = if self.online() { theme::GREEN } else { theme::RED };
            ui.painter().circle_filled(rect.center(), 8.0, led);

            let current = self
                .ports
                .get(self.port_idx)
                .map(|p| p.1.clone())
                .unwrap_or_else(|| backend::NO_PORT_LABEL.to_string());
            egui::ComboBox::from_id_salt("port")
                .width(240.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, (_, label, _)) in self.ports.iter().enumerate() {
                        ui.selectable_value(&mut self.port_idx, i, label);
                    }
                });
            self.model_picker(ui);
            if ui.add_enabled(!self.online(), egui::Button::new("⟳")).clicked() {
                self.ports = backend::list_ports();
                if self.port_idx >= self.ports.len() {
                    self.port_idx = 0;
                }
            }
            let label = if self.online() { "Disconnect" } else { "Connect" };
            let enabled = self.conn != Conn::Connecting;
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                if self.online() {
                    self.be.disconnect();
                } else {
                    self.status = "Connecting...".into();
                    self.conn = Conn::Connecting;
                    self.layout_from_device = false;
                    let port = self.ports.get(self.port_idx).map(|p| p.0.clone());
                    self.be.connect_chosen(port);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // A firmware name can be long: it gets cut here, Device > Status has it in full.
                let text = format!("{}   {}", self.mode_label, self.firmware);
                ui.add(egui::Label::new(RichText::new(text.trim()).color(theme::FG_MUTED)).truncate());
            });
        });
    }

    /// Which keyboard to draw when none is connected. Connected, it says what the
    /// keyboard reports and can't be changed.
    fn model_picker(&mut self, ui: &mut egui::Ui) {
        let name = self.boards.get(self.board).map(|b| b.name.clone()).unwrap_or_default();
        let current = self.board;
        let mut picked = None;
        ui.add_enabled_ui(self.conn == Conn::Disconnected, |ui| {
            egui::ComboBox::from_id_salt("model").width(120.0).selected_text(name).show_ui(ui, |ui| {
                for (i, b) in self.boards.iter().enumerate() {
                    if ui.selectable_label(i == current, &b.name).clicked() {
                        picked = Some(i);
                    }
                }
            });
        });
        if let Some(i) = picked {
            self.choose_board(i);
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let w = ui.available_width();
        let gap = 2.0;
        let tab_w = (w - gap * (Tab::ALL.len() as f32 - 1.0)) / Tab::ALL.len() as f32;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for tab in Tab::ALL {
                let enabled = self.reachable || !tab.needs_keyboard();
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(tab_w, 32.0),
                    if enabled { egui::Sense::click() } else { egui::Sense::hover() },
                );
                let active = self.tab == tab;
                let p = ui.painter();
                if active {
                    p.rect_filled(rect, 4.0, theme::BG_SECONDARY);
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - 2.0),
                        rect.right_bottom(),
                    );
                    p.rect_filled(bar, 0.0, theme::PURPLE);
                }
                let color = if active { theme::FG_PRIMARY } else { theme::FG_MUTED };
                let color = if enabled { color } else { color.gamma_multiply(0.4) };
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tab.title(),
                    egui::FontId::proportional(13.0),
                    color,
                );
                if resp.clicked() {
                    self.set_tab(tab);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(&self.status).color(theme::FG_MUTED).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .color(theme::FG_MUTED)
                        .size(11.0),
                );
                if self.wpm > 0 {
                    ui.label(RichText::new(format!("WPM: {}", self.wpm)).color(theme::CYAN).size(12.0));
                }
            });
        });
    }

    fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.rename.take() else { return };
        let mut keep = true;
        let modal = egui::Modal::new(egui::Id::unique("rename")).show(ctx, |ui| {
            ui.set_width(228.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Rename layer").size(13.0));
                let edit = ui.add(egui::TextEdit::singleline(&mut name).hint_text("New name"));
                edit.request_focus();
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(30.0);
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    if ui.button("Rename").clicked() || enter {
                        keep = false;
                        let layer = self.layer;
                        if self.online() {
                            self.be.rename_layer(layer, name.clone());
                        } else if let Some(n) = self.layer_names.get_mut(layer) {
                            *n = name.clone();
                        }
                        self.status = format!("Renamed layer {layer} → {name}");
                    }
                });
            });
        });
        if modal.should_close() {
            keep = false;
        }
        if keep {
            self.rename = Some(name);
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = self.confirm.take() else { return };
        let mut keep = true;
        let mut go = false;
        let modal = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&c.title).color(theme::ORANGE).size(14.0));
            ui.add_space(4.0);
            ui.label(RichText::new(&c.body).color(theme::FG_MUTED));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                let ok = egui::Button::new(RichText::new(&c.ok).color(theme::BG_PRIMARY)).fill(theme::RED);
                if ui.add(ok).clicked() {
                    go = true;
                }
            });
        });
        if go {
            self.run_action(c.action);
        } else if keep && !modal.should_close() {
            self.confirm = Some(c);
        }
    }

    fn sec_dialog(&mut self, ctx: &egui::Context) {
        if !self.sec_warning {
            return;
        }
        let modal = egui::Modal::new(egui::Id::unique("sec")).show(ctx, |ui| {
            ui.set_width(304.0);
            let mut close = false;
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Sec Confirm sur la moitié droite").color(theme::ORANGE).size(14.0),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "La touche est assignée, mais le firmware ignorera l'appui : une \
                         confirmation venue de la moitié droite passe par une radio non \
                         authentifiée. Place-la sur la moitié gauche pour qu'elle agisse.",
                    )
                    .color(theme::FG_MUTED),
                );
                ui.add_space(8.0);
                close = ui.button("Compris").clicked();
            });
            close
        });
        if modal.inner || modal.should_close() {
            self.sec_warning = false;
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn take_screenshot_when_ready(&mut self, _ctx: &egui::Context) {}

    #[cfg(not(target_arch = "wasm32"))]
    fn take_screenshot_when_ready(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        // Connecting on purpose: the picture is of what the keyboard answered.
        if self.conn == Conn::Connecting {
            return;
        }
        self.frames += 1;
        if self.frames == SCREENSHOT_FRAME {
            let state = self.shot_state.clone();
            let cb = egui::ScreenshotCallback::new(move |img| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                let saved = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8);
                if let Err(e) = &saved {
                    eprintln!("screenshot: {e}");
                }
                state.store(if saved.is_ok() { 1 } else { 2 }, Ordering::SeqCst);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        if self.shot_state.load(Ordering::SeqCst) != 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// One whole frame of the window; `ui` is the root of the viewport.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            theme::apply(&ctx);
            self.be.set_ctx(ctx.clone());
            self.themed = true;
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_msg(msg);
        }
        self.run_timers();
        if self.online() || self.layout_ed.auto_refresh {
            ctx.request_repaint_after(Duration::from_millis(500));
        } else if self.watch_ports {
            ctx.request_repaint_after(PORT_SCAN_PERIOD);
        }

        let bar = Frame::new().fill(theme::BG_SECONDARY);
        egui::Panel::top("connection")
            .exact_size(48.0)
            .frame(bar.inner_margin(8))
            .show_separator_line(false)
            .show(ui, |ui| self.connection_bar(ui));
        egui::Panel::top("tabs")
            .exact_size(34.0)
            .frame(Frame::new().fill(theme::BG_PRIMARY).inner_margin(egui::Margin::symmetric(8, 1)))
            .show_separator_line(false)
            .show(ui, |ui| self.tab_bar(ui));
        egui::Panel::bottom("status")
            .exact_size(32.0)
            .frame(bar.inner_margin(egui::Margin::symmetric(12, 0)))
            .show_separator_line(false)
            .show(ui, |ui| self.status_bar(ui));
        let body = if self.tab == Tab::Keymap { theme::BG_SECONDARY } else { theme::BG_PRIMARY };
        egui::CentralPanel::default()
            .frame(Frame::new().fill(body))
            .show(ui, |ui| match self.tab {
                Tab::Keymap => crate::ui_keymap::show(self, ui),
                Tab::Layout => crate::ui_layout::show(self, ui),
                Tab::Behaviors => crate::ui_behaviors::show(self, ui),
                Tab::Typing => crate::ui_typing::show(self, ui),
                Tab::Practice => crate::ui_practice::show(self, ui),
                Tab::Device => crate::ui_device::show(self, ui),
            });

        crate::ui_selector::show(self, &ctx);
        self.rename_dialog(&ctx);
        self.confirm_dialog(&ctx);
        self.sec_dialog(&ctx);
        self.take_screenshot_when_ready(&ctx);
    }

    pub fn any_dialog_open(&self) -> bool {
        self.selector.is_some() || self.rename.is_some() || self.confirm.is_some() || self.sec_warning
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}
