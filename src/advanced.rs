use crate::context::{mod_idx_to_byte, AppContext, BgMsg};
use crate::protocol;
use crate::{AdvancedBridge, AppState, MainWindow};
use slint::{ComponentHandle, Model, SharedString};

/// Wire up all advanced callbacks: refresh, delete combo/leader/KO,
/// set trilayer, BT switch, TAMA action, toggle autoshift, create combo/KO/leader.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    // --- Advanced: refresh ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_refresh_advanced(move || {
            let serial = serial.clone();
            let tx = tx.clone();
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
        });
    }

    // --- Advanced: delete combo ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_combo(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::COMBO_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                    let _ = tx.send(BgMsg::ComboList(protocol::parsers::parse_combo_binary(&r.payload)));
                }
            });
        });
    }

    // --- Advanced: delete leader ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_leader(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::LEADER_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                    let _ = tx.send(BgMsg::LeaderList(protocol::parsers::parse_leader_binary(&r.payload)));
                }
            });
        });
    }

    // --- Advanced: delete KO ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_ko(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::KO_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                    let _ = tx.send(BgMsg::KoList(protocol::parsers::parse_ko_binary(&r.payload)));
                }
            });
        });
    }

    // --- Advanced: set trilayer ---
    {
        let serial = ctx.serial.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_set_trilayer(move |l1, l2, l3| {
            let payload = vec![l1 as u8, l2 as u8, l3 as u8];
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(protocol::binary::cmd::TRILAYER_SET, &payload);
            });
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text(
                    SharedString::from(format!("Tri-layer: {} + {} → {}", l1, l2, l3))
                );
            }
        });
    }

    // --- Advanced: BT switch ---
    {
        let serial = ctx.serial.clone();

        window.global::<AdvancedBridge>().on_bt_switch(move |slot| {
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(protocol::binary::cmd::BT_SWITCH, &[slot as u8]);
            });
        });
    }

    // --- Advanced: TAMA action ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_tama_action(move |action| {
            use protocol::binary::cmd;
            let action_cmd = match action.as_str() {
                "feed" => cmd::TAMA_FEED,
                "play" => cmd::TAMA_PLAY,
                "sleep" => cmd::TAMA_SLEEP,
                "meds" => cmd::TAMA_MEDICINE,
                "toggle" => cmd::TAMA_ENABLE, // toggle handled by firmware
                _ => return,
            };
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(action_cmd, &[]);
                if let Ok(r) = ser.send_binary(cmd::TAMA_QUERY, &[]) {
                    let _ = tx.send(BgMsg::TamaStatus(protocol::parsers::parse_tama_binary(&r.payload)));
                }
            });
        });
    }

    // --- Advanced: toggle autoshift ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<AdvancedBridge>().on_toggle_autoshift(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                match ser.send_binary(protocol::binary::cmd::AUTOSHIFT_TOGGLE, &[]) {
                    Ok(r) => {
                        let enabled = r.payload.first().copied().unwrap_or(0);
                        let status = if enabled != 0 { "Autoshift: ON" } else { "Autoshift: OFF" };
                        let _ = tx.send(BgMsg::AutoshiftStatus(status.to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send(BgMsg::AutoshiftStatus(format!("Error: {}", e)));
                    }
                }
            });
        });
    }

    // --- Advanced: create combo ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_create_combo(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let adv = w.global::<AdvancedBridge>();
            let r1 = adv.get_new_combo_r1() as u8;
            let c1 = adv.get_new_combo_c1() as u8;
            let r2 = adv.get_new_combo_r2() as u8;
            let c2 = adv.get_new_combo_c2() as u8;
            let result = adv.get_new_combo_result_code() as u8;
            let key1_name = adv.get_new_combo_key1_name();
            let key2_name = adv.get_new_combo_key2_name();
            if key1_name == "Pick..." || key2_name == "Pick..." {
                w.global::<AppState>().set_status_text("Pick both keys first".into());
                return;
            }
            let next_idx = adv.get_combos().row_count() as u8;
            let payload = protocol::binary::combo_set_payload(next_idx, r1, c1, r2, c2, result);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::COMBO_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                    let _ = tx.send(BgMsg::ComboList(protocol::parsers::parse_combo_binary(&r.payload)));
                }
            });
            w.global::<AppState>().set_status_text("Creating combo...".into());
        });
    }

    // --- Advanced: create KO ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_create_ko(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let adv = w.global::<AdvancedBridge>();
            let trig = adv.get_new_ko_trigger_code() as u8;
            let trig_mod = (adv.get_new_ko_trig_ctrl() as u8)
                | ((adv.get_new_ko_trig_shift() as u8) << 1)
                | ((adv.get_new_ko_trig_alt() as u8) << 2);
            let result = adv.get_new_ko_result_code() as u8;
            let res_mod = (adv.get_new_ko_res_ctrl() as u8)
                | ((adv.get_new_ko_res_shift() as u8) << 1)
                | ((adv.get_new_ko_res_alt() as u8) << 2);
            let next_idx = adv.get_key_overrides().row_count() as u8;
            let payload = protocol::binary::ko_set_payload(next_idx, trig, trig_mod, result, res_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::KO_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                    let _ = tx.send(BgMsg::KoList(protocol::parsers::parse_ko_binary(&r.payload)));
                }
            });
            w.global::<AppState>().set_status_text("Creating key override...".into());
        });
    }

    // --- Advanced: create leader ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_create_leader(move |result_code: i32, mod_idx: i32| {
            let Some(w) = window_weak.upgrade() else { return };
            let adv = w.global::<AdvancedBridge>();
            let count = adv.get_new_leader_seq_count() as usize;
            let mut sequence = Vec::new();
            if count > 0 { sequence.push(adv.get_new_leader_seq0_code() as u8); }
            if count > 1 { sequence.push(adv.get_new_leader_seq1_code() as u8); }
            if count > 2 { sequence.push(adv.get_new_leader_seq2_code() as u8); }
            if count > 3 { sequence.push(adv.get_new_leader_seq3_code() as u8); }
            let result = result_code as u8;
            let result_mod = mod_idx_to_byte(mod_idx);
            let next_idx = adv.get_leaders().row_count() as u8;
            let payload = protocol::binary::leader_set_payload(next_idx, &sequence, result, result_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::LEADER_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                    let _ = tx.send(BgMsg::LeaderList(protocol::parsers::parse_leader_binary(&r.payload)));
                }
            });
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Creating leader key...".into());
            }
        });
    }
}
