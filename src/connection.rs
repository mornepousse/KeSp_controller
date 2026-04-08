use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::{AppState, ConnectionBridge, ConnectionState, MainWindow};
use slint::ComponentHandle;

/// Auto-connect to the keyboard on startup.
pub fn auto_connect(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    window.global::<AppState>().set_status_text("Scanning ports...".into());
    window.global::<AppState>().set_connection(ConnectionState::Connecting);

    std::thread::spawn(move || {
        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
        match ser.auto_connect() {
            Ok(port_name) => {
                let fw = ser.get_firmware_version().unwrap_or_default();
                let names = ser.get_layer_names().unwrap_or_default();
                let km = ser.get_keymap(0).unwrap_or_default();
                let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
                // Fetch physical layout from firmware
                match ser.get_layout_json() {
                    Ok(json) => {
                        match protocol::layout::parse_json(&json) {
                            Ok(keys) => { let _ = tx.send(BgMsg::LayoutJson(keys)); }
                            Err(e) => eprintln!("Layout parse error: {}", e),
                        }
                    }
                    Err(e) => eprintln!("get_layout_json error: {}", e),
                }
            }
            Err(e) => {
                let _ = tx.send(BgMsg::ConnectError(e));
            }
        }
    });
}

/// Wire up Connect, Disconnect, refresh_ports, and tab-change auto-refresh.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    // --- Connect callback ---
    {
        let serial_c = ctx.serial.clone();
        let tx_c = ctx.bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_connect(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Scanning ports...".into());
                w.global::<AppState>().set_connection(ConnectionState::Connecting);
            }
            let serial = serial_c.clone();
            let tx = tx_c.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                match ser.auto_connect() {
                    Ok(port_name) => {
                        let fw = ser.get_firmware_version().unwrap_or_default();
                        let names = ser.get_layer_names().unwrap_or_default();
                        let km = ser.get_keymap(0).unwrap_or_default();
                        let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
                        match ser.get_layout_json() {
                            Ok(json) => {
                                match protocol::layout::parse_json(&json) {
                                    Ok(keys) => { let _ = tx.send(BgMsg::LayoutJson(keys)); }
                                    Err(e) => eprintln!("Layout parse error: {}", e),
                                }
                            }
                            Err(e) => eprintln!("get_layout_json error: {}", e),
                        }
                    }
                    Err(e) => { let _ = tx.send(BgMsg::ConnectError(e)); }
                }
            });
        });
    }

    // --- Disconnect callback ---
    {
        let serial_d = ctx.serial.clone();
        let tx_d = ctx.bg_tx.clone();
        window.global::<ConnectionBridge>().on_disconnect(move || {
            let mut ser = serial_d.lock().unwrap_or_else(|e| e.into_inner());
            ser.disconnect();
            let _ = tx_d.send(BgMsg::Disconnected);
        });
    }

    window.global::<ConnectionBridge>().on_refresh_ports(|| {});

    // --- Auto-refresh on tab change ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AppState>().on_tab_changed(move |tab_idx| {
            let Some(w) = window_weak.upgrade() else { return };
            if w.global::<AppState>().get_connection() != ConnectionState::Connected { return; }

            let serial = serial.clone();
            let tx = tx.clone();
            match tab_idx {
                1 => {
                    // Advanced: refresh TD, combo, leader, KO, BT via binary
                    std::thread::spawn(move || {
                        use protocol::binary::cmd;
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(r) = ser.send_binary(cmd::TD_LIST, &[]) {
                            let _ = tx.send(BgMsg::TdList(protocol::parsers::parse_td_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                            let _ = tx.send(BgMsg::ComboList(protocol::parsers::parse_combo_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                            let _ = tx.send(BgMsg::LeaderList(protocol::parsers::parse_leader_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                            let _ = tx.send(BgMsg::KoList(protocol::parsers::parse_ko_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::BT_QUERY, &[]) {
                            let _ = tx.send(BgMsg::BtStatus(protocol::parsers::parse_bt_binary(&r.payload)));
                        }
                    });
                }
                2 => {
                    // Macros: refresh via binary
                    std::thread::spawn(move || {
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(resp) = ser.send_binary(protocol::binary::cmd::LIST_MACROS, &[]) {
                            let macros = protocol::parsers::parse_macros_binary(&resp.payload);
                            let _ = tx.send(BgMsg::MacroList(macros));
                        }
                    });
                }
                3 => {
                    // Stats: refresh heatmap + bigrams via binary
                    std::thread::spawn(move || {
                        use protocol::binary::cmd;
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
                            let (data, max) = protocol::parsers::parse_keystats_binary(&r.payload);
                            let _ = tx.send(BgMsg::HeatmapData(data, max));
                        }
                        // Bigrams: keep text query (binary format needs dedicated parser)
                        let bigram_lines = if let Ok(r) = ser.send_binary(protocol::binary::cmd::BIGRAMS_TEXT, &[]) {
                            String::from_utf8_lossy(&r.payload).lines().map(|l| l.to_string()).collect()
                        } else { Vec::new() };
                        let _ = tx.send(BgMsg::BigramLines(bigram_lines));
                    });
                }
                _ => {}
            }
        });
    }
}
