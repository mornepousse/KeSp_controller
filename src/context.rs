use crate::protocol::layout::KeycapPos;
use crate::protocol::layout_remap::KeyboardLayout;
use crate::protocol::parsers;
use crate::protocol::serial::SerialManager;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Shared application state passed to all bridge setup functions.
///
/// Thread-safe fields (`serial`, `bg_tx`) are cloned into background threads.
/// Main-thread fields (`Rc<RefCell<…>>`) stay on the UI thread only.
pub struct AppContext {
    pub serial: Arc<Mutex<SerialManager>>,
    pub bg_tx: mpsc::Sender<BgMsg>,
    pub keys: Rc<RefCell<Vec<KeycapPos>>>,
    pub current_keymap: Rc<RefCell<Vec<Vec<u16>>>>,
    pub current_layer: Rc<Cell<usize>>,
    pub keyboard_layout: Rc<RefCell<KeyboardLayout>>,
    pub heatmap_data: Rc<RefCell<Vec<Vec<u32>>>>,
    pub macro_steps: Rc<RefCell<Vec<(u8, u8)>>>,
}

/// Messages sent from background serial threads to the UI event loop.
pub enum BgMsg {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>), // port, fw_version, layer_names, keymap
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    Disconnected,
    #[allow(dead_code)]
    TextLines(String, Vec<String>),
    HeatmapData(Vec<Vec<u32>>, u32), // counts, max
    BigramLines(Vec<String>),
    LayoutJson(Vec<KeycapPos>),
    MacroList(Vec<parsers::MacroEntry>),
    TdList(Vec<[u16; 4]>),
    ComboList(Vec<parsers::ComboEntry>),
    LeaderList(Vec<parsers::LeaderEntry>),
    KoList(Vec<[u8; 4]>),
    BtStatus(Vec<String>),
    TamaStatus(Vec<String>),
    AutoshiftStatus(String),
    Wpm(u16),
    FlashProgress(f32, String),
    FlashDone(Result<(), String>),
    OtaProgress(f32, String),
    OtaDone(Result<(), String>),
    ConfigProgress(f32, String),
    ConfigDone(Result<String, String>),
    MatrixTestToggled(bool, u8, u8), // enabled, rows, cols
    MatrixTestEvent(u8, u8, u8),     // row, col, state (1=pressed, 0=released)
    MatrixTestError(String),
    NvsResetDone(Result<u8, String>), // Ok(mask) or Err
}

/// Spawn a background thread that locks the serial port and runs `f`.
#[allow(dead_code)]
///
/// Eliminates the 4-line clone+spawn+lock boilerplate repeated 30+ times.
pub fn serial_spawn<F>(ctx: &AppContext, f: F)
where
    F: FnOnce(&mut SerialManager, &mpsc::Sender<BgMsg>) + Send + 'static,
{
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    std::thread::spawn(move || {
        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut ser, &tx);
    });
}

/// Map ComboBox modifier index to HID modifier byte.
/// [None, Ctrl, Shift, Alt, GUI, RCtrl, RShift, RAlt, RGUI]
pub fn mod_idx_to_byte(idx: i32) -> u8 {
    match idx {
        1 => 0x01, 2 => 0x02, 3 => 0x04, 4 => 0x08,
        5 => 0x10, 6 => 0x20, 7 => 0x40, 8 => 0x80,
        _ => 0x00,
    }
}
