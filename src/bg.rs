//! What the keyboard's two ways of being reached share: desktop threads and the
//! browser's queue both answer the window with these messages.

use crate::protocol::connection::ConnectionKind;
use crate::protocol::layout::KeycapPos;
use crate::protocol::{device, parsers};
use std::sync::{mpsc, Arc, OnceLock};

pub const PAIR_WINDOW_S: u8 = 30;

pub struct TamaStats {
    pub enabled: bool,
    pub level: u16,
    pub hunger: u16,
    pub happiness: u16,
    pub energy: u16,
    pub health: u16,
    pub xp: u16,
    pub total_keys: u32,
    pub max_kpm: u32,
}

pub enum BgMsg {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>),
    ConnectError(String),
    /// Not connected, and that's not a failure: says what to do.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    ConnectHint(String),
    /// A layer as read. The number says which edit it follows, when it matters:
    /// a read that an edit overtook must not overwrite that edit.
    Keymap(usize, Vec<Vec<u16>>, Option<u64>),
    KeymapError(String),
    KeyWriteFailed((usize, usize, usize), u16, String),
    LayerNames(Vec<String>),
    Disconnected,
    HeatmapData(Vec<Vec<u32>>, u32),
    BigramLines(Vec<String>),
    LayoutJson(Vec<KeycapPos>),
    LayoutPreview(String),
    LayoutStatus(String),
    MacroList(Vec<parsers::MacroEntry>),
    TdList(Vec<[u16; 4]>),
    ComboList(Vec<parsers::ComboEntry>),
    LeaderList(Vec<parsers::LeaderEntry>),
    KoList(Vec<parsers::KoEntry>),
    BtStatus(Vec<String>),
    TamaStatus(Vec<String>),
    TamaStats(TamaStats),
    AutoshiftStatus(String),
    Wpm(u16),
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    FlashProgress(f32, String),
    FlashDone(Result<(), String>),
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OtaProgress(f32, String),
    OtaDone(Result<(), String>),
    ConfigProgress(f32, String),
    ConfigDone(Result<String, String>),
    MatrixToggled(bool),
    MatrixEvent(u8, u8, u8),
    MatrixError(String),
    NvsResetDone(Result<u8, String>),
    RfMode(bool),
    ConnectionMode(ConnectionKind),
    KeyboardReachable(bool),
    Profile(device::DeviceProfile),
    RfStatus(parsers::RfStatus),
    RfPairList(parsers::PairList),
    Battery(parsers::Battery),
    PairTick(u8),
    PairResult(Result<u8, String>),
    PickedFile(PickTarget, String),
    /// A file's content, where files have no path to read them from (browser).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    PickedText(PickTarget, String, String),
    Status(String),
    StatusError(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickTarget {
    Ota,
    Firmware,
    ConfigExport,
    ConfigImport,
    LayoutLoad,
    LayoutExport,
}

/// Sender that also wakes the window, so replies show without polling the UI.
#[derive(Clone)]
pub struct Tx {
    tx: mpsc::Sender<BgMsg>,
    ctx: Arc<OnceLock<egui::Context>>,
}

impl Tx {
    pub fn channel() -> (Self, mpsc::Receiver<BgMsg>) {
        let (tx, rx) = mpsc::channel();
        (Self { tx, ctx: Arc::new(OnceLock::new()) }, rx)
    }

    pub fn set_ctx(&self, ctx: egui::Context) {
        let _ = self.ctx.set(ctx);
    }

    pub fn send(&self, msg: BgMsg) {
        let _ = self.tx.send(msg);
        if let Some(c) = self.ctx.get() {
            c.request_repaint();
        }
    }
}


/// What to read back after a command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refresh {
    Nothing,
    Macros,
    Combos,
    Leaders,
    KeyOverrides,
}

/// The firmware removed the pet but still answers its query, with zeros: "TAMA
/// removed — commands kept for protocol compat". All zeros means nothing to show.
pub fn tama_present(payload: &[u8]) -> bool {
    payload.iter().any(|&b| b != 0)
}

pub fn parse_tama_stats(p: &[u8]) -> TamaStats {
    let u16_at = |i: usize| u16::from_le_bytes([p[i], p[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
    TamaStats {
        enabled: p[0] != 0,
        hunger: u16_at(2),
        happiness: u16_at(4),
        energy: u16_at(6),
        health: u16_at(8),
        level: u16_at(10),
        xp: u16_at(12),
        total_keys: u32_at(14),
        max_kpm: u32_at(18),
    }
}

/// Tie protocol to the UI's `is_right_half` check without exposing the module path.
pub fn sec_confirm_misplaced(keys: &[KeycapPos], key_idx: usize, device_name: &str) -> bool {
    crate::protocol::stats::key_is_on_right_half(keys, key_idx, device_name)
}
