use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::{config, MainWindow, SettingsBridge};
use slint::{ComponentHandle, SharedString};

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    setup_change_layout(window, ctx);
    setup_ota_browse(window);
    setup_ota_start(window, ctx);
    setup_config_export(window, ctx);
    setup_config_import(window, ctx);
}

// --- Change keyboard layout (AZERTY / QWERTZ / ...) ---
fn setup_change_layout(window: &MainWindow, ctx: &AppContext) {
    let keyboard_layout = ctx.keyboard_layout.clone();
    let keys = ctx.keys.clone();
    let current_keymap = ctx.current_keymap.clone();
    let window_weak = window.as_weak();

    window.global::<SettingsBridge>().on_change_layout(move |idx| {
        let all = protocol::layout_remap::KeyboardLayout::all();
        let new_layout = all.get(idx as usize).copied().unwrap_or(all[0]);

        // Update shared state
        *keyboard_layout.borrow_mut() = new_layout;

        // Persist to disk
        let mut s = protocol::settings::load();
        s.keyboard_layout = new_layout.name().to_string();
        protocol::settings::save(&s);

        // Refresh keycap labels + key selector popup
        if let Some(w) = window_weak.upgrade() {
            let keycaps = w.global::<crate::KeymapBridge>().get_keycaps();
            let km = current_keymap.borrow();
            let k = keys.borrow();
            if !km.is_empty() {
                crate::models::update_keycap_labels(&keycaps, &k, &km, &new_layout);
            }

            let all_keys = crate::models::build_key_entries_with_layout(&new_layout);
            w.global::<crate::KeySelectorBridge>().set_all_keys(slint::ModelRc::from(all_keys.clone()));
            crate::models::populate_key_categories(&w, &all_keys, "");
        }
    });
}

// --- OTA: browse ---
fn setup_ota_browse(window: &MainWindow) {
    let window_weak = window.as_weak();
    window.global::<SettingsBridge>().on_ota_browse(move || {
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("Firmware", &["bin"])
                .pick_file();
            if let Some(path) = file {
                let path_str = path.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<SettingsBridge>().set_ota_path(SharedString::from(path_str.as_str()));
                    }
                });
            }
        });
    });
}

// --- OTA: start ---
fn setup_ota_start(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();

    window.global::<SettingsBridge>().on_ota_start(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let settings = w.global::<SettingsBridge>();
        let path = settings.get_ota_path().to_string();
        if path.is_empty() { return; }

        let firmware = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) => {
                let _ = tx.send(BgMsg::OtaDone(Err(format!("Cannot read {}: {}", path, e))));
                return;
            }
        };

        settings.set_ota_flashing(true);
        settings.set_ota_progress(0.0);
        settings.set_ota_status(SharedString::from("Starting OTA..."));

        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write, BufRead, BufReader};

            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            let total = firmware.len();
            let chunk_size = 4096usize;

            // Step 1: Send OTA <size> command
            let cmd = format!("OTA {}", total);
            if let Err(e) = ser.send_command(&cmd) {
                let _ = tx.send(BgMsg::OtaDone(Err(format!("Send OTA cmd failed: {}", e))));
                return;
            }

            // Step 2: Wait for OTA_READY (read lines with timeout)
            let _ = tx.send(BgMsg::OtaProgress(0.0, "Waiting for OTA_READY...".into()));
            let port = match ser.port_mut() {
                Some(p) => p,
                None => {
                    let _ = tx.send(BgMsg::OtaDone(Err("Port not available".into())));
                    return;
                }
            };

            // Read until we get OTA_READY or timeout
            let old_timeout = port.timeout();
            let _ = port.set_timeout(std::time::Duration::from_secs(5));
            let mut got_ready = false;
            let port_clone = port.try_clone().unwrap();
            let mut reader = BufReader::new(port_clone);
            for _ in 0..20 {
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() && line.contains("OTA_READY") {
                    got_ready = true;
                    break;
                }
            }
            drop(reader);

            if !got_ready {
                let _ = port.set_timeout(old_timeout);
                let _ = tx.send(BgMsg::OtaDone(Err("Firmware did not respond OTA_READY".into())));
                return;
            }

            // Step 3: Send chunks and wait for ACK after each
            let num_chunks = total.div_ceil(chunk_size);
            let _ = port.set_timeout(std::time::Duration::from_secs(5));

            for (i, chunk) in firmware.chunks(chunk_size).enumerate() {
                // Send chunk
                if let Err(e) = port.write_all(chunk) {
                    let _ = port.set_timeout(old_timeout);
                    let _ = tx.send(BgMsg::OtaDone(Err(format!("Write chunk {} failed: {}", i, e))));
                    return;
                }
                let _ = port.flush();

                let progress = (i + 1) as f32 / num_chunks as f32;
                let _ = tx.send(BgMsg::OtaProgress(progress * 0.95, format!(
                    "Chunk {}/{} ({} KB / {} KB)",
                    i + 1, num_chunks,
                    ((i + 1) * chunk_size).min(total) / 1024,
                    total / 1024
                )));

                // Wait for ACK line
                let mut ack_buf = [0u8; 256];
                let mut ack = String::new();
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_secs(5) {
                    match port.read(&mut ack_buf) {
                        Ok(n) if n > 0 => {
                            ack.push_str(&String::from_utf8_lossy(&ack_buf[..n]));
                            if ack.contains("OTA_OK") || ack.contains("OTA_DONE") || ack.contains("OTA_FAIL") {
                                break;
                            }
                        }
                        _ => std::thread::sleep(std::time::Duration::from_millis(10)),
                    }
                }

                if ack.contains("OTA_FAIL") {
                    let _ = port.set_timeout(old_timeout);
                    let _ = tx.send(BgMsg::OtaDone(Err(format!("Firmware error: {}", ack.trim()))));
                    return;
                }
                if ack.contains("OTA_DONE") {
                    break; // Firmware signals all received
                }
            }

            let _ = port.set_timeout(old_timeout);
            let _ = tx.send(BgMsg::OtaProgress(1.0, "OTA complete, rebooting...".into()));
            let _ = tx.send(BgMsg::OtaDone(Ok(())));
        });
    });
}

fn read_config_mask(w: &MainWindow) -> u8 {
    let tb = w.global::<crate::ToolsBridge>();
    let mut mask: u8 = 0;
    if tb.get_nvs_keymaps()  { mask |= 0x01; }
    if tb.get_nvs_macros()   { mask |= 0x02; }
    if tb.get_nvs_features() { mask |= 0x08; }
    mask // 0 = all
}

// --- Config Export ---
fn setup_config_export(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();

    window.global::<SettingsBridge>().on_config_export(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let mask = read_config_mask(&w);
        w.global::<SettingsBridge>().set_config_busy(true);
        w.global::<SettingsBridge>().set_config_progress(0.0);
        w.global::<SettingsBridge>().set_config_status(SharedString::from("Reading config..."));

        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let result = config::export_config(&serial, &tx, mask);
            let _ = tx.send(BgMsg::ConfigDone(result));
        });
    });
}

// --- Config Import ---
fn setup_config_import(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();

    window.global::<SettingsBridge>().on_config_import(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let mask = read_config_mask(&w);
        let serial = serial.clone();
        let tx = tx.clone();
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("KeSp Config", &["json"])
                .pick_file();
            let Some(path) = file else { return };

            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        let s = w.global::<SettingsBridge>();
                        s.set_config_busy(true);
                        s.set_config_progress(0.0);
                        s.set_config_status(SharedString::from("Importing config..."));
                    }
                }
            });

            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(e) => {
                    let _ = tx.send(BgMsg::ConfigDone(Err(format!("Read error: {}", e))));
                    return;
                }
            };
            let config = match protocol::config_io::KeyboardConfig::from_json(&json) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BgMsg::ConfigDone(Err(format!("Parse error: {}", e))));
                    return;
                }
            };

            let result = config::import_config(&serial, &tx, &config, mask);
            let _ = tx.send(BgMsg::ConfigDone(result));
        });
    });
}
