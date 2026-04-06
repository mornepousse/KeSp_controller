mod background;
mod connection;
mod geometry;
mod helpers;
mod key_selector;
mod render;
mod tab_advanced;
mod tab_keymap;
mod tab_macros;
mod tab_settings;
mod tab_stats;
mod update;

use std::sync::mpsc;
use crate::serial::{self, SharedSerial};
use crate::layout::{self, KeycapPos};
use crate::layout_remap::KeyboardLayout;

// Instant doesn't exist in WASM - use a wrapper
#[cfg(not(target_arch = "wasm32"))]
type Instant = std::time::Instant;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct Instant(f64);

#[cfg(target_arch = "wasm32")]
impl Instant {
    fn now() -> Self {
        let window = web_sys::window().unwrap();
        let performance = window.performance().unwrap();
        Instant(performance.now())
    }
    fn elapsed(&self) -> std::time::Duration {
        let now = Self::now();
        let ms = now.0 - self.0;
        std::time::Duration::from_millis(ms as u64)
    }
}

/// Results from background serial operations.
pub(super) enum BgResult {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>),
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    TextLines(String, Vec<String>),
    #[allow(dead_code)] // constructed only in WASM builds
    BinaryPayload(String, Vec<u8>), // tag, raw KR payload
    LayoutJson(Vec<crate::layout::KeycapPos>), // physical key positions from firmware
    OtaProgress(f32, String),       // progress 0-1, status message
    HeatmapData(Vec<Vec<u32>>, u32), // counts, max
    Error(String),
}

#[derive(PartialEq)]
pub(super) enum Tab {
    Keymap,
    Advanced,
    Macros,
    Stats,
    Settings,
}

pub struct KaSeApp {
    pub(super) serial: SharedSerial,
    pub(super) bg_tx: mpsc::Sender<BgResult>,
    pub(super) bg_rx: mpsc::Receiver<BgResult>,
    pub(super) busy: bool,
    #[cfg(target_arch = "wasm32")]
    pub(super) web_busy: std::rc::Rc<std::cell::Cell<bool>>,
    pub(super) tab: Tab,

    // Connection
    /// Cached connection status — avoids serial.lock() in views.
    /// Updated in poll_bg() when Connected/ConnectError is received.
    pub(super) connected_cache: bool,
    pub(super) firmware_version: String,

    // Keymap
    pub(super) current_layer: usize,
    pub(super) layer_names: Vec<String>,
    pub(super) keymap: Vec<Vec<u16>>, // rows x cols
    pub(super) key_layout: Vec<KeycapPos>,
    pub(super) editing_key: Option<(usize, usize)>, // (row, col) being edited

    // Keymap editing
    pub(super) layer_rename: String,

    // Advanced
    pub(super) td_lines: Vec<String>,
    pub(super) td_data: Vec<[u16; 4]>,  // parsed tap dance slots
    pub(super) combo_lines: Vec<String>,
    pub(super) combo_data: Vec<crate::parsers::ComboEntry>,
    /// None = pas de picking en cours. Some((combo_idx, slot)) = on attend un clic clavier.
    /// combo_idx: usize::MAX = nouveau combo. slot: 0 = key1, 1 = key2.
    pub(super) combo_picking: Option<(usize, u8)>,
    pub(super) combo_new_r1: u8,
    pub(super) combo_new_c1: u8,
    pub(super) combo_new_r2: u8,
    pub(super) combo_new_c2: u8,
    pub(super) combo_new_result: u16,
    pub(super) combo_new_key1_set: bool,
    pub(super) combo_new_key2_set: bool,
    pub(super) leader_lines: Vec<String>,
    pub(super) leader_data: Vec<crate::parsers::LeaderEntry>,
    // Leader editing: new sequence being built
    pub(super) leader_new_seq: Vec<u8>,     // HID keycodes for the sequence
    pub(super) leader_new_result: u8,
    pub(super) leader_new_mod: u8,
    pub(super) leader_new_result_set: bool,
    pub(super) ko_lines: Vec<String>,
    pub(super) ko_data: Vec<[u8; 4]>,   // parsed: [trigger, trig_mod, result, res_mod]
    pub(super) ko_new_trig_key: u8,
    pub(super) ko_new_trig_mod: u8,
    pub(super) ko_new_res_key: u8,
    pub(super) ko_new_res_mod: u8,
    pub(super) ko_new_trig_set: bool,
    pub(super) ko_new_res_set: bool,
    pub(super) bt_lines: Vec<String>,
    pub(super) tama_lines: Vec<String>,
    pub(super) wpm_text: String,
    pub(super) autoshift_status: String,
    pub(super) tri_l1: String,
    pub(super) tri_l2: String,
    pub(super) tri_l3: String,

    // Macros
    pub(super) macro_lines: Vec<String>,
    pub(super) macro_data: Vec<crate::parsers::MacroEntry>,
    pub(super) macro_slot: String,
    pub(super) macro_name: String,
    pub(super) macro_steps: String,

    // Stats / Heatmap
    pub(super) keystats_lines: Vec<String>,
    pub(super) bigrams_lines: Vec<String>,
    pub(super) heatmap_data: Vec<Vec<u32>>,  // rows x cols press counts
    pub(super) heatmap_max: u32,
    pub(super) heatmap_on: bool,
    pub(super) heatmap_selected: Option<(usize, usize)>, // selected key for bigram view
    pub(super) stats_dirty: bool,

    // Key selector
    pub(super) key_search: String,
    pub(super) mt_mod: u8,
    pub(super) mt_key: u8,
    pub(super) lt_layer: u8,
    pub(super) lt_key: u8,
    pub(super) hex_input: String,

    // Settings
    pub(super) keyboard_layout: crate::layout_remap::KeyboardLayout,

    // OTA
    pub(super) ota_path: String,
    pub(super) ota_status: String,
    pub(super) ota_firmware_data: Vec<u8>,
    pub(super) ota_progress: f32,
    pub(super) ota_releases: Vec<(String, String)>, // (tag_name, asset_url)
    pub(super) ota_selected_release: usize,

    // Prog port flasher (native only)
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) prog_port: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) prog_path: String,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) prog_ports_list: Vec<String>,

    // Notifications
    pub(super) notifications: Vec<(String, Instant)>,

    // Status
    pub(super) status_msg: String,
    pub(super) last_reconnect_poll: Instant,
    pub(super) last_port_check: Instant,
    pub(super) last_wpm_poll: Instant,
    pub(super) last_stats_refresh: Instant,
}

impl KaSeApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();

        let saved_settings = crate::settings::load();
        let keyboard_layout = KeyboardLayout::from_name(&saved_settings.keyboard_layout);

        Self {
            serial: serial::new_shared(),
            bg_tx,
            bg_rx,
            busy: false,
            connected_cache: false,
            #[cfg(target_arch = "wasm32")]
            web_busy: std::rc::Rc::new(std::cell::Cell::new(false)),
            tab: Tab::Keymap,
            firmware_version: String::new(),
            current_layer: 0,
            layer_names: vec!["Layer 0".into()],
            keymap: Vec::new(),
            key_layout: layout::default_layout(),
            editing_key: None,
            layer_rename: String::new(),
            td_lines: Vec::new(),
            td_data: Vec::new(),
            combo_lines: Vec::new(),
            combo_data: Vec::new(),
            combo_picking: None,
            combo_new_r1: 0,
            combo_new_c1: 0,
            combo_new_r2: 0,
            combo_new_c2: 0,
            combo_new_result: 0,
            combo_new_key1_set: false,
            combo_new_key2_set: false,
            leader_new_seq: Vec::new(),
            leader_new_result: 0,
            leader_new_mod: 0,
            leader_new_result_set: false,
            leader_lines: Vec::new(),
            leader_data: Vec::new(),
            ko_lines: Vec::new(),
            ko_data: Vec::new(),
            ko_new_trig_key: 0,
            ko_new_trig_mod: 0,
            ko_new_res_key: 0,
            ko_new_res_mod: 0,
            ko_new_trig_set: false,
            ko_new_res_set: false,
            bt_lines: Vec::new(),
            macro_lines: Vec::new(),
            macro_data: Vec::new(),
            tama_lines: Vec::new(),
            wpm_text: String::new(),
            autoshift_status: String::new(),
            tri_l1: "1".into(),
            tri_l2: "2".into(),
            tri_l3: "3".into(),
            macro_slot: "0".into(),
            macro_name: String::new(),
            macro_steps: String::new(),
            keystats_lines: Vec::new(),
            bigrams_lines: Vec::new(),
            heatmap_data: Vec::new(),
            heatmap_max: 0,
            heatmap_on: false,
            heatmap_selected: None,
            stats_dirty: true,
            key_search: String::new(),
            mt_mod: 0x02, // HID Left Shift
            mt_key: 0x04, // HID 'A'
            lt_layer: 1,
            lt_key: 0x2C, // HID Space
            hex_input: String::new(),
            keyboard_layout,
            #[cfg(not(target_arch = "wasm32"))]
            prog_port: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            prog_path: String::new(),
            #[cfg(not(target_arch = "wasm32"))]
            prog_ports_list: Vec::new(),
            notifications: Vec::new(),
            ota_path: String::new(),
            ota_status: String::new(),
            ota_firmware_data: Vec::new(),
            ota_progress: 0.0,
            ota_releases: Vec::new(),
            ota_selected_release: 0,
            status_msg: "Searching KeSp...".into(),
            last_reconnect_poll: Instant::now(),
            last_port_check: Instant::now(),
            last_wpm_poll: Instant::now(),
            last_stats_refresh: Instant::now(),
        }
    }
}
