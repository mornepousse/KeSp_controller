use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::protocol::serial::SerialManager;
use crate::{AppState, ConnectionBridge, ConnectionState, MainWindow};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::mpsc;

/// Connect to `port` (or auto-detect when `None`) and load the device state.
///
/// Both the wired keyboard and the wireless dongle expose the same command set
/// (keymap, layers, layout, stats…), so the load path is identical; only the USB
/// identity tells them apart (`SerialManager::is_dongle`).
fn connect_and_load(ser: &mut SerialManager, tx: &mpsc::Sender<BgMsg>, port: Option<String>) {
    let result = match port {
        Some(p) => ser.connect(&p).map(|_| p),
        None => ser.auto_connect(),
    };
    match result {
        Ok(port_name) => {
            let fw = ser.get_firmware_version().unwrap_or_default();
            let _ = tx.send(BgMsg::RfModeChanged(ser.is_dongle));
            let _ = tx.send(BgMsg::ConnectionMode(ser.connection_kind()));

            // Le layout AVANT la keymap : c'est lui qui porte la géométrie, et
            // `get_keymap` en a besoin pour découper le payload en rangées.
            match ser.get_layout_json() {
                Ok(json) => {
                    let (rows, cols) = protocol::device::resolve_geometry(
                        protocol::layout::parse_geometry(&json),
                        None,
                    );
                    ser.profile.rows = rows;
                    ser.profile.cols = cols;
                    if let Some(name) = protocol::layout::parse_name(&json) {
                        ser.profile.name = name;
                    }
                    match protocol::layout::parse_json(&json) {
                        Ok(keys) => {
                            let _ = tx.send(BgMsg::LayoutJson(keys));
                        }
                        Err(e) => eprintln!("Layout parse error: {}", e),
                    }
                }
                // Pas de layout : le profil garde ses valeurs par défaut, et la
                // keymap se parsera avec elles. Un dongle répond ERR_UNKNOWN ici.
                Err(e) => eprintln!("get_layout_json error: {}", e),
            }

            let _ = tx.send(BgMsg::Profile(ser.profile.clone()));

            let names = ser.get_layer_names().unwrap_or_default();
            let km = match ser.get_keymap(0) {
                Ok(km) => km,
                Err(e) => {
                    eprintln!(
                        "get_keymap error (geometrie {}x{}): {}",
                        ser.profile.rows, ser.profile.cols, e
                    );
                    Vec::new()
                }
            };
            let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
        }
        Err(e) => {
            let _ = tx.send(BgMsg::ConnectError(e));
        }
    }
}

/// Populate the connection dropdown with the KaSe-family ports currently present.
fn refresh_port_list(window: &MainWindow) {
    let ports = SerialManager::list_kase_ports();
    let labels: Vec<SharedString> = ports
        .iter()
        .map(|(_, label, _)| SharedString::from(label))
        .collect();
    let cb = window.global::<ConnectionBridge>();
    if cb.get_selected_index() as usize >= labels.len() {
        cb.set_selected_index(0);
    }
    cb.set_port_labels(ModelRc::from(Rc::new(VecModel::from(labels))));
}

/// Auto-connect to the keyboard on startup.
pub fn auto_connect(window: &MainWindow, ctx: &AppContext) {
    refresh_port_list(window);

    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    window.global::<AppState>().set_status_text("Scanning ports...".into());
    window.global::<AppState>().set_connection(ConnectionState::Connecting);

    std::thread::spawn(move || {
        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
        connect_and_load(&mut ser, &tx, None);
    });
}

/// Wire up Connect, Disconnect, refresh_ports, and tab-change auto-refresh.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    // --- Connect callback (connects to the port selected in the dropdown) ---
    {
        let serial_c = ctx.serial.clone();
        let tx_c = ctx.bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_connect(move || {
            let Some(w) = window_weak.upgrade() else { return };
            w.global::<AppState>().set_status_text("Connecting...".into());
            w.global::<AppState>().set_connection(ConnectionState::Connecting);

            // Resolve the chosen port; fall back to auto-detect if none/out of range.
            let idx = w.global::<ConnectionBridge>().get_selected_index() as usize;
            let port = SerialManager::list_kase_ports()
                .into_iter()
                .nth(idx)
                .map(|(path, _, _)| path);

            let serial = serial_c.clone();
            let tx = tx_c.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                connect_and_load(&mut ser, &tx, port);
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

    // --- Refresh ports callback ---
    {
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_refresh_ports(move || {
            let Some(w) = window_weak.upgrade() else { return };
            refresh_port_list(&w);
        });
    }

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
                6 => {
                    // Dongle: refresh RF status, pair list, and battery (silent if not a dongle).
                    if !w.global::<AppState>().get_is_dongle() { return; }
                    std::thread::spawn(move || {
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        crate::dongle::fetch_rf_status(&mut ser, &tx);
                        crate::dongle::fetch_pair_list(&mut ser, &tx);
                        crate::dongle::fetch_battery(&mut ser, &tx);
                    });
                }
                _ => {}
            }
        });
    }
}
