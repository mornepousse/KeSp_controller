use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::protocol::keycode;
use crate::{AppState, MacroBridge, MacroStepDisplay, MainWindow};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

/// Build a closure that refreshes the macro step display from `ctx.macro_steps`.
pub fn make_refresh_display(window: &MainWindow, ctx: &AppContext) -> Rc<dyn Fn()> {
    let macro_steps = ctx.macro_steps.clone();
    let window_weak = window.as_weak();
    Rc::new(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let steps = macro_steps.borrow();
        let display: Vec<MacroStepDisplay> = steps.iter().map(|&(kc, _md)| {
            if kc == 0xFF {
                MacroStepDisplay {
                    label: SharedString::from(format!("T {}ms", _md as u32 * 10)),
                    is_delay: true,
                }
            } else {
                MacroStepDisplay {
                    label: SharedString::from(keycode::hid_key_name(kc)),
                    is_delay: false,
                }
            }
        }).collect();
        let text: Vec<String> = steps.iter().map(|&(kc, md)| {
            if kc == 0xFF { format!("T({})", md as u32 * 10) }
            else { format!("D({:02X})", kc) }
        }).collect();
        let mb = w.global::<MacroBridge>();
        mb.set_new_steps(ModelRc::from(Rc::new(VecModel::from(display))));
        mb.set_new_steps_text(SharedString::from(text.join(" ")));
    })
}

/// Wire up all macro callbacks: refresh, add delay, add shortcut, remove last, clear, save, delete.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    // --- Macros: refresh ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<MacroBridge>().on_refresh_macros(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if ser.v2 {
                    if let Ok(resp) = ser.send_binary(protocol::binary::cmd::LIST_MACROS, &[]) {
                        let macros = protocol::parsers::parse_macros_binary(&resp.payload);
                        let _ = tx.send(BgMsg::MacroList(macros));
                    }
                } else {
                    // Legacy fallback — should not happen with v2 firmware
                    let _ = ser.send_binary(protocol::binary::cmd::LIST_MACROS, &[]);
                }
            });
        });
    }

    // --- Macros: add delay step ---
    {
        let macro_steps = ctx.macro_steps.clone();
        let refresh = make_refresh_display(window, ctx);

        window.global::<MacroBridge>().on_add_delay_step(move |ms| {
            let units = (ms as u8) / 10;
            macro_steps.borrow_mut().push((0xFF, units));
            refresh();
        });
    }

    // --- Macros: add shortcut preset ---
    {
        let macro_steps = ctx.macro_steps.clone();
        let refresh = make_refresh_display(window, ctx);

        window.global::<MacroBridge>().on_add_shortcut(move |shortcut| {
            let ctrl = |key: u8| vec![(0xE0u8, 0u8), (key, 0)];
            let steps: Vec<(u8, u8)> = match shortcut.as_str() {
                "ctrl+c" => ctrl(0x06),
                "ctrl+v" => ctrl(0x19),
                "ctrl+x" => ctrl(0x1B),
                "ctrl+z" => ctrl(0x1D),
                "ctrl+y" => ctrl(0x1C),
                "ctrl+s" => ctrl(0x16),
                "ctrl+a" => ctrl(0x04),
                "alt+f4" => vec![(0xE2, 0), (0x3D, 0)],
                _ => return,
            };
            macro_steps.borrow_mut().extend(steps);
            refresh();
        });
    }

    // --- Macros: remove last step ---
    {
        let macro_steps = ctx.macro_steps.clone();
        let refresh = make_refresh_display(window, ctx);

        window.global::<MacroBridge>().on_remove_last_step(move || {
            macro_steps.borrow_mut().pop();
            refresh();
        });
    }

    // --- Macros: clear steps ---
    {
        let macro_steps = ctx.macro_steps.clone();
        let refresh = make_refresh_display(window, ctx);

        window.global::<MacroBridge>().on_clear_steps(move || {
            macro_steps.borrow_mut().clear();
            refresh();
        });
    }

    // --- Macros: save ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let macro_steps = ctx.macro_steps.clone();
        let window_weak = window.as_weak();

        window.global::<MacroBridge>().on_save_macro(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let mb = w.global::<MacroBridge>();
            let slot_num = mb.get_new_slot_idx() as u8;
            mb.set_new_slot_idx(slot_num as i32 + 1);
            let name = mb.get_new_name().to_string();
            let steps = macro_steps.borrow();
            let steps_str: Vec<String> = steps.iter().map(|&(kc, md)| {
                format!("{:02X}:{:02X}", kc, md)
            }).collect();
            let steps_text = steps_str.join(",");
            drop(steps);
            let payload = protocol::binary::macro_add_seq_payload(slot_num, &name, &steps_text);

            macro_steps.borrow_mut().clear();
            mb.set_new_name(SharedString::default());
            mb.set_new_steps(ModelRc::from(Rc::new(VecModel::<MacroStepDisplay>::default())));
            mb.set_new_steps_text(SharedString::default());

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::MACRO_ADD_SEQ, &payload);
                if let Ok(resp) = ser.send_binary(cmd::LIST_MACROS, &[]) {
                    let macros = protocol::parsers::parse_macros_binary(&resp.payload);
                    let _ = tx.send(BgMsg::MacroList(macros));
                }
            });
            w.global::<AppState>().set_status_text(
                SharedString::from(format!("Saving macro #{}...", slot_num))
            );
        });
    }

    // --- Macros: delete ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<MacroBridge>().on_delete_macro(move |slot| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use protocol::binary::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::MACRO_DELETE, &protocol::binary::macro_delete_payload(slot as u8));
                if let Ok(resp) = ser.send_binary(cmd::LIST_MACROS, &[]) {
                    let macros = protocol::parsers::parse_macros_binary(&resp.payload);
                    let _ = tx.send(BgMsg::MacroList(macros));
                }
            });
        });
    }
}
