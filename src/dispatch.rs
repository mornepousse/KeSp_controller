use crate::context::{AppContext, BgMsg};
use crate::protocol::{self as protocol, keycode};
use crate::models;
use crate::{
    AdvancedBridge, AppState, BigramData, ComboData, ConnectionState, FingerLoadData,
    FlasherBridge, HandBalanceData, KeyOverrideData, KeymapBridge, LayoutBridge, LeaderData,
    MacroBridge, MacroData, MainWindow, RowUsageData, SettingsBridge, StatsBridge,
    TapDanceAction, TapDanceData, TopKeyData, LayerInfo,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::mpsc;

/// Start the event loop with BgMsg polling, WPM timer, and layout auto-refresh.
pub fn run(window: &MainWindow, ctx: &AppContext, bg_rx: mpsc::Receiver<BgMsg>) {
    let window_weak = window.as_weak();
    let keys_arc = ctx.keys.clone();
    let current_keymap = ctx.current_keymap.clone();
    let keyboard_layout = ctx.keyboard_layout.clone();
    let heatmap_data = ctx.heatmap_data.clone();

    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let Some(window) = window_weak.upgrade() else { return };

            while let Ok(msg) = bg_rx.try_recv() {
                handle_msg(&window, msg, &keys_arc, &current_keymap, &keyboard_layout, &heatmap_data);
            }
        },
    );

    // WPM polling timer (5s)
    let wpm_timer = slint::Timer::default();
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak2 = window.as_weak();

        wpm_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(5),
            move || {
                let Some(w) = window_weak2.upgrade() else { return };
                if w.global::<AppState>().get_connection() != ConnectionState::Connected { return; }
                let serial = serial.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let Ok(mut ser) = serial.try_lock() else { return };
                    if let Ok(r) = ser.send_binary(protocol::binary::cmd::WPM_QUERY, &[]) {
                        let wpm = if r.payload.len() >= 2 {
                            u16::from_le_bytes([r.payload[0], r.payload[1]])
                        } else { 0 };
                        let _ = tx.send(BgMsg::Wpm(wpm));
                    }
                });
            },
        );
    }

    // Layout auto-refresh timer (5s)
    let layout_timer = slint::Timer::default();
    {
        let window_weak3 = window.as_weak();
        layout_timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(5),
            move || {
                let Some(w) = window_weak3.upgrade() else { return };
                let lb = w.global::<LayoutBridge>();
                if !lb.get_auto_refresh() { return; }
                let path = lb.get_file_path().to_string();
                if path.is_empty() { return; }
                if let Ok(json) = std::fs::read_to_string(&path) {
                    models::populate_layout_preview(&w, &json);
                }
            },
        );
    }

    let _keep_timer = timer;
    let _keep_wpm = wpm_timer;
    let _keep_layout = layout_timer;
    window.run().unwrap();
}

fn handle_msg(
    window: &MainWindow,
    msg: BgMsg,
    keys_arc: &Rc<std::cell::RefCell<Vec<crate::protocol::layout::KeycapPos>>>,
    current_keymap: &Rc<std::cell::RefCell<Vec<Vec<u16>>>>,
    keyboard_layout: &Rc<std::cell::RefCell<protocol::layout_remap::KeyboardLayout>>,
    heatmap_data: &Rc<std::cell::RefCell<Vec<Vec<u32>>>>,
) {
    match msg {
        BgMsg::Connected(port, fw, names, km) => {
            let app = window.global::<AppState>();
            app.set_connection(ConnectionState::Connected);
            app.set_firmware_version(SharedString::from(&fw));
            app.set_status_text(SharedString::from(format!("Connected to {}", port)));

            let new_layers = models::build_layer_model(&names);
            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));

            *current_keymap.borrow_mut() = km.clone();
            let keycaps = window.global::<KeymapBridge>().get_keycaps();
            let layout = keyboard_layout.borrow();
            let keys = keys_arc.borrow();
            models::update_keycap_labels(&keycaps, &keys, &km, &layout);
        }
        BgMsg::ConnectError(e) => {
            let app = window.global::<AppState>();
            app.set_connection(ConnectionState::Disconnected);
            app.set_status_text(SharedString::from(format!("Error: {}", e)));
        }
        BgMsg::Keymap(km) => {
            *current_keymap.borrow_mut() = km.clone();
            let keycaps = window.global::<KeymapBridge>().get_keycaps();
            let layout = keyboard_layout.borrow();
            let keys = keys_arc.borrow();
            models::update_keycap_labels(&keycaps, &keys, &km, &layout);
            window.global::<AppState>().set_status_text("Keymap loaded".into());
        }
        BgMsg::LayerNames(names) => {
            let active = window.global::<KeymapBridge>().get_active_layer() as usize;
            let layers: Vec<LayerInfo> = names.iter().enumerate().map(|(i, name)| LayerInfo {
                index: i as i32,
                name: SharedString::from(name.as_str()),
                active: i == active,
            }).collect();
            window.global::<KeymapBridge>().set_layers(
                ModelRc::from(Rc::new(VecModel::from(layers)))
            );
        }
        BgMsg::Disconnected => {
            let app = window.global::<AppState>();
            app.set_connection(ConnectionState::Disconnected);
            app.set_firmware_version(SharedString::default());
            app.set_status_text("Disconnected".into());
        }
        BgMsg::LayoutJson(new_keys) => {
            *keys_arc.borrow_mut() = new_keys.clone();
            let new_model = models::build_keycap_model(&new_keys);
            let km = current_keymap.borrow();
            if !km.is_empty() {
                let layout = keyboard_layout.borrow();
                models::update_keycap_labels(&new_model, &new_keys, &km, &layout);
            }
            let mut max_x: f32 = 0.0;
            let mut max_y: f32 = 0.0;
            for kp in &new_keys {
                if kp.x + kp.w > max_x { max_x = kp.x + kp.w; }
                if kp.y + kp.h > max_y { max_y = kp.y + kp.h; }
            }
            let bridge = window.global::<KeymapBridge>();
            bridge.set_content_width(max_x);
            bridge.set_content_height(max_y);
            bridge.set_keycaps(ModelRc::from(new_model));
            window.global::<AppState>().set_status_text(
                SharedString::from(format!("Layout loaded ({} keys)", new_keys.len()))
            );
        }
        BgMsg::BigramLines(lines) => {
            let entries = protocol::stats::parse_bigram_lines(&lines);
            let analysis = protocol::stats::analyze_bigrams(&entries);
            window.global::<StatsBridge>().set_bigrams(BigramData {
                alt_hand_pct: analysis.alt_hand_pct,
                same_hand_pct: analysis.same_hand_pct,
                sfb_pct: analysis.sfb_pct,
                total: analysis.total as i32,
            });
        }
        BgMsg::FlashProgress(progress, msg) => {
            let f = window.global::<FlasherBridge>();
            f.set_flash_progress(progress);
            f.set_flash_status(SharedString::from(msg));
        }
        BgMsg::FlashDone(result) => {
            let f = window.global::<FlasherBridge>();
            f.set_flashing(false);
            match result {
                Ok(()) => {
                    f.set_flash_progress(1.0);
                    f.set_flash_status(SharedString::from("Flash complete!"));
                    window.global::<AppState>().set_status_text("Flash complete!".into());
                }
                Err(e) => {
                    f.set_flash_status(SharedString::from(format!("Error: {}", e)));
                    window.global::<AppState>().set_status_text(
                        SharedString::from(format!("Flash error: {}", e))
                    );
                }
            }
        }
        BgMsg::HeatmapData(data, max) => {
            *heatmap_data.borrow_mut() = data.clone();
            let keycaps = window.global::<KeymapBridge>().get_keycaps();
            let keys = keys_arc.borrow();
            for i in 0..keycaps.row_count() {
                if i >= keys.len() { break; }
                let mut item = keycaps.row_data(i).unwrap();
                let kp = &keys[i];
                let count = data.get(kp.row).and_then(|r| r.get(kp.col)).copied().unwrap_or(0);
                item.heat = if max > 0 { count as f32 / max as f32 } else { 0.0 };
                keycaps.set_row_data(i, item);
            }
            drop(keys);

            let km = current_keymap.borrow();
            let balance = protocol::stats::hand_balance(&data);
            let fingers = protocol::stats::finger_load(&data);
            let rows = protocol::stats::row_usage(&data);
            let top = protocol::stats::top_keys(&data, &km, 10);
            let dead = protocol::stats::dead_keys(&data, &km);

            let stats = window.global::<StatsBridge>();
            stats.set_hand_balance(HandBalanceData {
                left_pct: balance.left_pct, right_pct: balance.right_pct, total: balance.total as i32,
            });
            stats.set_total_presses(balance.total as i32);
            stats.set_finger_load(ModelRc::from(Rc::new(VecModel::from(
                fingers.iter().map(|f| FingerLoadData {
                    name: SharedString::from(&f.name), pct: f.pct, count: f.count as i32,
                }).collect::<Vec<_>>()
            ))));
            stats.set_row_usage(ModelRc::from(Rc::new(VecModel::from(
                rows.iter().map(|r| RowUsageData {
                    name: SharedString::from(&r.name), pct: r.pct, count: r.count as i32,
                }).collect::<Vec<_>>()
            ))));
            stats.set_top_keys(ModelRc::from(Rc::new(VecModel::from(
                top.iter().map(|t| TopKeyData {
                    name: SharedString::from(&t.name), finger: SharedString::from(&t.finger),
                    count: t.count as i32, pct: t.pct,
                }).collect::<Vec<_>>()
            ))));
            stats.set_dead_keys(ModelRc::from(Rc::new(VecModel::from(
                dead.iter().map(|d| SharedString::from(d.as_str())).collect::<Vec<_>>()
            ))));
            window.global::<AppState>().set_status_text(
                SharedString::from(format!("Stats loaded ({} total presses, max {})", balance.total, max))
            );
        }
        BgMsg::TextLines(_tag, _lines) => {}
        BgMsg::TdList(td_data) => {
            let model: Vec<TapDanceData> = td_data.iter().enumerate()
                .filter(|(_, actions)| actions.iter().any(|&a| a != 0))
                .map(|(i, actions)| TapDanceData {
                    index: i as i32,
                    actions: ModelRc::from(Rc::new(VecModel::from(
                        actions.iter().map(|&a| TapDanceAction {
                            name: SharedString::from(keycode::decode_keycode(a)),
                            code: a as i32,
                        }).collect::<Vec<_>>()
                    ))),
                }).collect();
            window.global::<AdvancedBridge>().set_tap_dances(
                ModelRc::from(Rc::new(VecModel::from(model)))
            );
        }
        BgMsg::ComboList(combo_data) => {
            let model: Vec<ComboData> = combo_data.iter().map(|c| ComboData {
                index: c.index as i32,
                key1: SharedString::from(format!("R{}C{}", c.r1, c.c1)),
                key2: SharedString::from(format!("R{}C{}", c.r2, c.c2)),
                result: SharedString::from(keycode::decode_keycode(c.result)),
            }).collect();
            window.global::<AdvancedBridge>().set_combos(
                ModelRc::from(Rc::new(VecModel::from(model)))
            );
        }
        BgMsg::LeaderList(leader_data) => {
            let model: Vec<LeaderData> = leader_data.iter().map(|l| {
                let seq: Vec<String> = l.sequence.iter().map(|&k| keycode::hid_key_name(k)).collect();
                LeaderData {
                    index: l.index as i32,
                    sequence: SharedString::from(seq.join(" → ")),
                    result: SharedString::from(keycode::hid_key_name(l.result)),
                }
            }).collect();
            window.global::<AdvancedBridge>().set_leaders(
                ModelRc::from(Rc::new(VecModel::from(model)))
            );
        }
        BgMsg::KoList(ko_data) => {
            let model: Vec<KeyOverrideData> = ko_data.iter().enumerate().map(|(i, ko)| {
                let trig_key = keycode::hid_key_name(ko[0]);
                let trig_mod = keycode::mod_name(ko[1]);
                let res_key = keycode::hid_key_name(ko[2]);
                let res_mod = keycode::mod_name(ko[3]);
                let trigger = if ko[1] != 0 { format!("{}+{}", trig_mod, trig_key) } else { trig_key };
                let result = if ko[3] != 0 { format!("{}+{}", res_mod, res_key) } else { res_key };
                KeyOverrideData {
                    index: i as i32,
                    trigger: SharedString::from(trigger),
                    result: SharedString::from(result),
                }
            }).collect();
            window.global::<AdvancedBridge>().set_key_overrides(
                ModelRc::from(Rc::new(VecModel::from(model)))
            );
        }
        BgMsg::BtStatus(lines) => {
            window.global::<AdvancedBridge>().set_bt_status(SharedString::from(lines.join("\n")));
        }
        BgMsg::TamaStatus(lines) => {
            window.global::<AdvancedBridge>().set_tama_status(SharedString::from(lines.join("\n")));
        }
        BgMsg::AutoshiftStatus(text) => {
            window.global::<AdvancedBridge>().set_autoshift_status(SharedString::from(text));
        }
        BgMsg::Wpm(wpm) => {
            window.global::<AppState>().set_wpm(wpm as i32);
        }
        BgMsg::OtaProgress(progress, msg) => {
            let s = window.global::<SettingsBridge>();
            s.set_ota_progress(progress);
            s.set_ota_status(SharedString::from(msg));
        }
        BgMsg::OtaDone(result) => {
            let s = window.global::<SettingsBridge>();
            s.set_ota_flashing(false);
            match result {
                Ok(()) => { s.set_ota_progress(1.0); s.set_ota_status("OTA complete!".into()); }
                Err(e) => { s.set_ota_status(SharedString::from(format!("OTA error: {}", e))); }
            }
        }
        BgMsg::ConfigProgress(progress, msg) => {
            let s = window.global::<SettingsBridge>();
            s.set_config_progress(progress);
            s.set_config_status(SharedString::from(msg));
        }
        BgMsg::ConfigDone(result) => {
            let s = window.global::<SettingsBridge>();
            s.set_config_busy(false);
            match result {
                Ok(msg) => { s.set_config_progress(1.0); s.set_config_status(SharedString::from(msg)); }
                Err(e) => { s.set_config_progress(0.0); s.set_config_status(SharedString::from(format!("Error: {}", e))); }
            }
        }
        BgMsg::MacroList(macros) => {
            let model: Vec<MacroData> = macros.iter().map(|m| {
                let steps_str: Vec<String> = m.steps.iter().map(|s| {
                    if s.is_delay() { format!("T({})", s.delay_ms()) }
                    else { keycode::hid_key_name(s.keycode).to_string() }
                }).collect();
                MacroData {
                    slot: m.slot as i32,
                    name: SharedString::from(&m.name),
                    steps: SharedString::from(steps_str.join(" ")),
                }
            }).collect();
            let max_slot = model.iter().fold(-1i32, |acc, m| acc.max(m.slot));
            let mb = window.global::<MacroBridge>();
            mb.set_macros(ModelRc::from(Rc::new(VecModel::from(model))));
            let next_slot = max_slot + 1;
            if next_slot > mb.get_new_slot_idx() {
                mb.set_new_slot_idx(next_slot);
            }
        }
    }
}
