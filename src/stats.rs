use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::{MainWindow, StatsBridge};
use slint::ComponentHandle;

/// Wire up the stats refresh callback.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();

    window.global::<StatsBridge>().on_refresh_stats(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            use protocol::binary::cmd;
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
                let (data, max) = protocol::parsers::parse_keystats_binary(&r.payload);
                let _ = tx.send(BgMsg::HeatmapData(data, max));
            }
            let bigram_lines = if let Ok(r) = ser.send_binary(cmd::BIGRAMS_TEXT, &[]) {
                String::from_utf8_lossy(&r.payload).lines().map(|l| l.to_string()).collect()
            } else { Vec::new() };
            let _ = tx.send(BgMsg::BigramLines(bigram_lines));
            // Tama stats
            if let Ok(r) = ser.send_binary(cmd::TAMA_QUERY, &[]) {
                if r.payload.len() >= 22 {
                    let _ = tx.send(BgMsg::TamaStats(parse_tama_stats(&r.payload)));
                }
            }
        });
    });
}

/// Structured tama stats for UI display.
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

pub fn parse_tama_stats(payload: &[u8]) -> TamaStats {
    TamaStats {
        enabled: payload[0] != 0,
        level: u16::from_le_bytes([payload[10], payload[11]]),
        hunger: u16::from_le_bytes([payload[2], payload[3]]),
        happiness: u16::from_le_bytes([payload[4], payload[5]]),
        energy: u16::from_le_bytes([payload[6], payload[7]]),
        health: u16::from_le_bytes([payload[8], payload[9]]),
        xp: u16::from_le_bytes([payload[12], payload[13]]),
        total_keys: u32::from_le_bytes([payload[14], payload[15], payload[16], payload[17]]),
        max_kpm: u32::from_le_bytes([payload[18], payload[19], payload[20], payload[21]]),
    }
}
