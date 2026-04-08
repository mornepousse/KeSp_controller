use crate::context::AppContext;
use crate::{MainWindow, StatsBridge};
use crate::protocol;
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
            use crate::context::BgMsg;
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
                let (data, max) = protocol::parsers::parse_keystats_binary(&r.payload);
                let _ = tx.send(BgMsg::HeatmapData(data, max));
            }
            let bigram_lines = if let Ok(r) = ser.send_binary(protocol::binary::cmd::BIGRAMS_TEXT, &[]) {
                String::from_utf8_lossy(&r.payload).lines().map(|l| l.to_string()).collect()
            } else { Vec::new() };
            let _ = tx.send(BgMsg::BigramLines(bigram_lines));
        });
    });
}
