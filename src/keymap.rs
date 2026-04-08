use crate::context::{AppContext, BgMsg};
use crate::protocol;
use crate::{AppState, KeymapBridge, MainWindow};
use slint::{ComponentHandle, Model, SharedString};

/// Wire up key selection, layer switch, layer rename, and heatmap toggle.
pub fn setup(window: &MainWindow, ctx: &AppContext) {
    // --- Key selection callback ---
    {
        let window_weak = window.as_weak();
        window.global::<KeymapBridge>().on_select_key(move |key_index| {
            let Some(w) = window_weak.upgrade() else { return };
            let keycaps = w.global::<KeymapBridge>().get_keycaps();
            let idx = key_index as usize;
            if idx >= keycaps.row_count() { return; }
            for i in 0..keycaps.row_count() {
                let mut item = keycaps.row_data(i).unwrap();
                let should_select = i == idx;
                if item.selected != should_select {
                    item.selected = should_select;
                    keycaps.set_row_data(i, item);
                }
            }
            let bridge = w.global::<KeymapBridge>();
            bridge.set_selected_key_index(key_index);
            let item = keycaps.row_data(idx).unwrap();
            bridge.set_selected_key_label(item.label.clone());
        });
    }

    // --- Layer switch callback ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let current_layer = ctx.current_layer.clone();
        let window_weak = window.as_weak();

        window.global::<KeymapBridge>().on_switch_layer(move |layer_index| {
            let idx = layer_index as usize;
            current_layer.set(idx);

            // Update active flag on the CURRENT model (not a captured stale ref)
            if let Some(w) = window_weak.upgrade() {
                let layers = w.global::<KeymapBridge>().get_layers();
                for i in 0..layers.row_count() {
                    let mut item = layers.row_data(i).unwrap();
                    let should_be_active = item.index == layer_index;
                    if item.active != should_be_active {
                        item.active = should_be_active;
                        layers.set_row_data(i, item);
                    }
                }
                w.global::<KeymapBridge>().set_active_layer(layer_index);
                w.global::<AppState>().set_status_text(SharedString::from(format!("Loading layer {}...", idx)));
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                match ser.get_keymap(idx as u8) {
                    Ok(km) => { let _ = tx.send(BgMsg::Keymap(km)); }
                    Err(e) => { let _ = tx.send(BgMsg::ConnectError(e)); }
                }
            });
        });
    }

    // --- Layer rename callback ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<KeymapBridge>().on_rename_layer(move |layer_idx, new_name| {
            let payload = protocol::binary::set_layout_name_payload(layer_idx as u8, &new_name);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(protocol::binary::cmd::SET_LAYOUT_NAME, &payload);
                if let Ok(names) = ser.get_layer_names() {
                    let _ = tx.send(BgMsg::LayerNames(names));
                }
            });
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text(
                    SharedString::from(format!("Renamed layer {} → {}", layer_idx, new_name))
                );
            }
        });
    }

    // --- Heatmap toggle: auto-load data ---
    {
        let serial = ctx.serial.clone();
        let tx = ctx.bg_tx.clone();

        window.global::<KeymapBridge>().on_toggle_heatmap(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(resp) = ser.send_binary(protocol::binary::cmd::KEYSTATS_BIN, &[]) {
                    let (data, max) = protocol::parsers::parse_keystats_binary(&resp.payload);
                    let _ = tx.send(BgMsg::HeatmapData(data, max));
                }
            });
        });
    }
}
