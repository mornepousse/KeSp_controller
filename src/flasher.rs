use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::protocol::serial::SerialManager;
use crate::{FlasherBridge, MainWindow};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;
use std::sync::mpsc;

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    init_prog_ports(window);
    setup_refresh_prog_ports(window);
    setup_browse_firmware(window);
    setup_flash(window, ctx);
}

// Init prog ports list on startup
fn init_prog_ports(window: &MainWindow) {
    let ports = SerialManager::list_prog_ports();
    if let Some(first) = ports.first() {
        window.global::<FlasherBridge>().set_selected_prog_port(SharedString::from(first.as_str()));
    }
    let model: Vec<SharedString> = ports.iter().map(|p| SharedString::from(p.as_str())).collect();
    window.global::<FlasherBridge>().set_prog_ports(
        ModelRc::from(Rc::new(VecModel::from(model)))
    );
}

// --- Flasher: refresh prog ports ---
fn setup_refresh_prog_ports(window: &MainWindow) {
    let window_weak = window.as_weak();

    window.global::<FlasherBridge>().on_refresh_prog_ports(move || {
        let ports = SerialManager::list_prog_ports();
        if let Some(w) = window_weak.upgrade() {
            if let Some(first) = ports.first() {
                w.global::<FlasherBridge>().set_selected_prog_port(SharedString::from(first.as_str()));
            }
            let model: Vec<SharedString> = ports.iter().map(|p| SharedString::from(p.as_str())).collect();
            w.global::<FlasherBridge>().set_prog_ports(
                ModelRc::from(Rc::new(VecModel::from(model)))
            );
        }
    });
}

// --- Flasher: browse firmware ---
fn setup_browse_firmware(window: &MainWindow) {
    let window_weak = window.as_weak();

    window.global::<FlasherBridge>().on_browse_firmware(move || {
        let window_weak = window_weak.clone();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .add_filter("Firmware", &["bin"])
                .pick_file();
            if let Some(path) = file {
                let path_str = path.to_string_lossy().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<FlasherBridge>().set_firmware_path(
                            SharedString::from(path_str.as_str())
                        );
                    }
                });
            }
        });
    });
}

// --- Flasher: flash ---
fn setup_flash(window: &MainWindow, ctx: &AppContext) {
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();

    window.global::<FlasherBridge>().on_flash(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let flasher = w.global::<FlasherBridge>();
        let port = flasher.get_selected_prog_port().to_string();
        let path = flasher.get_firmware_path().to_string();
        let offset: u32 = match flasher.get_flash_offset_index() {
            0 => 0x0,       // full flash
            1 => 0x20000,   // factory
            2 => 0x220000,  // ota_0
            _ => 0x20000,
        };

        if port.is_empty() || path.is_empty() { return; }

        // Read firmware file
        let firmware = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) => {
                let _ = tx.send(BgMsg::FlashDone(Err(format!("Cannot read {}: {}", path, e))));
                return;
            }
        };

        flasher.set_flashing(true);
        flasher.set_flash_progress(0.0);
        flasher.set_flash_status(SharedString::from("Starting..."));

        let tx = tx.clone();
        std::thread::spawn(move || {
            let (ftx, frx) = mpsc::channel();
            // Forward flash progress to main bg channel
            let tx2 = tx.clone();
            let progress_thread = std::thread::spawn(move || {
                while let Ok(protocol::flasher::FlashProgress::OtaProgress(p, msg)) = frx.recv() {
                    let _ = tx2.send(BgMsg::FlashProgress(p, msg));
                }
            });

            let result = protocol::flasher::flash_firmware(&port, &firmware, offset, &ftx);
            drop(ftx); // close channel so progress_thread exits
            let _ = progress_thread.join();
            let _ = tx.send(BgMsg::FlashDone(result.map_err(|e| e.to_string())));
        });
    });
}
