use crate::context::{AppContext, BgMsg};
use crate::protocol::binary::{self as bp};
use crate::{MainWindow, ToolsBridge};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Shared flag to stop the matrix test polling thread.
static MATRIX_POLLING: AtomicBool = AtomicBool::new(false);

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    setup_toggle_matrix_test(window, ctx);
    setup_nvs_reset(window, ctx);
    setup_nvs_reset_all(window, ctx);
}

fn build_nvs_mask(window: &MainWindow) -> u8 {
    let tb = window.global::<ToolsBridge>();
    let mut mask: u8 = 0;
    if tb.get_nvs_keymaps()   { mask |= 0x01; }
    if tb.get_nvs_macros()    { mask |= 0x02; }
    if tb.get_nvs_stats()     { mask |= 0x04; }
    if tb.get_nvs_features()  { mask |= 0x08; }
    if tb.get_nvs_bluetooth() { mask |= 0x10; }
    if tb.get_nvs_tama()      { mask |= 0x20; }
    mask
}

fn setup_nvs_reset(window: &MainWindow, ctx: &AppContext) {
    let window_weak = window.as_weak();
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();

    window.global::<ToolsBridge>().on_nvs_reset(move || {
        let Some(w) = window_weak.upgrade() else { return };
        let mask = build_nvs_mask(&w);
        if mask == 0 { return; }
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            match ser.send_binary(bp::cmd::NVS_RESET, &[mask]) {
                Ok(_) => {
                    ser.disconnect();
                    let _ = tx.send(BgMsg::Disconnected);
                    let _ = tx.send(BgMsg::NvsResetDone(Ok(mask)));
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::NvsResetDone(Err(e)));
                }
            }
        });
    });
}

fn setup_nvs_reset_all(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();

    window.global::<ToolsBridge>().on_nvs_reset_all(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            match ser.send_binary(bp::cmd::NVS_RESET, &[0xFF]) {
                Ok(_) => {
                    ser.disconnect();
                    let _ = tx.send(BgMsg::Disconnected);
                    let _ = tx.send(BgMsg::NvsResetDone(Ok(0xFF)));
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::NvsResetDone(Err(e)));
                }
            }
        });
    });
}

fn setup_toggle_matrix_test(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();

    window.global::<ToolsBridge>().on_toggle_matrix_test(move || {
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());

            // Send toggle command
            match ser.send_binary(bp::cmd::MATRIX_TEST, &[]) {
                Ok(resp) => {
                    if resp.payload.len() >= 3 {
                        let enabled = resp.payload[0];
                        let rows = resp.payload[1];
                        let cols = resp.payload[2];
                        let _ = tx.send(BgMsg::MatrixTestToggled(enabled != 0, rows, cols));

                        if enabled != 0 {
                            // Start polling thread for unsolicited events
                            MATRIX_POLLING.store(true, Ordering::SeqCst);
                            let serial2 = serial.clone();
                            let tx2 = tx.clone();
                            drop(ser); // release lock before spawning poller
                            std::thread::spawn(move || {
                                poll_matrix_events(serial2, tx2);
                            });
                        } else {
                            MATRIX_POLLING.store(false, Ordering::SeqCst);
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::MatrixTestError(e));
                }
            }
        });
    });
}

/// Poll serial for unsolicited KR [0xB0] events.
fn poll_matrix_events(
    serial: Arc<std::sync::Mutex<crate::protocol::serial::SerialManager>>,
    tx: std::sync::mpsc::Sender<BgMsg>,
) {
    let mut buf = vec![0u8; 256];

    while MATRIX_POLLING.load(Ordering::SeqCst) {
        let read_result = {
            let mut ser = match serial.try_lock() {
                Ok(s) => s,
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
            };
            let port = match ser.port_mut() {
                Some(p) => p,
                None => {
                    MATRIX_POLLING.store(false, Ordering::SeqCst);
                    let _ = tx.send(BgMsg::MatrixTestError("Port disconnected".into()));
                    break;
                }
            };
            port.read(&mut buf)
        };

        match read_result {
            Ok(n) if n > 0 => {
                let frames = bp::parse_all_kr(&buf[..n]);
                for frame in frames {
                    if frame.cmd == bp::cmd::MATRIX_TEST && frame.is_ok() && frame.payload.len() >= 3 {
                        let row = frame.payload[0];
                        let col = frame.payload[1];
                        let state = frame.payload[2];
                        let _ = tx.send(BgMsg::MatrixTestEvent(row, col, state));
                    }
                }
            }
            _ => {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}
