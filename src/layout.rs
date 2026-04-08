use crate::context::AppContext;
use crate::models;
use crate::{LayoutBridge, MainWindow};
use slint::{ComponentHandle, SharedString};

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    setup_load_from_file(window);
    setup_load_from_keyboard(window, ctx);
    setup_export_json(window);
}

// --- Layout preview: load from file ---
fn setup_load_from_file(window: &MainWindow) {
    let window_weak = window.as_weak();
    window.global::<LayoutBridge>().on_load_from_file(move || {
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("Layout JSON", &["json"])
                .pick_file();
            let Some(path) = file else { return };
            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(e) => {
                    let err = format!("Read error: {}", e);
                    let _ = slint::invoke_from_event_loop({
                        let window_weak = window_weak.clone();
                        move || {
                            if let Some(w) = window_weak.upgrade() {
                                w.global::<LayoutBridge>().set_status(SharedString::from(err));
                            }
                        }
                    });
                    return;
                }
            };
            let path_str = path.to_string_lossy().to_string();
            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<LayoutBridge>().set_file_path(SharedString::from(&path_str));
                        models::populate_layout_preview(&w, &json);
                    }
                }
            });
        });
    });
}

// --- Layout preview: load from keyboard ---
fn setup_load_from_keyboard(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let window_weak = window.as_weak();
    window.global::<LayoutBridge>().on_load_from_keyboard(move || {
        let serial = serial.clone();
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            let json = match ser.get_layout_json() {
                Ok(j) => j,
                Err(e) => {
                    let err = format!("Error: {}", e);
                    let _ = slint::invoke_from_event_loop({
                        let window_weak = window_weak.clone();
                        move || {
                            if let Some(w) = window_weak.upgrade() {
                                w.global::<LayoutBridge>().set_status(SharedString::from(err));
                            }
                        }
                    });
                    return;
                }
            };
            let _ = slint::invoke_from_event_loop({
                let window_weak = window_weak.clone();
                move || {
                    if let Some(w) = window_weak.upgrade() {
                        models::populate_layout_preview(&w, &json);
                    }
                }
            });
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
