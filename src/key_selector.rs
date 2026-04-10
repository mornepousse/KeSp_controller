use crate::context::AppContext;
use crate::protocol::{self as protocol, keycode};
use crate::models;
use crate::{
    AdvancedBridge, AppState, KeySelectorBridge, KeymapBridge, MainWindow,
};
use slint::{ComponentHandle, Model, SharedString};
use std::rc::Rc;

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    setup_filter(window, ctx);
    let apply_keycode = build_apply_keycode(window, ctx);
    let refresh_macro_display = crate::macros::make_refresh_display(window, ctx);
    let dispatch_keycode = build_dispatch_keycode(window, ctx, apply_keycode, refresh_macro_display);
    setup_callbacks(window, dispatch_keycode);
}

fn setup_filter(window: &MainWindow, ctx: &AppContext) {
    let keyboard_layout = ctx.keyboard_layout.clone();
    let window_weak = window.as_weak();
    window.global::<KeySelectorBridge>().on_apply_filter(move |search| {
        if let Some(w) = window_weak.upgrade() {
            let layout = *keyboard_layout.borrow();
            let all_keys = models::build_key_entries_with_layout(&layout);
            models::populate_key_categories(&w, &all_keys, &search);
        }
    });
}

fn build_apply_keycode(window: &MainWindow, ctx: &AppContext) -> Rc<dyn Fn(u16)> {
    let serial = ctx.serial.clone();
    let keys_arc = ctx.keys.clone();
    let current_keymap = ctx.current_keymap.clone();
    let current_layer = ctx.current_layer.clone();
    let keyboard_layout = ctx.keyboard_layout.clone();
    let window_weak = window.as_weak();

    Rc::new(move |code: u16| {
        let Some(w) = window_weak.upgrade() else { return };
        let key_idx = w.global::<KeymapBridge>().get_selected_key_index();
        if key_idx < 0 { return; }
        let key_idx = key_idx as usize;
        let keys = keys_arc.borrow();
        if key_idx >= keys.len() { return; }

        let kp = &keys[key_idx];
        let row = kp.row;
        let col = kp.col;
        drop(keys);
        let layer = current_layer.get() as u8;

        {
            let mut km = current_keymap.borrow_mut();
            if row < km.len() && col < km[row].len() {
                km[row][col] = code;
            }
        }

        let layout = *keyboard_layout.borrow();
        let km = current_keymap.borrow().clone();
        let keys = keys_arc.borrow().clone();
        let keycaps = w.global::<KeymapBridge>().get_keycaps();
        models::update_keycap_labels(&keycaps, &keys, &km, &layout);

        let payload = protocol::binary::setkey_payload(layer, row as u8, col as u8, code);
        let serial = serial.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            let _ = ser.send_binary(protocol::binary::cmd::SETKEY, &payload);
        });

        w.global::<AppState>().set_status_text(
            SharedString::from(format!("[{},{}] = 0x{:04X}", row, col, code))
        );
    })
}

fn build_dispatch_keycode(
    window: &MainWindow,
    ctx: &AppContext,
    apply_keycode: Rc<dyn Fn(u16)>,
    refresh_macro_display: Rc<dyn Fn()>,
) -> Rc<dyn Fn(u16)> {
    let keys_arc = ctx.keys.clone();
    let serial = ctx.serial.clone();
    let macro_steps = ctx.macro_steps.clone();
    let window_weak = window.as_weak();

    Rc::new(move |code: u16| {
        let Some(w) = window_weak.upgrade() else { return };
        let target = w.global::<KeymapBridge>().get_selector_target();
        let name = SharedString::from(keycode::decode_keycode(code));

        match target.as_str() {
            "keymap" => { apply_keycode(code); }
            "combo-result" => {
                let adv = w.global::<AdvancedBridge>();
                adv.set_new_combo_result_code(code as i32);
                adv.set_new_combo_result_name(name);
            }
            "ko-trigger" => {
                let adv = w.global::<AdvancedBridge>();
                adv.set_new_ko_trigger_code(code as i32);
                adv.set_new_ko_trigger_name(name);
            }
            "ko-result" => {
                let adv = w.global::<AdvancedBridge>();
                adv.set_new_ko_result_code(code as i32);
                adv.set_new_ko_result_name(name);
            }
            "leader-result" => {
                let adv = w.global::<AdvancedBridge>();
                adv.set_new_leader_result_code(code as i32);
                adv.set_new_leader_result_name(name);
            }
            "combo-key1" | "combo-key2" => {
                let keys = keys_arc.borrow();
                let idx = code as usize;
                if idx < keys.len() {
                    let kp = &keys[idx];
                    let adv = w.global::<AdvancedBridge>();
                    let label = SharedString::from(format!("R{}C{}", kp.row, kp.col));
                    if target.as_str() == "combo-key1" {
                        adv.set_new_combo_r1(kp.row as i32);
                        adv.set_new_combo_c1(kp.col as i32);
                        adv.set_new_combo_key1_name(label);
                    } else {
                        adv.set_new_combo_r2(kp.row as i32);
                        adv.set_new_combo_c2(kp.col as i32);
                        adv.set_new_combo_key2_name(label);
                    }
                }
            }
            "leader-seq" => {
                let adv = w.global::<AdvancedBridge>();
                let count = adv.get_new_leader_seq_count();
                match count {
                    0 => { adv.set_new_leader_seq0_code(code as i32); adv.set_new_leader_seq0_name(name); }
                    1 => { adv.set_new_leader_seq1_code(code as i32); adv.set_new_leader_seq1_name(name); }
                    2 => { adv.set_new_leader_seq2_code(code as i32); adv.set_new_leader_seq2_name(name); }
                    3 => { adv.set_new_leader_seq3_code(code as i32); adv.set_new_leader_seq3_name(name); }
                    _ => {}
                }
                if count < 4 { adv.set_new_leader_seq_count(count + 1); }
            }
            "td-action" => {
                let adv = w.global::<AdvancedBridge>();
                let td_idx = adv.get_editing_td_index();
                let slot = adv.get_editing_td_slot() as usize;
                if td_idx >= 0 && slot < 4 {
                    let tds = adv.get_tap_dances();
                    for i in 0..tds.row_count() {
                        let td = tds.row_data(i).unwrap();
                        if td.index == td_idx {
                            let actions = td.actions;
                            let mut a = actions.row_data(slot).unwrap();
                            a.name = name.clone();
                            a.code = code as i32;
                            actions.set_row_data(slot, a);

                            let mut codes = [0u16; 4];
                            for (j, code) in codes.iter_mut().enumerate().take(4.min(actions.row_count())) {
                                *code = actions.row_data(j).unwrap().code as u16;
                            }
                            let payload = protocol::binary::td_set_payload(td_idx as u8, &codes);
                            let serial = serial.clone();
                            std::thread::spawn(move || {
                                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                                let _ = ser.send_binary(protocol::binary::cmd::TD_SET, &payload);
                            });
                            w.global::<AppState>().set_status_text(
                                SharedString::from(format!("TD{} slot {} = {}", td_idx, slot, name))
                            );
                            break;
                        }
                    }
                }
            }
            "macro-step" => {
                let mut steps = macro_steps.borrow_mut();
                steps.push((code as u8, 0x00));
                drop(steps);
                refresh_macro_display();
            }
            _ => { apply_keycode(code); }
        }
    })
}

fn setup_callbacks(window: &MainWindow, dispatch_keycode: Rc<dyn Fn(u16)>) {
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_select_keycode(move |code| {
            dispatch(code as u16);
        });
    }
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_hex(move |hex_str| {
            if let Ok(code) = u16::from_str_radix(hex_str.trim(), 16) {
                dispatch(code);
            }
        });
    }
    {
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_preview_hex(move |hex_str| {
            let preview = u16::from_str_radix(hex_str.trim(), 16)
                .map(keycode::decode_keycode)
                .unwrap_or_default();
            if let Some(w) = window_weak.upgrade() {
                w.global::<KeySelectorBridge>().set_hex_preview(SharedString::from(preview));
            }
        });
    }
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_mt(move |mod_idx, key_idx| {
            let mod_nibble: u16 = match mod_idx {
                0 => 0x01, 1 => 0x02, 2 => 0x04, 3 => 0x08,
                4 => 0x10, 5 => 0x20, 6 => 0x40, 7 => 0x80,
                _ => 0x02,
            };
            let hid: u16 = match key_idx {
                0..=25 => 0x04 + key_idx as u16,
                26..=35 => 0x1E + (key_idx - 26) as u16,
                36 => 0x2C, 37 => 0x28, 38 => 0x29, 39 => 0x2B, 40 => 0x2A,
                _ => 0x04,
            };
            let code = 0x5000 | (mod_nibble << 8) | hid;
            dispatch(code);
        });
    }
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_lt(move |layer_idx, key_idx| {
            let layer = (layer_idx as u16) & 0x0F;
            let hid: u16 = match key_idx {
                0 => 0x2C, 1 => 0x28, 2 => 0x29, 3 => 0x2A, 4 => 0x2B,
                5..=9 => 0x04 + (key_idx - 5) as u16,
                _ => 0x2C,
            };
            let code = 0x4000 | (layer << 8) | hid;
            dispatch(code);
        });
    }
}
