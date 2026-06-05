use crate::context::{AppContext, BgMsg};
use crate::models;
use crate::protocol::layout::{KeycapPos, UNIT_PX};
use crate::{LayoutBridge, MainWindow};
use slint::{ComponentHandle, SharedString};

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    setup_load_from_file(window, ctx);
    setup_load_from_keyboard(window, ctx);
    setup_export_json(window);
    setup_editor(window, ctx);
}

// --- Layout preview: load from file ---
fn setup_load_from_file(window: &MainWindow, ctx: &AppContext) {
    let window_weak = window.as_weak();
    let tx = ctx.bg_tx.clone();
    window.global::<LayoutBridge>().on_load_from_file(move || {
        let window_weak = window_weak.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .pick_file();
            let Some(path) = file else { return };
            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(e) => {
                    let err = format!("Read error: {}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<LayoutBridge>().set_status(SharedString::from(err));
                        }
                    });
                    return;
                }
            };
            let path_str = path.to_string_lossy().to_string();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = window_weak.upgrade() {
                    w.global::<LayoutBridge>().set_file_path(SharedString::from(&path_str));
                }
            });
            // Parsing + buffer fill happen on the UI thread (needs the non-Send
            // editable buffer), so route the JSON through the message channel.
            let _ = tx.send(BgMsg::LayoutPreview(json));
        });
    });
}

// --- Layout preview: load from keyboard ---
fn setup_load_from_keyboard(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();
    window.global::<LayoutBridge>().on_load_from_keyboard(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            match ser.get_layout_json() {
                Ok(json) => {
                    let _ = tx.send(BgMsg::LayoutPreview(json));
                }
                Err(e) => {
                    let err = format!("Error: {}", e);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<LayoutBridge>().set_status(SharedString::from(err));
                        }
                    });
                }
            }
        });
    });
}

// --- Layout preview: export JSON ---
fn setup_export_json(window: &MainWindow) {
    let window_weak = window.as_weak();
    window.global::<LayoutBridge>().on_export_json(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let json = w.global::<LayoutBridge>().get_json_text().to_string();
        if json.is_empty() { return; }
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .set_file_name("layout.json")
                .save_file();
            if let Some(path) = file {
                let msg = match std::fs::write(&path, &json) {
                    Ok(()) => format!("Exported to {}", path.display()),
                    Err(e) => format!("Write error: {}", e),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<LayoutBridge>().set_status(SharedString::from(msg));
                    }
                });
            }
        });
    });
}

// --- Layout preview: numeric position editor ---
fn setup_editor(window: &MainWindow, ctx: &AppContext) {
    let lb = window.global::<LayoutBridge>();

    // Select a key by clicking it in the preview.
    {
        let buffer = ctx.layout_edit.clone();
        let sel = ctx.layout_sel.clone();
        let window_weak = window.as_weak();
        lb.on_select_key(move |idx| {
            let Some(w) = window_weak.upgrade() else { return };
            let len = buffer.borrow().len() as i32;
            let new_sel = if idx >= 0 && idx < len { idx } else { -1 };
            sel.set(new_sel);
            models::rebuild_layout_model(&w, &buffer.borrow(), new_sel);
        });
    }

    // Commit an edited field for the selected key.
    {
        let buffer = ctx.layout_edit.clone();
        let sel = ctx.layout_sel.clone();
        let window_weak = window.as_weak();
        lb.on_update_key(move |field, value| {
            let Some(w) = window_weak.upgrade() else { return };
            let idx = sel.get();
            if idx < 0 { return; }
            {
                let mut keys = buffer.borrow_mut();
                let Some(k) = keys.get_mut(idx as usize) else { return };
                let field = field.as_str();
                match field {
                    "x" | "y" | "rotation" => {
                        let Ok(v) = value.trim().parse::<f32>() else { return };
                        match field {
                            "x" => k.x = v * UNIT_PX,
                            "y" => k.y = v * UNIT_PX,
                            _ => k.angle = v,
                        }
                    }
                    "row" | "col" => {
                        let Ok(v) = value.trim().parse::<usize>() else { return };
                        if field == "row" { k.row = v; } else { k.col = v; }
                    }
                    _ => return,
                }
            }
            w.global::<LayoutBridge>()
                .set_status(SharedString::from("Layout edited — Export to save"));
            models::rebuild_layout_model(&w, &buffer.borrow(), idx);
        });
    }

    // Append a new 1u key at the origin and select it.
    {
        let buffer = ctx.layout_edit.clone();
        let sel = ctx.layout_sel.clone();
        let window_weak = window.as_weak();
        lb.on_add_key(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let new_idx = {
                let mut keys = buffer.borrow_mut();
                keys.push(KeycapPos {
                    row: 0, col: 0,
                    x: 0.0, y: 0.0,
                    w: UNIT_PX, h: UNIT_PX,
                    angle: 0.0,
                });
                keys.len() as i32 - 1
            };
            sel.set(new_idx);
            w.global::<LayoutBridge>()
                .set_status(SharedString::from("Key added — Export to save"));
            models::rebuild_layout_model(&w, &buffer.borrow(), new_idx);
        });
    }

    // Delete the selected key.
    {
        let buffer = ctx.layout_edit.clone();
        let sel = ctx.layout_sel.clone();
        let window_weak = window.as_weak();
        lb.on_delete_key(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let idx = sel.get();
            if idx < 0 { return; }
            {
                let mut keys = buffer.borrow_mut();
                if (idx as usize) >= keys.len() { return; }
                keys.remove(idx as usize);
            }
            sel.set(-1);
            w.global::<LayoutBridge>()
                .set_status(SharedString::from("Key deleted — Export to save"));
            models::rebuild_layout_model(&w, &buffer.borrow(), -1);
        });
    }
}
