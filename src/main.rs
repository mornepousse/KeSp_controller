mod logic;

slint::include_modules!();

use logic::keycode;
use logic::layout::KeycapPos;
use logic::serial::SerialManager;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

// Messages from background serial thread to UI
enum BgMsg {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>), // port, fw_version, layer_names, keymap
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    Disconnected,
    TextLines(String, Vec<String>), // tag, lines
    HeatmapData(Vec<Vec<u32>>, u32), // counts, max
    BigramLines(Vec<String>),
    LayoutJson(Vec<KeycapPos>),     // physical key positions from firmware
    FlashProgress(f32, String),  // progress 0-1, status message
    FlashDone(Result<(), String>),
}

fn build_keycap_model(keys: &[KeycapPos]) -> Rc<VecModel<KeycapData>> {
    let keycaps: Vec<KeycapData> = keys
        .iter()
        .enumerate()
        .map(|(idx, kp)| KeycapData {
            x: kp.x,
            y: kp.y,
            w: kp.w,
            h: kp.h,
            rotation: kp.angle,
            rotation_cx: kp.w / 2.0,
            rotation_cy: kp.h / 2.0,
            label: SharedString::from(format!("{},{}", kp.col, kp.row)),
            sublabel: SharedString::default(),
            keycode: 0,
            color: slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a),
            heat: 0.0,
            selected: false,
            index: idx as i32,
        })
        .collect();
    Rc::new(VecModel::from(keycaps))
}

fn build_layer_model(names: &[String]) -> Rc<VecModel<LayerInfo>> {
    let layers: Vec<LayerInfo> = names
        .iter()
        .enumerate()
        .map(|(i, name)| LayerInfo {
            index: i as i32,
            name: SharedString::from(name.as_str()),
            active: i == 0,
        })
        .collect();
    Rc::new(VecModel::from(layers))
}

/// Update keycap labels from keymap data (row x col -> keycode -> label)
fn update_keycap_labels(
    keycap_model: &impl Model<Data = KeycapData>,
    keys: &[KeycapPos],
    keymap: &[Vec<u16>],
    layout: &logic::layout_remap::KeyboardLayout,
) {
    for i in 0..keycap_model.row_count() {
        if i >= keys.len() { break; }
        let mut item = keycap_model.row_data(i).unwrap();
        let kp = &keys[i];
        let row = kp.row as usize;
        let col = kp.col as usize;

        if row < keymap.len() && col < keymap[row].len() {
            let code = keymap[row][col];
            let decoded = keycode::decode_keycode(code);
            let remapped = logic::layout_remap::remap_key_label(layout, &decoded);
            let label = remapped.unwrap_or(&decoded).to_string();
            item.keycode = code as i32;
            item.label = SharedString::from(label);
            item.sublabel = if decoded != format!("0x{:04X}", code) {
                SharedString::default()
            } else {
                SharedString::from(format!("0x{:04X}", code))
            };
        }
        keycap_model.set_row_data(i, item);
    }
}

/// Build the list of all selectable HID keycodes for the key selector
fn build_key_entries() -> Rc<VecModel<KeyEntry>> {
    let mut entries = Vec::new();

    // Letters
    for code in 0x04u16..=0x1D {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Letter"),
        });
    }
    // Numbers
    for code in 0x1Eu16..=0x27 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Number"),
        });
    }
    // Control keys
    for code in [0x28u16, 0x29, 0x2A, 0x2B, 0x2C] {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Control"),
        });
    }
    // Punctuation
    for code in 0x2Du16..=0x38 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Symbol"),
        });
    }
    // F keys
    for code in 0x3Au16..=0x45 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Function"),
        });
    }
    // Navigation
    for code in [0x46u16, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52] {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Navigation"),
        });
    }
    // Modifiers
    for code in 0xE0u16..=0xE7 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Modifier"),
        });
    }
    // Caps Lock
    entries.push(KeyEntry {
        name: SharedString::from("Caps Lock"),
        code: 0x39,
        category: SharedString::from("Control"),
    });
    // Keypad
    for code in 0x53u16..=0x63 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Keypad"),
        });
    }
    // F13-F24
    for code in 0x68u16..=0x73 {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Function"),
        });
    }
    // Media
    for code in [0x7Fu16, 0x80, 0x81] {
        entries.push(KeyEntry {
            name: SharedString::from(keycode::hid_key_name(code as u8)),
            code: code as i32,
            category: SharedString::from("Media"),
        });
    }
    // BT keys
    for (code, name) in [
        (0x2900u16, "BT Next"), (0x2A00, "BT Prev"), (0x2B00, "BT Pair"),
        (0x2C00, "BT Disc"), (0x2E00, "USB/BT"), (0x2F00, "BT On/Off"),
    ] {
        entries.push(KeyEntry {
            name: SharedString::from(name),
            code: code as i32,
            category: SharedString::from("Bluetooth"),
        });
    }
    // Tap Dance 0..7
    for i in 0u16..=7 {
        let code = (0x60 | i) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("TD {}", i)),
            code: code as i32,
            category: SharedString::from("Tap Dance"),
        });
    }
    // Macro 0..9
    for i in 0u16..=9 {
        let code = (i + 0x15) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("M{}", i)),
            code: code as i32,
            category: SharedString::from("Macro"),
        });
    }
    // OSL 0..9
    for i in 0u16..=9 {
        entries.push(KeyEntry {
            name: SharedString::from(format!("OSL {}", i)),
            code: 0x3100 + i as i32,
            category: SharedString::from("Layer"),
        });
    }
    // Layer: MO 0..9
    for layer in 0u16..=9 {
        let code = (layer + 1) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("MO {}", layer)),
            code: code as i32,
            category: SharedString::from("Layer"),
        });
    }
    // Layer: TO 0..9
    for layer in 0u16..=9 {
        let code = (layer + 0x0B) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("TO {}", layer)),
            code: code as i32,
            category: SharedString::from("Layer"),
        });
    }
    // Special KaSe firmware keys
    for (code, name) in [
        (0x3200u16, "Caps Word"),
        (0x3300, "Repeat"),
        (0x3400, "Leader"),
        (0x3900, "GEsc"),
        (0x3A00, "Layer Lock"),
        (0x3C00, "AS Toggle"),
    ] {
        entries.push(KeyEntry {
            name: SharedString::from(name),
            code: code as i32,
            category: SharedString::from("Special"),
        });
    }
    // None
    entries.insert(0, KeyEntry {
        name: SharedString::from("None"),
        code: 0,
        category: SharedString::from("Special"),
    });

    Rc::new(VecModel::from(entries))
}

fn populate_key_categories(window: &MainWindow, all_keys: &VecModel<KeyEntry>, search: &str) {
    let search_lower = search.to_lowercase();
    let filter = |cat: &str| -> Vec<KeyEntry> {
        (0..all_keys.row_count())
            .filter_map(|i| {
                let e = all_keys.row_data(i).unwrap();
                let cat_match = e.category.as_str() == cat
                    || (cat == "Navigation" && (e.category.as_str() == "Control" || e.category.as_str() == "Navigation"))
                    || (cat == "Special" && (e.category.as_str() == "Special" || e.category.as_str() == "Bluetooth" || e.category.as_str() == "Media"))
                    || (cat == "TDMacro" && (e.category.as_str() == "Tap Dance" || e.category.as_str() == "Macro"));
                let search_match = search_lower.is_empty()
                    || e.name.to_lowercase().contains(&search_lower)
                    || e.category.to_lowercase().contains(&search_lower);
                if cat_match && search_match { Some(e) } else { None }
            })
            .collect()
    };
    let set = |model: Vec<KeyEntry>| ModelRc::from(Rc::new(VecModel::from(model)));
    let ks = window.global::<KeySelectorBridge>();
    ks.set_cat_letters(set(filter("Letter")));
    ks.set_cat_numbers(set(filter("Number")));
    ks.set_cat_modifiers(set(filter("Modifier")));
    ks.set_cat_nav(set(filter("Navigation")));
    ks.set_cat_function(set(filter("Function")));
    ks.set_cat_symbols(set(filter("Symbol")));
    ks.set_cat_layers(set(filter("Layer")));
    ks.set_cat_special(set(filter("Special")));
    ks.set_cat_td_macro(set(filter("TDMacro")));
}

/// Map ComboBox index [None,Ctrl,Shift,Alt,GUI,RCtrl,RShift,RAlt,RGUI] to HID mod byte
fn mod_idx_to_byte(idx: i32) -> u8 {
    match idx {
        1 => 0x01, // Ctrl
        2 => 0x02, // Shift
        3 => 0x04, // Alt
        4 => 0x08, // GUI
        5 => 0x10, // RCtrl
        6 => 0x20, // RShift
        7 => 0x40, // RAlt
        8 => 0x80, // RGUI
        _ => 0x00, // None
    }
}

fn main() {
    let keys = logic::layout::default_layout();
    let keys_arc: Rc<std::cell::RefCell<Vec<KeycapPos>>> = Rc::new(std::cell::RefCell::new(keys.clone()));

    let window = MainWindow::new().unwrap();

    // Set up initial keymap models
    let keymap_bridge = window.global::<KeymapBridge>();
    keymap_bridge.set_keycaps(ModelRc::from(build_keycap_model(&keys)));
    keymap_bridge.set_layers(ModelRc::from(build_layer_model(&[
        "Layer 0".into(), "Layer 1".into(), "Layer 2".into(), "Layer 3".into(),
    ])));
    // Compute initial content bounds
    {
        let mut max_x: f32 = 0.0;
        let mut max_y: f32 = 0.0;
        for kp in &keys {
            let right = kp.x + kp.w;
            let bottom = kp.y + kp.h;
            if right > max_x { max_x = right; }
            if bottom > max_y { max_y = bottom; }
        }
        keymap_bridge.set_content_width(max_x);
        keymap_bridge.set_content_height(max_y);
    }

    // Set up settings bridge
    {
        let layouts: Vec<SharedString> = logic::layout_remap::KeyboardLayout::all()
            .iter()
            .map(|l| SharedString::from(l.name()))
            .collect();
        let layout_model = Rc::new(VecModel::from(layouts));
        window.global::<SettingsBridge>().set_available_layouts(ModelRc::from(layout_model));
    }

    // Set up key selector
    let all_keys = build_key_entries();
    window.global::<KeySelectorBridge>().set_all_keys(ModelRc::from(all_keys.clone()));
    populate_key_categories(&window, &all_keys, "");

    // Serial manager shared between threads
    let serial: Arc<Mutex<SerialManager>> = Arc::new(Mutex::new(SerialManager::new()));
    let (bg_tx, bg_rx) = mpsc::channel::<BgMsg>();

    // Current state
    let current_keymap: Rc<std::cell::RefCell<Vec<Vec<u16>>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let current_layer: Rc<std::cell::Cell<usize>> = Rc::new(std::cell::Cell::new(0));
    let saved_settings = logic::settings::load();
    let keyboard_layout = Rc::new(std::cell::RefCell::new(
        logic::layout_remap::KeyboardLayout::from_name(&saved_settings.keyboard_layout),
    ));

    // Set initial layout index
    {
        let all_layouts = logic::layout_remap::KeyboardLayout::all();
        let current = *keyboard_layout.borrow();
        let idx = all_layouts.iter().position(|l| *l == current).unwrap_or(0);
        window.global::<SettingsBridge>().set_selected_layout_index(idx as i32);
    }

    // Heatmap data (for stats)
    let heatmap_data: Rc<std::cell::RefCell<Vec<Vec<u32>>>> = Rc::new(std::cell::RefCell::new(Vec::new()));

    // --- Auto-connect on startup ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AppState>().set_status_text("Scanning ports...".into());
        window.global::<AppState>().set_connection(ConnectionState::Connecting);

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
            match ser.auto_connect() {
                Ok(port_name) => {
                    let fw = ser.get_firmware_version().unwrap_or_default();
                    let names = ser.get_layer_names().unwrap_or_default();
                    let km = ser.get_keymap(0).unwrap_or_default();
                    let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
                    // Fetch physical layout from firmware
                    match ser.get_layout_json() {
                        Ok(json) => {
                            match logic::layout::parse_json(&json) {
                                Ok(keys) => { let _ = tx.send(BgMsg::LayoutJson(keys)); }
                                Err(e) => eprintln!("Layout parse error: {}", e),
                            }
                        }
                        Err(e) => eprintln!("get_layout_json error: {}", e),
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::ConnectError(e));
                }
            }
        });
    }

    // --- Key selection callback ---
    {
        let window_weak = window.as_weak();
        keymap_bridge.on_select_key(move |key_index| {
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
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let current_layer = current_layer.clone();
        let window_weak = window.as_weak();

        keymap_bridge.on_switch_layer(move |layer_index| {
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
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        keymap_bridge.on_rename_layer(move |layer_idx, new_name| {
            let cmd = logic::protocol::cmd_set_layer_name(layer_idx as u8, &new_name);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
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

    // --- Connect/Disconnect callbacks ---
    {
        let serial_c = serial.clone();
        let tx_c = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_connect(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Scanning ports...".into());
                w.global::<AppState>().set_connection(ConnectionState::Connecting);
            }
            let serial = serial_c.clone();
            let tx = tx_c.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                match ser.auto_connect() {
                    Ok(port_name) => {
                        let fw = ser.get_firmware_version().unwrap_or_default();
                        let names = ser.get_layer_names().unwrap_or_default();
                        let km = ser.get_keymap(0).unwrap_or_default();
                        let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
                        match ser.get_layout_json() {
                            Ok(json) => {
                                match logic::layout::parse_json(&json) {
                                    Ok(keys) => { let _ = tx.send(BgMsg::LayoutJson(keys)); }
                                    Err(e) => eprintln!("Layout parse error: {}", e),
                                }
                            }
                            Err(e) => eprintln!("get_layout_json error: {}", e),
                        }
                    }
                    Err(e) => { let _ = tx.send(BgMsg::ConnectError(e)); }
                }
            });
        });
    }

    {
        let serial_d = serial.clone();
        let tx_d = bg_tx.clone();
        window.global::<ConnectionBridge>().on_disconnect(move || {
            let mut ser = serial_d.lock().unwrap_or_else(|e| e.into_inner());
            ser.disconnect();
            let _ = tx_d.send(BgMsg::Disconnected);
        });
    }

    window.global::<ConnectionBridge>().on_refresh_ports(|| {});

    // --- Settings: change layout ---
    {
        let keyboard_layout = keyboard_layout.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let window_weak = window.as_weak();

        window.global::<SettingsBridge>().on_change_layout(move |idx| {
            let all_layouts = logic::layout_remap::KeyboardLayout::all();
            let idx = idx as usize;
            if idx >= all_layouts.len() { return; }

            let new_layout = all_layouts[idx];
            *keyboard_layout.borrow_mut() = new_layout;

            let settings = logic::settings::Settings {
                keyboard_layout: new_layout.name().to_string(),
            };
            logic::settings::save(&settings);

            let km = current_keymap.borrow();
            let keys = keys_arc.borrow();
            if let Some(w) = window_weak.upgrade() {
                if !km.is_empty() {
                    let keycaps = w.global::<KeymapBridge>().get_keycaps();
                    update_keycap_labels(&keycaps, &keys, &km, &new_layout);
                }
            }

            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text(
                    SharedString::from(format!("Layout: {}", new_layout.name()))
                );
            }
        });
    }

    // --- Key selector: filter ---
    {
        let all_keys = all_keys.clone();
        let window_weak = window.as_weak();

        window.global::<KeySelectorBridge>().on_apply_filter(move |search| {
            if let Some(w) = window_weak.upgrade() {
                populate_key_categories(&w, &all_keys, &search);
            }
        });
    }

    // --- Key selector: shared apply logic ---
    // Wraps keycode application in a closure shared by all key selector actions.
    let apply_keycode = {
        let serial = serial.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let current_layer = current_layer.clone();
        let keyboard_layout = keyboard_layout.clone();
        let window_weak = window.as_weak();

        Rc::new(move |code: u16| {
            let Some(w) = window_weak.upgrade() else { return };
            let key_idx = w.global::<KeymapBridge>().get_selected_key_index();
            if key_idx < 0 { return; }
            let key_idx = key_idx as usize;
            let keys = keys_arc.borrow();
            if key_idx >= keys.len() { return; }

            let kp = &keys[key_idx];
            let row = kp.row as usize;
            let col = kp.col as usize;
            drop(keys);
            let layer = current_layer.get() as u8;

            {
                let mut km = current_keymap.borrow_mut();
                if row < km.len() && col < km[row].len() {
                    km[row][col] = code;
                }
            }

            // Clone out of RefCells to avoid holding borrows across bridge calls
            let layout = *keyboard_layout.borrow();
            let km = current_keymap.borrow().clone();
            let keys = keys_arc.borrow().clone();
            let keycaps = w.global::<KeymapBridge>().get_keycaps();
            update_keycap_labels(&keycaps, &keys, &km, &layout);

            let cmd = logic::protocol::cmd_set_key(layer, row as u8, col as u8, code);
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
            });

            w.global::<AppState>().set_status_text(
                SharedString::from(format!("[{},{}] = 0x{:04X}", row, col, code))
            );
        })
    };

    // Macro steps state (needed by dispatch_keycode)
    let macro_steps: Rc<std::cell::RefCell<Vec<(u8, u8)>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let refresh_macro_display = {
        let macro_steps = macro_steps.clone();
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
    };

    // Dispatch key selection based on target
    let dispatch_keycode = {
        let apply_keycode = apply_keycode.clone();
        let keys_arc = keys_arc.clone();
        let serial = serial.clone();
        let macro_steps = macro_steps.clone();
        let refresh_macro_display = refresh_macro_display.clone();
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
                    // code = key index from the mini keyboard in the popup
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
                        // Update model in place
                        let tds = adv.get_tap_dances();
                        for i in 0..tds.row_count() {
                            let td = tds.row_data(i).unwrap();
                            if td.index == td_idx {
                                let actions = td.actions;
                                let mut a = actions.row_data(slot).unwrap();
                                a.name = name.clone();
                                a.code = code as i32;
                                actions.set_row_data(slot, a);

                                // Collect all 4 action codes and send to firmware
                                let mut codes = [0u16; 4];
                                for j in 0..4.min(actions.row_count()) {
                                    codes[j] = actions.row_data(j).unwrap().code as u16;
                                }
                                let payload = logic::binary_protocol::td_set_payload(td_idx as u8, &codes);
                                let serial = serial.clone();
                                std::thread::spawn(move || {
                                    let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                                    let _ = ser.send_binary(logic::binary_protocol::cmd::TD_SET, &payload);
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
                    // Add key press (Down + Up) to macro steps
                    let mut steps = macro_steps.borrow_mut();
                    steps.push((code as u8, 0x00)); // D(key)
                    drop(steps);
                    refresh_macro_display();
                }
                _ => { apply_keycode(code); }
            }
        })
    };

    // Key from list
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_select_keycode(move |code| {
            dispatch(code as u16);
        });
    }

    // Hex input
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_hex(move |hex_str| {
            if let Ok(code) = u16::from_str_radix(hex_str.trim(), 16) {
                dispatch(code);
            }
        });
    }

    // Hex preview: decode keycode and show human-readable name
    {
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_preview_hex(move |hex_str| {
            let preview = u16::from_str_radix(hex_str.trim(), 16)
                .map(|code| keycode::decode_keycode(code))
                .unwrap_or_default();
            if let Some(w) = window_weak.upgrade() {
                w.global::<KeySelectorBridge>().set_hex_preview(SharedString::from(preview));
            }
        });
    }

    // MT builder: mod_combo_index maps to modifier nibble, key_combo_index maps to HID code
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_mt(move |mod_idx, key_idx| {
            let mod_nibble: u16 = match mod_idx {
                0 => 0x01, // Ctrl
                1 => 0x02, // Shift
                2 => 0x04, // Alt
                3 => 0x08, // GUI
                4 => 0x10, // RCtrl
                5 => 0x20, // RShift
                6 => 0x40, // RAlt
                7 => 0x80, // RGUI
                _ => 0x02,
            };
            // ComboBox order: A-Z (0-25), 1-0 (26-35), Space(36), Enter(37), Esc(38), Tab(39), Bksp(40)
            let hid: u16 = match key_idx {
                0..=25 => 0x04 + key_idx as u16,    // A-Z
                26..=35 => 0x1E + (key_idx - 26) as u16, // 1-0
                36 => 0x2C, // Space
                37 => 0x28, // Enter
                38 => 0x29, // Esc
                39 => 0x2B, // Tab
                40 => 0x2A, // Backspace
                _ => 0x04,
            };
            let code = 0x5000 | (mod_nibble << 8) | hid;
            dispatch(code);
        });
    }

    // LT builder: layer_combo_index = layer (0-9), key_combo_index maps to HID code
    {
        let dispatch = dispatch_keycode.clone();
        window.global::<KeySelectorBridge>().on_apply_lt(move |layer_idx, key_idx| {
            let layer = (layer_idx as u16) & 0x0F;
            // ComboBox order: Space(0), Enter(1), Esc(2), Bksp(3), Tab(4), A-E(5-9)
            let hid: u16 = match key_idx {
                0 => 0x2C, // Space
                1 => 0x28, // Enter
                2 => 0x29, // Esc
                3 => 0x2A, // Backspace
                4 => 0x2B, // Tab
                5..=9 => 0x04 + (key_idx - 5) as u16, // A-E
                _ => 0x2C,
            };
            let code = 0x4000 | (layer << 8) | hid;
            dispatch(code);
        });
    }

    // --- Stats: refresh ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<StatsBridge>().on_refresh_stats(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let lines = ser.query_command("KEYSTATS?").unwrap_or_default();
                let (data, max) = logic::parsers::parse_heatmap_lines(&lines);
                let _ = tx.send(BgMsg::HeatmapData(data, max));
                // Also fetch bigrams
                let bigram_lines = ser.query_command("BIGRAMS?").unwrap_or_default();
                let _ = tx.send(BgMsg::BigramLines(bigram_lines));
            });
        });
    }

    // --- Advanced: refresh ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_refresh_advanced(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                // Use binary protocol if v2, text fallback otherwise
                let queries: &[(&str, &str)] = if ser.v2 {
                    &[("td", "TD?"), ("combo", "COMBO?"), ("leader", "LEADER?"), ("ko", "KO?"), ("bt", "BT?")]
                } else {
                    &[("td", "TD?"), ("combo", "COMBO?"), ("leader", "LEADER?"), ("ko", "KO?"), ("bt", "BT?")]
                };
                for (tag, cmd) in queries {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let lines = ser.query_command(cmd).unwrap_or_default();
                    let _ = tx.send(BgMsg::TextLines((*tag).into(), lines));
                }
            });
        });
    }

    // --- Advanced: delete combo ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_combo(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            let payload = vec![idx as u8];
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::COMBO_DELETE, &payload);
                let lines = ser.query_command("COMBO?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("combo".into(), lines));
            });
        });
    }

    // --- Advanced: delete leader ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_leader(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            let payload = vec![idx as u8];
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::LEADER_DELETE, &payload);
                let lines = ser.query_command("LEADER?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("leader".into(), lines));
            });
        });
    }

    // --- Advanced: delete KO ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_delete_ko(move |idx| {
            let serial = serial.clone();
            let tx = tx.clone();
            let payload = vec![idx as u8];
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::KO_DELETE, &payload);
                let lines = ser.query_command("KO?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("ko".into(), lines));
            });
        });
    }

    // --- Advanced: set trilayer ---
    {
        let serial = serial.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_set_trilayer(move |l1, l2, l3| {
            let l1 = l1 as u8;
            let l2 = l2 as u8;
            let l3 = l3 as u8;
            let cmd = logic::protocol::cmd_trilayer(l1, l2, l3);
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
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
        let serial = serial.clone();

        window.global::<AdvancedBridge>().on_bt_switch(move |slot| {
            let cmd = logic::protocol::cmd_bt_switch(slot as u8);
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
            });
        });
    }

    // --- Advanced: TAMA action ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_tama_action(move |action| {
            let cmd = match action.as_str() {
                "feed" => "TAMA FEED",
                "play" => "TAMA PLAY",
                "sleep" => "TAMA SLEEP",
                "meds" => "TAMA MEDS",
                "toggle" => "TAMA TOGGLE",
                _ => return,
            };
            let serial = serial.clone();
            let tx = tx.clone();
            let cmd = cmd.to_string();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
                let lines = ser.query_command("TAMA?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("tama".into(), lines));
            });
        });
    }

    // --- Advanced: toggle autoshift ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_toggle_autoshift(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command("AUTOSHIFT TOGGLE");
                let lines = ser.query_command("AUTOSHIFT?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("autoshift".into(), lines));
            });
        });
    }

    // --- Advanced: create combo ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
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
            let payload = logic::binary_protocol::combo_set_payload(255, r1, c1, r2, c2, result);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::COMBO_SET, &payload);
                let lines = ser.query_command("COMBO?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("combo".into(), lines));
            });
            w.global::<AppState>().set_status_text("Creating combo...".into());
        });
    }

    // --- Advanced: create KO ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_create_ko(move |trig_code, trig_mod_idx, result_code, res_mod_idx| {
            let trig = trig_code as u8;
            let trig_mod = mod_idx_to_byte(trig_mod_idx);
            let result = result_code as u8;
            let res_mod = mod_idx_to_byte(res_mod_idx);
            let payload = logic::binary_protocol::ko_set_payload(255, trig, trig_mod, result, res_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::KO_SET, &payload);
                let lines = ser.query_command("KO?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("ko".into(), lines));
            });
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Creating key override...".into());
            }
        });
    }

    // --- Advanced: create leader ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
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
            let payload = logic::binary_protocol::leader_set_payload(255, &sequence, result, result_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::LEADER_SET, &payload);
                let lines = ser.query_command("LEADER?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("leader".into(), lines));
            });
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Creating leader key...".into());
            }
        });
    }

    // --- Macros: refresh ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<MacroBridge>().on_refresh_macros(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let lines = ser.query_command("MACROS?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("macros".into(), lines));
            });
        });
    }

    // --- Macros: add delay step ---
    {
        let macro_steps = macro_steps.clone();
        let refresh = refresh_macro_display.clone();

        window.global::<MacroBridge>().on_add_delay_step(move |ms| {
            let units = (ms as u8) / 10;
            macro_steps.borrow_mut().push((0xFF, units));
            refresh();
        });
    }

    // --- Macros: remove last step ---
    {
        let macro_steps = macro_steps.clone();
        let refresh = refresh_macro_display.clone();

        window.global::<MacroBridge>().on_remove_last_step(move || {
            macro_steps.borrow_mut().pop();
            refresh();
        });
    }

    // --- Macros: clear steps ---
    {
        let macro_steps = macro_steps.clone();
        let refresh = refresh_macro_display.clone();

        window.global::<MacroBridge>().on_clear_steps(move || {
            macro_steps.borrow_mut().clear();
            refresh();
        });
    }

    // --- Macros: save ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();

        window.global::<MacroBridge>().on_save_macro(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let mb = w.global::<MacroBridge>();
            let slot_num = mb.get_new_slot_idx() as u8;
            let name = mb.get_new_name().to_string();
            let steps = macro_steps.borrow();
            let steps_str: Vec<String> = steps.iter().map(|&(kc, md)| {
                if kc == 0xFF { format!("{:02X}:{:02X}", kc, md) }
                else { format!("{:02X}:{:02X}", kc, md) }
            }).collect();
            let steps_text = steps_str.join(",");
            drop(steps);
            let cmd = logic::protocol::cmd_macroseq(slot_num, &name, &steps_text);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
                let lines = ser.query_command("MACROS?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("macros".into(), lines));
            });
            w.global::<AppState>().set_status_text(
                SharedString::from(format!("Saving macro #{}...", slot_num))
            );
        });
    }

    // --- Macros: delete ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<MacroBridge>().on_delete_macro(move |slot| {
            let cmd = logic::protocol::cmd_macro_del(slot as u8);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_command(&cmd);
                let lines = ser.query_command("MACROS?").unwrap_or_default();
                let _ = tx.send(BgMsg::TextLines("macros".into(), lines));
            });
        });
    }

    // --- Flasher: refresh prog ports ---
    {
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
    {
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
    {
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<FlasherBridge>().on_flash(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let flasher = w.global::<FlasherBridge>();
            let port = flasher.get_selected_prog_port().to_string();
            let path = flasher.get_firmware_path().to_string();
            let offset: u32 = match flasher.get_flash_offset_index() {
                0 => 0x20000,   // factory
                1 => 0x220000,  // ota_0
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
                    while let Ok(logic::flasher::FlashProgress::OtaProgress(p, msg)) = frx.recv() {
                        let _ = tx2.send(BgMsg::FlashProgress(p, msg));
                    }
                });

                let result = logic::flasher::flash_firmware(&port, &firmware, offset, &ftx);
                drop(ftx); // close channel so progress_thread exits
                let _ = progress_thread.join();
                let _ = tx.send(BgMsg::FlashDone(result.map_err(|e| e.to_string())));
            });
        });
    }

    // Init prog ports list
    {
        let ports = SerialManager::list_prog_ports();
        if let Some(first) = ports.first() {
            window.global::<FlasherBridge>().set_selected_prog_port(SharedString::from(first.as_str()));
        }
        let model: Vec<SharedString> = ports.iter().map(|p| SharedString::from(p.as_str())).collect();
        window.global::<FlasherBridge>().set_prog_ports(
            ModelRc::from(Rc::new(VecModel::from(model)))
        );
    }

    // --- Poll background messages via timer ---
    {
        let window_weak = window.as_weak();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let keyboard_layout = keyboard_layout.clone();
        let heatmap_data = heatmap_data.clone();

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(50),
            move || {
                let Some(window) = window_weak.upgrade() else { return };

                while let Ok(msg) = bg_rx.try_recv() {
                    match msg {
                        BgMsg::Connected(port, fw, names, km) => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Connected);
                            app.set_firmware_version(SharedString::from(&fw));
                            app.set_status_text(SharedString::from(format!("Connected to {}", port)));

                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));

                            *current_keymap.borrow_mut() = km.clone();
                            let keycaps = window.global::<KeymapBridge>().get_keycaps();
                            let layout = keyboard_layout.borrow();
                            let keys = keys_arc.borrow();
                            update_keycap_labels(&keycaps, &keys, &km, &layout);
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
                            update_keycap_labels(&keycaps, &keys, &km, &layout);
                            window.global::<AppState>().set_status_text("Keymap loaded".into());
                        }
                        BgMsg::LayerNames(names) => {
                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));
                        }
                        BgMsg::Disconnected => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Disconnected);
                            app.set_firmware_version(SharedString::default());
                            app.set_status_text("Disconnected".into());
                        }
                        BgMsg::LayoutJson(new_keys) => {
                            *keys_arc.borrow_mut() = new_keys.clone();
                            let new_model = build_keycap_model(&new_keys);
                            let km = current_keymap.borrow();
                            if !km.is_empty() {
                                let layout = keyboard_layout.borrow();
                                update_keycap_labels(&new_model, &new_keys, &km, &layout);
                            }
                            // Compute content bounds for responsive scaling
                            let mut max_x: f32 = 0.0;
                            let mut max_y: f32 = 0.0;
                            for kp in &new_keys {
                                let right = kp.x + kp.w;
                                let bottom = kp.y + kp.h;
                                if right > max_x { max_x = right; }
                                if bottom > max_y { max_y = bottom; }
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
                            let entries = logic::stats_analyzer::parse_bigram_lines(&lines);
                            let analysis = logic::stats_analyzer::analyze_bigrams(&entries);
                            window.global::<StatsBridge>().set_bigrams(BigramData {
                                alt_hand_pct: analysis.alt_hand_pct,
                                same_hand_pct: analysis.same_hand_pct,
                                sfb_pct: analysis.sfb_pct,
                                total: analysis.total as i32,
                            });
                        }
                        BgMsg::FlashProgress(progress, msg) => {
                            let flasher = window.global::<FlasherBridge>();
                            flasher.set_flash_progress(progress);
                            flasher.set_flash_status(SharedString::from(msg));
                        }
                        BgMsg::FlashDone(result) => {
                            let flasher = window.global::<FlasherBridge>();
                            flasher.set_flashing(false);
                            match result {
                                Ok(()) => {
                                    flasher.set_flash_progress(1.0);
                                    flasher.set_flash_status(SharedString::from("Flash complete!"));
                                    window.global::<AppState>().set_status_text("Flash complete!".into());
                                }
                                Err(e) => {
                                    flasher.set_flash_status(SharedString::from(format!("Error: {}", e)));
                                    window.global::<AppState>().set_status_text(
                                        SharedString::from(format!("Flash error: {}", e))
                                    );
                                }
                            }
                        }
                        BgMsg::HeatmapData(data, max) => {
                            *heatmap_data.borrow_mut() = data.clone();

                            // Update heat intensity on keycaps
                            let keycaps = window.global::<KeymapBridge>().get_keycaps();
                            let keys = keys_arc.borrow();
                            for i in 0..keycaps.row_count() {
                                if i >= keys.len() { break; }
                                let mut item = keycaps.row_data(i).unwrap();
                                let kp = &keys[i];
                                let row = kp.row as usize;
                                let col = kp.col as usize;
                                let count = data.get(row)
                                    .and_then(|r| r.get(col))
                                    .copied()
                                    .unwrap_or(0);
                                item.heat = if max > 0 { count as f32 / max as f32 } else { 0.0 };
                                keycaps.set_row_data(i, item);
                            }
                            drop(keys);

                            let km = current_keymap.borrow();
                            let balance = logic::stats_analyzer::hand_balance(&data);
                            let fingers = logic::stats_analyzer::finger_load(&data);
                            let rows = logic::stats_analyzer::row_usage(&data);
                            let top = logic::stats_analyzer::top_keys(&data, &km, 10);
                            let dead = logic::stats_analyzer::dead_keys(&data, &km);

                            let stats = window.global::<StatsBridge>();
                            stats.set_hand_balance(HandBalanceData {
                                left_pct: balance.left_pct,
                                right_pct: balance.right_pct,
                                total: balance.total as i32,
                            });
                            stats.set_total_presses(balance.total as i32);

                            let finger_model: Vec<FingerLoadData> = fingers.iter().map(|f| FingerLoadData {
                                name: SharedString::from(&f.name),
                                pct: f.pct,
                                count: f.count as i32,
                            }).collect();
                            stats.set_finger_load(ModelRc::from(Rc::new(VecModel::from(finger_model))));

                            let row_model: Vec<RowUsageData> = rows.iter().map(|r| RowUsageData {
                                name: SharedString::from(&r.name),
                                pct: r.pct,
                                count: r.count as i32,
                            }).collect();
                            stats.set_row_usage(ModelRc::from(Rc::new(VecModel::from(row_model))));

                            let top_model: Vec<TopKeyData> = top.iter().map(|t| TopKeyData {
                                name: SharedString::from(&t.name),
                                finger: SharedString::from(&t.finger),
                                count: t.count as i32,
                                pct: t.pct,
                            }).collect();
                            stats.set_top_keys(ModelRc::from(Rc::new(VecModel::from(top_model))));

                            let dead_model: Vec<SharedString> = dead.iter().map(|d| SharedString::from(d.as_str())).collect();
                            stats.set_dead_keys(ModelRc::from(Rc::new(VecModel::from(dead_model))));

                            window.global::<AppState>().set_status_text(
                                SharedString::from(format!("Stats loaded ({} total presses, max {})", balance.total, max))
                            );
                        }
                        BgMsg::TextLines(tag, lines) => {
                            match tag.as_str() {
                                "td" => {
                                    let td_data = logic::parsers::parse_td_lines(&lines);
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
                                        })
                                        .collect();
                                    window.global::<AdvancedBridge>().set_tap_dances(
                                        ModelRc::from(Rc::new(VecModel::from(model)))
                                    );
                                }
                                "combo" => {
                                    let combo_data = logic::parsers::parse_combo_lines(&lines);
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
                                "leader" => {
                                    let leader_data = logic::parsers::parse_leader_lines(&lines);
                                    let model: Vec<LeaderData> = leader_data.iter().map(|l| {
                                        let seq: Vec<String> = l.sequence.iter()
                                            .map(|&k| keycode::hid_key_name(k))
                                            .collect();
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
                                "ko" => {
                                    let ko_data = logic::parsers::parse_ko_lines(&lines);
                                    let model: Vec<KeyOverrideData> = ko_data.iter().enumerate().map(|(i, ko)| {
                                        KeyOverrideData {
                                            index: i as i32,
                                            trigger: SharedString::from(keycode::hid_key_name(ko[0])),
                                            result: SharedString::from(keycode::hid_key_name(ko[2])),
                                        }
                                    }).collect();
                                    window.global::<AdvancedBridge>().set_key_overrides(
                                        ModelRc::from(Rc::new(VecModel::from(model)))
                                    );
                                }
                                "bt" => {
                                    let bt_text = lines.join("\n");
                                    window.global::<AdvancedBridge>().set_bt_status(SharedString::from(bt_text));
                                }
                                "macros" => {
                                    let macro_data = logic::parsers::parse_macro_lines(&lines);
                                    let model: Vec<MacroData> = macro_data.iter().map(|m| {
                                        let steps_str: Vec<String> = m.steps.iter().map(|s| {
                                            if s.is_delay() {
                                                format!("T({})", s.delay_ms())
                                            } else {
                                                format!("D({:02X})", s.keycode)
                                            }
                                        }).collect();
                                        MacroData {
                                            slot: m.slot as i32,
                                            name: SharedString::from(&m.name),
                                            steps: SharedString::from(steps_str.join(" ")),
                                        }
                                    }).collect();
                                    window.global::<MacroBridge>().set_macros(
                                        ModelRc::from(Rc::new(VecModel::from(model)))
                                    );
                                }
                                "wpm" => {
                                    if let Some(line) = lines.first() {
                                        let wpm: u16 = line.split_whitespace()
                                            .last()
                                            .and_then(|s| s.parse().ok())
                                            .unwrap_or(0);
                                        window.global::<AppState>().set_wpm(wpm as i32);
                                    }
                                }
                                "tama" => {
                                    let text = lines.join("\n");
                                    window.global::<AdvancedBridge>().set_tama_status(SharedString::from(text));
                                }
                                "autoshift" => {
                                    let text = lines.join(" ");
                                    window.global::<AdvancedBridge>().set_autoshift_status(SharedString::from(text));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            },
        );

        // WPM polling timer (5s, non-blocking try_lock in background thread)
        let wpm_timer = slint::Timer::default();
        {
            let serial = serial.clone();
            let tx = bg_tx.clone();
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
                        let lines = ser.query_command("WPM?").unwrap_or_default();
                        drop(ser);
                        let _ = tx.send(BgMsg::TextLines("wpm".into(), lines));
                    });
                },
            );
        }

        // Keep timers alive
        let _keep_timer = timer;
        let _keep_wpm = wpm_timer;
        window.run().unwrap();
    }
}
