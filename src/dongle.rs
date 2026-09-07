//! Dongle integration per `CDC_DONGLE_INTEGRATION.md`.
//!
//! Implements the documented dongle command set:
//! - `RF_STATUS` (0xB3) — live link snapshot (poll 1–2 Hz)
//! - `RF_PAIR_LIST` (0xB4) — paired half MACs
//! - `BATTERY` (0xB6) — per-half battery telemetry (0.1–0.2 Hz)
//! - `RF_PAIR_START` (0xB2) + polling — 30 s pairing workflow
//! - `RF_PAIR_RESET` (0xB5) — destructive: purge paired halves
//!
//! All callbacks no-op silently when the connected device isn't a dongle
//! (`SerialManager::is_dongle == false`). Detection itself happens in
//! `SerialManager::connect()` via the FEATURES tag `RF_DONGLE`.

use crate::context::{AppContext, BgMsg};
use crate::protocol::binary;
use crate::protocol::parsers;
use crate::protocol::serial::SerialManager;
use crate::{DongleBridge, MainWindow};
use slint::ComponentHandle;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 30-second pairing window per the firmware spec.
const PAIR_WINDOW_S: u8 = 30;

// =============================================================================
// Pollers (read-only, idempotent)
// =============================================================================

pub fn fetch_rf_status(serial: &mut SerialManager, tx: &mpsc::Sender<BgMsg>) {
    if !serial.connected {
        return;
    }
    match serial.send_binary(binary::cmd::RF_STATUS, &[]) {
        Ok(resp) => match parsers::parse_rf_status(&resp.payload) {
            Ok(s) => {
                let _ = tx.send(BgMsg::RfStatus(s));
            }
            Err(e) => {
                let _ = tx.send(BgMsg::StatusError(format!("RF_STATUS parse: {}", e)));
            }
        },
        Err(_) => {
            // ERR_UNKNOWN on a standalone keyboard is expected — silent.
        }
    }
}

pub fn fetch_pair_list(serial: &mut SerialManager, tx: &mpsc::Sender<BgMsg>) {
    if !serial.connected {
        return;
    }
    if let Ok(resp) = serial.send_binary(binary::cmd::RF_PAIR_LIST, &[]) { match parsers::parse_pair_list(&resp.payload) {
        Ok(p) => {
            let _ = tx.send(BgMsg::RfPairList(p));
        }
        Err(e) => {
            let _ = tx.send(BgMsg::StatusError(format!("RF_PAIR_LIST parse: {}", e)));
        }
    } }
}

pub fn fetch_battery(serial: &mut SerialManager, tx: &mpsc::Sender<BgMsg>) {
    if !serial.connected {
        return;
    }
    if let Ok(resp) = serial.send_binary(binary::cmd::BATTERY, &[]) { match parsers::parse_battery(&resp.payload) {
        Ok(b) => {
            let _ = tx.send(BgMsg::Battery(b));
        }
        Err(e) => {
            let _ = tx.send(BgMsg::StatusError(format!("BATTERY parse: {}", e)));
        }
    } }
}

// =============================================================================
// Pairing workflow
// =============================================================================

/// Open a pairing window and poll RF_PAIR_LIST until a new half is captured
/// or `PAIR_WINDOW_S` seconds elapse. Emits per-second `PairTick` updates and
/// a final `PairResult`. Also forwards each polled pair list as `RfPairList`.
fn run_pairing(
    serial: Arc<Mutex<SerialManager>>,
    tx: mpsc::Sender<BgMsg>,
    reset: bool,
) {
    let baseline_count = {
        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());

        // Snapshot baseline (so we can detect a real increment, not just current state).
        let baseline = match ser.send_binary(binary::cmd::RF_PAIR_LIST, &[]) {
            Ok(resp) => parsers::parse_pair_list(&resp.payload)
                .map(|p| p.paired_count)
                .unwrap_or(0),
            Err(_) => 0,
        };

        // Open the window.
        match ser.send_binary(
            binary::cmd::RF_PAIR_START,
            &binary::rf_pair_start_payload(reset),
        ) {
            Ok(resp) => match parsers::parse_pair_start(&resp.payload) {
                Ok(p) => {
                    let _ = tx.send(BgMsg::StatusMessage(format!(
                        "Pairing window open (set_id={:04X}, paired={})",
                        p.set_id, p.paired_count
                    )));
                    // If a reset happened, baseline becomes what the firmware now reports.
                    if reset { p.paired_count } else { baseline }
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::PairResult(Err(format!("parse: {}", e))));
                    return;
                }
            },
            Err(e) => {
                let _ = tx.send(BgMsg::PairResult(Err(e)));
                return;
            }
        }
    };

    // Poll RF_PAIR_LIST every second; emit countdown ticks; bail on increment.
    for remaining in (0..PAIR_WINDOW_S).rev() {
        let _ = tx.send(BgMsg::PairTick(remaining + 1));
        thread::sleep(Duration::from_secs(1));

        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(resp) = ser.send_binary(binary::cmd::RF_PAIR_LIST, &[]) {
            if let Ok(list) = parsers::parse_pair_list(&resp.payload) {
                let count = list.paired_count;
                let _ = tx.send(BgMsg::RfPairList(list));
                if count > baseline_count {
                    let _ = tx.send(BgMsg::PairResult(Ok(count)));
                    return;
                }
            }
        }
    }
    let _ = tx.send(BgMsg::PairResult(Err(
        "Timeout — no half completed the handshake in 30 s".into(),
    )));
}

fn reset_pairing(serial: &mut SerialManager, tx: &mpsc::Sender<BgMsg>) {
    match serial.send_binary(binary::cmd::RF_PAIR_RESET, &[]) {
        Ok(resp) if resp.is_ok() => {
            let _ = tx.send(BgMsg::StatusMessage("Paired halves cleared".into()));
            // Refresh pair list so the UI shows zeros.
            fetch_pair_list(serial, tx);
        }
        Ok(resp) => {
            let _ = tx.send(BgMsg::StatusError(format!(
                "RF_PAIR_RESET: {}",
                resp.status_name()
            )));
        }
        Err(e) => {
            let _ = tx.send(BgMsg::StatusError(format!("RF_PAIR_RESET: {}", e)));
        }
    }
}

// =============================================================================
// UI Setup
// =============================================================================

pub fn setup(window: &MainWindow, ctx: &AppContext) {
    let serial = ctx.serial.clone();
    let tx = ctx.bg_tx.clone();
    let window_weak = window.as_weak();

    // Refresh all
    {
        let serial = serial.clone();
        let tx = tx.clone();
        let window_weak = window_weak.clone();
        window.global::<DongleBridge>().on_refresh_all(move || {
            let Some(_w) = window_weak.upgrade() else { return };
            let serial = serial.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if !ser.is_dongle {
                    return;
                }
                fetch_rf_status(&mut ser, &tx);
                fetch_pair_list(&mut ser, &tx);
                fetch_battery(&mut ser, &tx);
            });
        });
    }

    // Start pairing (open 30 s window, with reset = false)
    {
        let serial = serial.clone();
        let tx = tx.clone();
        let window_weak = window_weak.clone();
        window.global::<DongleBridge>().on_start_pair(move || {
            let Some(_w) = window_weak.upgrade() else { return };
            let serial = serial.clone();
            let tx = tx.clone();
            thread::spawn(move || run_pairing(serial, tx, false));
        });
    }

    // Reset pairing (wipe NVS)
    {
        let serial = serial.clone();
        let tx = tx.clone();
        let window_weak = window_weak.clone();
        window.global::<DongleBridge>().on_reset_pair(move || {
            let Some(_w) = window_weak.upgrade() else { return };
            let serial = serial.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if !ser.is_dongle {
                    return;
                }
                reset_pairing(&mut ser, &tx);
            });
        });
    }
}
