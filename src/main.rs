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
    #[allow(dead_code)]
    TextLines(String, Vec<String>), // kept for OTA legacy compatibility
    HeatmapData(Vec<Vec<u32>>, u32), // counts, max
    BigramLines(Vec<String>),
    LayoutJson(Vec<KeycapPos>),
    MacroList(Vec<logic::parsers::MacroEntry>),
    TdList(Vec<[u16; 4]>),
    ComboList(Vec<logic::parsers::ComboEntry>),
    LeaderList(Vec<logic::parsers::LeaderEntry>),
    KoList(Vec<[u8; 4]>),
    BtStatus(Vec<String>),
    TamaStatus(Vec<String>),
    AutoshiftStatus(String),
    Wpm(u16),
    FlashProgress(f32, String),
    FlashDone(Result<(), String>),
    OtaProgress(f32, String),
    OtaDone(Result<(), String>),
    ConfigProgress(f32, String),
    ConfigDone(Result<String, String>),
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

/// Export all keyboard config to JSON file via rfd save dialog.
/// Uses binary protocol v2 for all queries (fast, no text parsing).
fn export_config(
    serial: &Arc<Mutex<SerialManager>>,
    tx: &mpsc::Sender<BgMsg>,
) -> Result<String, String> {
    use logic::binary_protocol::cmd;
    use logic::config_io::*;

    let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());

    // 1. Layer names (binary LIST_LAYOUTS 0x21)
    let _ = tx.send(BgMsg::ConfigProgress(0.05, "Reading layer names...".into()));
    let layer_names = ser.get_layer_names().unwrap_or_default();
    let num_layers = layer_names.len().max(1);

    // 2. Keymaps — binary KEYMAP_GET per layer
    let mut keymaps = Vec::new();
    for layer in 0..num_layers {
        let progress = 0.05 + (layer as f32 / num_layers as f32) * 0.30;
        let _ = tx.send(BgMsg::ConfigProgress(progress, format!("Reading layer {}...", layer)));
        let km = ser.get_keymap(layer as u8).unwrap_or_default();
        keymaps.push(km);
    }

    // 3. Tap dances — binary TD_LIST (0x51)
    let _ = tx.send(BgMsg::ConfigProgress(0.40, "Reading tap dances...".into()));
    let tap_dances = match ser.send_binary(cmd::TD_LIST, &[]) {
        Ok(resp) => {
            let td_raw = logic::parsers::parse_td_binary(&resp.payload);
            td_raw.iter().enumerate()
                .filter(|(_, actions)| actions.iter().any(|&a| a != 0))
                .map(|(i, actions)| TdConfig { index: i as u8, actions: *actions })
                .collect()
        }
        Err(_) => Vec::new(),
    };

    // 4. Combos — binary COMBO_LIST (0x61)
    let _ = tx.send(BgMsg::ConfigProgress(0.50, "Reading combos...".into()));
    let combos = match ser.send_binary(cmd::COMBO_LIST, &[]) {
        Ok(resp) => {
            logic::parsers::parse_combo_binary(&resp.payload).iter().map(|c| ComboConfig {
                index: c.index, r1: c.r1, c1: c.c1, r2: c.r2, c2: c.c2, result: c.result,
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    // 5. Key overrides — binary KO_LIST (0x92)
    let _ = tx.send(BgMsg::ConfigProgress(0.60, "Reading key overrides...".into()));
    let key_overrides = match ser.send_binary(cmd::KO_LIST, &[]) {
        Ok(resp) => {
            logic::parsers::parse_ko_binary(&resp.payload).iter().map(|ko| KoConfig {
                trigger_key: ko[0], trigger_mod: ko[1], result_key: ko[2], result_mod: ko[3],
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    // 6. Leaders — binary LEADER_LIST (0x71)
    let _ = tx.send(BgMsg::ConfigProgress(0.70, "Reading leaders...".into()));
    let leaders = match ser.send_binary(cmd::LEADER_LIST, &[]) {
        Ok(resp) => {
            logic::parsers::parse_leader_binary(&resp.payload).iter().map(|l| LeaderConfig {
                index: l.index, sequence: l.sequence.clone(), result: l.result, result_mod: l.result_mod,
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    // 7. Macros — binary LIST_MACROS (0x30)
    let _ = tx.send(BgMsg::ConfigProgress(0.80, "Reading macros...".into()));
    let macros = match ser.send_binary(cmd::LIST_MACROS, &[]) {
        Ok(resp) => {
            logic::parsers::parse_macros_binary(&resp.payload).iter().map(|m| {
                let steps_str: Vec<String> = m.steps.iter()
                    .map(|s| format!("{:02X}:{:02X}", s.keycode, s.modifier))
                    .collect();
                MacroConfig { slot: m.slot, name: m.name.clone(), steps: steps_str.join(",") }
            }).collect()
        }
        Err(_) => Vec::new(),
    };

    drop(ser); // Release serial lock before file dialog

    let _ = tx.send(BgMsg::ConfigProgress(0.90, "Saving file...".into()));

    let config = KeyboardConfig {
        version: 1,
        layer_names,
        keymaps,
        tap_dances,
        combos,
        key_overrides,
        leaders,
        macros,
    };

    let json = config.to_json()?;

    let file = rfd::FileDialog::new()
        .add_filter("KeSp Config", &["json"])
        .set_file_name("kesp_config.json")
        .save_file();

    match file {
        Some(path) => {
            std::fs::write(&path, &json).map_err(|e| format!("Write error: {}", e))?;
            Ok(format!("Exported to {}", path.display()))
        }
        None => Ok("Export cancelled".into()),
    }
}

/// Import keyboard config using binary protocol v2.
/// SETLAYER sends a full layer in one frame (~131 bytes) instead of 65 individual SETKEY.
fn import_config(
    serial: &Arc<Mutex<SerialManager>>,
    tx: &mpsc::Sender<BgMsg>,
    config: &logic::config_io::KeyboardConfig,
) -> Result<String, String> {
    use logic::binary_protocol as bp;

    let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
    let mut errors = 0usize;

    let total_steps = (config.layer_names.len()
        + config.keymaps.len()  // 1 SETLAYER per layer (not per key!)
        + config.tap_dances.len()
        + config.combos.len()
        + config.key_overrides.len()
        + config.leaders.len()
        + config.macros.len())
        .max(1) as f32;
    let mut done = 0usize;

    // 1. Layer names — binary SET_LAYOUT_NAME (0x20)
    let _ = tx.send(BgMsg::ConfigProgress(0.0, "Setting layer names...".into()));
    for (i, name) in config.layer_names.iter().enumerate() {
        let payload = bp::set_layout_name_payload(i as u8, name);
        if ser.send_binary(bp::cmd::SET_LAYOUT_NAME, &payload).is_err() { errors += 1; }
        done += 1;
    }

    // 2. Keymaps — binary SETLAYER (0x10): one frame per layer!
    for (layer, km) in config.keymaps.iter().enumerate() {
        let progress = done as f32 / total_steps;
        let _ = tx.send(BgMsg::ConfigProgress(progress, format!("Writing layer {}...", layer)));
        let payload = bp::setlayer_payload(layer as u8, km);
        if let Err(e) = ser.send_binary(bp::cmd::SETLAYER, &payload) {
            eprintln!("SETLAYER {} failed: {}", layer, e);
            errors += 1;
        }
        done += 1;
    }

    // 3. Tap dances — binary TD_SET (0x50)
    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting tap dances...".into()));
    for td in &config.tap_dances {
        let payload = bp::td_set_payload(td.index, &td.actions);
        if ser.send_binary(bp::cmd::TD_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    // 4. Combos — binary COMBO_SET (0x60)
    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting combos...".into()));
    for combo in &config.combos {
        let payload = bp::combo_set_payload(combo.index, combo.r1, combo.c1, combo.r2, combo.c2, combo.result as u8);
        if ser.send_binary(bp::cmd::COMBO_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    // 5. Key overrides — binary KO_SET (0x91)
    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting key overrides...".into()));
    for (i, ko) in config.key_overrides.iter().enumerate() {
        let payload = bp::ko_set_payload(i as u8, ko.trigger_key, ko.trigger_mod, ko.result_key, ko.result_mod);
        if ser.send_binary(bp::cmd::KO_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    // 6. Leaders — binary LEADER_SET (0x70)
    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting leaders...".into()));
    for leader in &config.leaders {
        let payload = bp::leader_set_payload(leader.index, &leader.sequence, leader.result, leader.result_mod);
        if ser.send_binary(bp::cmd::LEADER_SET, &payload).is_err() { errors += 1; }
        done += 1;
    }

    // 7. Macros — binary MACRO_ADD_SEQ (0x32)
    let _ = tx.send(BgMsg::ConfigProgress(done as f32 / total_steps, "Setting macros...".into()));
    for m in &config.macros {
        let payload = bp::macro_add_seq_payload(m.slot, &m.name, &m.steps);
        if ser.send_binary(bp::cmd::MACRO_ADD_SEQ, &payload).is_err() { errors += 1; }
    }

    // 8. Refresh UI
    let _ = tx.send(BgMsg::ConfigProgress(0.95, "Refreshing...".into()));
    let names = ser.get_layer_names().unwrap_or_default();
    let km = ser.get_keymap(0).unwrap_or_default();
    let _ = tx.send(BgMsg::LayerNames(names));
    let _ = tx.send(BgMsg::Keymap(km));

    let total_keys: usize = config.keymaps.iter()
        .map(|l| l.iter().map(|r| r.len()).sum::<usize>())
        .sum();

    if errors > 0 {
        Ok(format!("Import done with {} errors (check stderr)", errors))
    } else {
        Ok(format!("Imported: {} layers, {} keys, {} TD, {} combos, {} KO, {} leaders, {} macros",
            config.layer_names.len(),
            total_keys,
            config.tap_dances.len(),
            config.combos.len(),
            config.key_overrides.len(),
            config.leaders.len(),
            config.macros.len(),
        ))
    }
}

/// Populate LayoutBridge with parsed JSON layout for preview.
fn populate_layout_preview(window: &MainWindow, json: &str) {
    let lb = window.global::<LayoutBridge>();
    match logic::layout::parse_json(json) {
        Ok(keys) => {
            let keycaps: Vec<KeycapData> = keys.iter().enumerate().map(|(idx, kp)| KeycapData {
                x: kp.x, y: kp.y, w: kp.w, h: kp.h,
                rotation: kp.angle,
                rotation_cx: kp.w / 2.0, rotation_cy: kp.h / 2.0,
                label: SharedString::from(format!("R{}C{}", kp.row, kp.col)),
                sublabel: SharedString::default(),
                keycode: 0, color: slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a),
                heat: 0.0, selected: false, index: idx as i32,
            }).collect();
            // Compute content bounds
            let max_x = keys.iter().map(|k| k.x + k.w).fold(0.0f32, f32::max);
            let max_y = keys.iter().map(|k| k.y + k.h).fold(0.0f32, f32::max);
            lb.set_content_width(max_x + 20.0);
            lb.set_content_height(max_y + 20.0);
            lb.set_keycaps(ModelRc::from(Rc::new(VecModel::from(keycaps))));
            lb.set_status(SharedString::from(format!("{} keys loaded", keys.len())));
            // Pretty-print JSON for display and export
            let pretty_json = serde_json::from_str::<serde_json::Value>(json)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or_else(|_| json.to_string());
            lb.set_json_text(SharedString::from(pretty_json));
        }
        Err(e) => {
            lb.set_status(SharedString::from(format!("Parse error: {}", e)));
            lb.set_json_text(SharedString::from(json));
        }
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
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        keymap_bridge.on_rename_layer(move |layer_idx, new_name| {
            let payload = logic::binary_protocol::set_layout_name_payload(layer_idx as u8, &new_name);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::SET_LAYOUT_NAME, &payload);
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
        let serial = serial.clone();
        let tx = bg_tx.clone();

        keymap_bridge.on_toggle_heatmap(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(resp) = ser.send_binary(logic::binary_protocol::cmd::KEYSTATS_BIN, &[]) {
                    let (data, max) = logic::parsers::parse_keystats_binary(&resp.payload);
                    let _ = tx.send(BgMsg::HeatmapData(data, max));
                }
            });
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

    // --- Auto-refresh on tab change ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<AppState>().on_tab_changed(move |tab_idx| {
            let Some(w) = window_weak.upgrade() else { return };
            if w.global::<AppState>().get_connection() != ConnectionState::Connected { return; }

            let serial = serial.clone();
            let tx = tx.clone();
            match tab_idx {
                1 => {
                    // Advanced: refresh TD, combo, leader, KO, BT via binary
                    std::thread::spawn(move || {
                        use logic::binary_protocol::cmd;
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(r) = ser.send_binary(cmd::TD_LIST, &[]) {
                            let _ = tx.send(BgMsg::TdList(logic::parsers::parse_td_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                            let _ = tx.send(BgMsg::ComboList(logic::parsers::parse_combo_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                            let _ = tx.send(BgMsg::LeaderList(logic::parsers::parse_leader_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                            let _ = tx.send(BgMsg::KoList(logic::parsers::parse_ko_binary(&r.payload)));
                        }
                        if let Ok(r) = ser.send_binary(cmd::BT_QUERY, &[]) {
                            let _ = tx.send(BgMsg::BtStatus(logic::parsers::parse_bt_binary(&r.payload)));
                        }
                    });
                }
                2 => {
                    // Macros: refresh via binary
                    std::thread::spawn(move || {
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(resp) = ser.send_binary(logic::binary_protocol::cmd::LIST_MACROS, &[]) {
                            let macros = logic::parsers::parse_macros_binary(&resp.payload);
                            let _ = tx.send(BgMsg::MacroList(macros));
                        }
                    });
                }
                3 => {
                    // Stats: refresh heatmap + bigrams via binary
                    std::thread::spawn(move || {
                        use logic::binary_protocol::cmd;
                        let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                        if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
                            let (data, max) = logic::parsers::parse_keystats_binary(&r.payload);
                            let _ = tx.send(BgMsg::HeatmapData(data, max));
                        }
                        // Bigrams: keep text query (binary format needs dedicated parser)
                        let bigram_lines = if let Ok(r) = ser.send_binary(logic::binary_protocol::cmd::BIGRAMS_TEXT, &[]) {
                            String::from_utf8_lossy(&r.payload).lines().map(|l| l.to_string()).collect()
                        } else { Vec::new() };
                        let _ = tx.send(BgMsg::BigramLines(bigram_lines));
                    });
                }
                _ => {}
            }
        });
    }

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

            let payload = logic::binary_protocol::setkey_payload(layer, row as u8, col as u8, code);
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::SETKEY, &payload);
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
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(r) = ser.send_binary(cmd::KEYSTATS_BIN, &[]) {
                    let (data, max) = logic::parsers::parse_keystats_binary(&r.payload);
                    let _ = tx.send(BgMsg::HeatmapData(data, max));
                }
                let bigram_lines = if let Ok(r) = ser.send_binary(logic::binary_protocol::cmd::BIGRAMS_TEXT, &[]) {
                    String::from_utf8_lossy(&r.payload).lines().map(|l| l.to_string()).collect()
                } else { Vec::new() };
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
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                if let Ok(r) = ser.send_binary(cmd::TD_LIST, &[]) {
                    let _ = tx.send(BgMsg::TdList(logic::parsers::parse_td_binary(&r.payload)));
                }
                if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                    let _ = tx.send(BgMsg::ComboList(logic::parsers::parse_combo_binary(&r.payload)));
                }
                if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                    let _ = tx.send(BgMsg::LeaderList(logic::parsers::parse_leader_binary(&r.payload)));
                }
                if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                    let _ = tx.send(BgMsg::KoList(logic::parsers::parse_ko_binary(&r.payload)));
                }
                if let Ok(r) = ser.send_binary(cmd::BT_QUERY, &[]) {
                    let _ = tx.send(BgMsg::BtStatus(logic::parsers::parse_bt_binary(&r.payload)));
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
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::COMBO_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                    let _ = tx.send(BgMsg::ComboList(logic::parsers::parse_combo_binary(&r.payload)));
                }
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
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::LEADER_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                    let _ = tx.send(BgMsg::LeaderList(logic::parsers::parse_leader_binary(&r.payload)));
                }
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
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::KO_DELETE, &[idx as u8]);
                if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                    let _ = tx.send(BgMsg::KoList(logic::parsers::parse_ko_binary(&r.payload)));
                }
            });
        });
    }

    // --- Advanced: set trilayer ---
    {
        let serial = serial.clone();
        let window_weak = window.as_weak();

        window.global::<AdvancedBridge>().on_set_trilayer(move |l1, l2, l3| {
            let payload = vec![l1 as u8, l2 as u8, l3 as u8];
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::TRILAYER_SET, &payload);
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
            let serial = serial.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(logic::binary_protocol::cmd::BT_SWITCH, &[slot as u8]);
            });
        });
    }

    // --- Advanced: TAMA action ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<AdvancedBridge>().on_tama_action(move |action| {
            use logic::binary_protocol::cmd;
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
                    let _ = tx.send(BgMsg::TamaStatus(logic::parsers::parse_tama_binary(&r.payload)));
                }
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
                match ser.send_binary(logic::binary_protocol::cmd::AUTOSHIFT_TOGGLE, &[]) {
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
            let next_idx = adv.get_combos().row_count() as u8;
            let payload = logic::binary_protocol::combo_set_payload(next_idx, r1, c1, r2, c2, result);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::COMBO_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::COMBO_LIST, &[]) {
                    let _ = tx.send(BgMsg::ComboList(logic::parsers::parse_combo_binary(&r.payload)));
                }
            });
            w.global::<AppState>().set_status_text("Creating combo...".into());
        });
    }

    // --- Advanced: create KO ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
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
            let payload = logic::binary_protocol::ko_set_payload(next_idx, trig, trig_mod, result, res_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::KO_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::KO_LIST, &[]) {
                    let _ = tx.send(BgMsg::KoList(logic::parsers::parse_ko_binary(&r.payload)));
                }
            });
            w.global::<AppState>().set_status_text("Creating key override...".into());
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
            let next_idx = adv.get_leaders().row_count() as u8;
            let payload = logic::binary_protocol::leader_set_payload(next_idx, &sequence, result, result_mod);
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::LEADER_SET, &payload);
                if let Ok(r) = ser.send_binary(cmd::LEADER_LIST, &[]) {
                    let _ = tx.send(BgMsg::LeaderList(logic::parsers::parse_leader_binary(&r.payload)));
                }
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
                if ser.v2 {
                    if let Ok(resp) = ser.send_binary(logic::binary_protocol::cmd::LIST_MACROS, &[]) {
                        let macros = logic::parsers::parse_macros_binary(&resp.payload);
                        let _ = tx.send(BgMsg::MacroList(macros));
                    }
                } else {
                    // Legacy fallback — should not happen with v2 firmware
                    let _ = ser.send_binary(logic::binary_protocol::cmd::LIST_MACROS, &[]);
                }
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
            let slot_num = mb.get_macros().row_count() as u8;
            let name = mb.get_new_name().to_string();
            let steps = macro_steps.borrow();
            let steps_str: Vec<String> = steps.iter().map(|&(kc, md)| {
                if kc == 0xFF { format!("{:02X}:{:02X}", kc, md) }
                else { format!("{:02X}:{:02X}", kc, md) }
            }).collect();
            let steps_text = steps_str.join(",");
            drop(steps);
            let payload = logic::binary_protocol::macro_add_seq_payload(slot_num, &name, &steps_text);

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::MACRO_ADD_SEQ, &payload);
                if let Ok(resp) = ser.send_binary(cmd::LIST_MACROS, &[]) {
                    let macros = logic::parsers::parse_macros_binary(&resp.payload);
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
        let serial = serial.clone();
        let tx = bg_tx.clone();

        window.global::<MacroBridge>().on_delete_macro(move |slot| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use logic::binary_protocol::cmd;
                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let _ = ser.send_binary(cmd::MACRO_DELETE, &logic::binary_protocol::macro_delete_payload(slot as u8));
                if let Ok(resp) = ser.send_binary(cmd::LIST_MACROS, &[]) {
                    let macros = logic::parsers::parse_macros_binary(&resp.payload);
                    let _ = tx.send(BgMsg::MacroList(macros));
                }
            });
        });
    }

    // --- OTA: browse ---
    {
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_ota_browse(move || {
            let window_weak = window_weak.clone();
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("Firmware", &["bin"])
                    .pick_file();
                if let Some(path) = file {
                    let path_str = path.to_string_lossy().to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<SettingsBridge>().set_ota_path(SharedString::from(path_str.as_str()));
                        }
                    });
                }
            });
        });
    }

    // --- OTA: start ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<SettingsBridge>().on_ota_start(move || {
            let Some(w) = window_weak.upgrade() else { return };
            let settings = w.global::<SettingsBridge>();
            let path = settings.get_ota_path().to_string();
            if path.is_empty() { return; }

            let firmware = match std::fs::read(&path) {
                Ok(data) => data,
                Err(e) => {
                    let _ = tx.send(BgMsg::OtaDone(Err(format!("Cannot read {}: {}", path, e))));
                    return;
                }
            };

            settings.set_ota_flashing(true);
            settings.set_ota_progress(0.0);
            settings.set_ota_status(SharedString::from("Starting OTA..."));

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                use std::io::{Read, Write, BufRead, BufReader};

                let mut ser = serial.lock().unwrap_or_else(|e| e.into_inner());
                let total = firmware.len();
                let chunk_size = 4096usize;

                // Step 1: Send OTA <size> command
                let cmd = format!("OTA {}", total);
                if let Err(e) = ser.send_command(&cmd) {
                    let _ = tx.send(BgMsg::OtaDone(Err(format!("Send OTA cmd failed: {}", e))));
                    return;
                }

                // Step 2: Wait for OTA_READY (read lines with timeout)
                let _ = tx.send(BgMsg::OtaProgress(0.0, "Waiting for OTA_READY...".into()));
                let port = match ser.port_mut() {
                    Some(p) => p,
                    None => {
                        let _ = tx.send(BgMsg::OtaDone(Err("Port not available".into())));
                        return;
                    }
                };

                // Read until we get OTA_READY or timeout
                let old_timeout = port.timeout();
                let _ = port.set_timeout(std::time::Duration::from_secs(5));
                let mut got_ready = false;
                let port_clone = port.try_clone().unwrap();
                let mut reader = BufReader::new(port_clone);
                for _ in 0..20 {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() {
                        if line.contains("OTA_READY") {
                            got_ready = true;
                            break;
                        }
                    }
                }
                drop(reader);

                if !got_ready {
                    let _ = port.set_timeout(old_timeout);
                    let _ = tx.send(BgMsg::OtaDone(Err("Firmware did not respond OTA_READY".into())));
                    return;
                }

                // Step 3: Send chunks and wait for ACK after each
                let num_chunks = (total + chunk_size - 1) / chunk_size;
                let _ = port.set_timeout(std::time::Duration::from_secs(5));

                for (i, chunk) in firmware.chunks(chunk_size).enumerate() {
                    // Send chunk
                    if let Err(e) = port.write_all(chunk) {
                        let _ = port.set_timeout(old_timeout);
                        let _ = tx.send(BgMsg::OtaDone(Err(format!("Write chunk {} failed: {}", i, e))));
                        return;
                    }
                    let _ = port.flush();

                    let progress = (i + 1) as f32 / num_chunks as f32;
                    let _ = tx.send(BgMsg::OtaProgress(progress * 0.95, format!(
                        "Chunk {}/{} ({} KB / {} KB)",
                        i + 1, num_chunks,
                        ((i + 1) * chunk_size).min(total) / 1024,
                        total / 1024
                    )));

                    // Wait for ACK line
                    let mut ack_buf = [0u8; 256];
                    let mut ack = String::new();
                    let start = std::time::Instant::now();
                    while start.elapsed() < std::time::Duration::from_secs(5) {
                        match port.read(&mut ack_buf) {
                            Ok(n) if n > 0 => {
                                ack.push_str(&String::from_utf8_lossy(&ack_buf[..n]));
                                if ack.contains("OTA_OK") || ack.contains("OTA_DONE") || ack.contains("OTA_FAIL") {
                                    break;
                                }
                            }
                            _ => std::thread::sleep(std::time::Duration::from_millis(10)),
                        }
                    }

                    if ack.contains("OTA_FAIL") {
                        let _ = port.set_timeout(old_timeout);
                        let _ = tx.send(BgMsg::OtaDone(Err(format!("Firmware error: {}", ack.trim()))));
                        return;
                    }
                    if ack.contains("OTA_DONE") {
                        break; // Firmware signals all received
                    }
                }

                let _ = port.set_timeout(old_timeout);
                let _ = tx.send(BgMsg::OtaProgress(1.0, "OTA complete, rebooting...".into()));
                let _ = tx.send(BgMsg::OtaDone(Ok(())));
            });
        });
    }

    // --- Config Export ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<SettingsBridge>().on_config_export(move || {
            let Some(w) = window_weak.upgrade() else { return };
            w.global::<SettingsBridge>().set_config_busy(true);
            w.global::<SettingsBridge>().set_config_progress(0.0);
            w.global::<SettingsBridge>().set_config_status(SharedString::from("Reading config..."));

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let result = export_config(&serial, &tx);
                let _ = tx.send(BgMsg::ConfigDone(result));
            });
        });
    }

    // --- Config Import ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();

        window.global::<SettingsBridge>().on_config_import(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            let window_weak = window_weak.clone();
            std::thread::spawn(move || {
                let file = rfd::FileDialog::new()
                    .add_filter("KeSp Config", &["json"])
                    .pick_file();
                let Some(path) = file else { return };

                let _ = slint::invoke_from_event_loop({
                    let window_weak = window_weak.clone();
                    move || {
                        if let Some(w) = window_weak.upgrade() {
                            let s = w.global::<SettingsBridge>();
                            s.set_config_busy(true);
                            s.set_config_progress(0.0);
                            s.set_config_status(SharedString::from("Importing config..."));
                        }
                    }
                });

                let json = match std::fs::read_to_string(&path) {
                    Ok(j) => j,
                    Err(e) => {
                        let _ = tx.send(BgMsg::ConfigDone(Err(format!("Read error: {}", e))));
                        return;
                    }
                };
                let config = match logic::config_io::KeyboardConfig::from_json(&json) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(BgMsg::ConfigDone(Err(format!("Parse error: {}", e))));
                        return;
                    }
                };

                let result = import_config(&serial, &tx, &config);
                let _ = tx.send(BgMsg::ConfigDone(result));
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

    // --- Layout preview: load from file ---
    {
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
                            populate_layout_preview(&w, &json);
                        }
                    }
                });
            });
        });
    }

    // --- Layout preview: load from keyboard ---
    {
        let serial = serial.clone();
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
                            populate_layout_preview(&w, &json);
                        }
                    }
                });
            });
        });
    }

    // --- Layout preview: export JSON ---
    {
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
                            let active = window.global::<KeymapBridge>().get_active_layer() as usize;
                            let layers: Vec<LayerInfo> = names.iter().enumerate().map(|(i, name)| LayerInfo {
                                index: i as i32,
                                name: SharedString::from(name.as_str()),
                                active: i == active,
                            }).collect();
                            window.global::<KeymapBridge>().set_layers(
                                ModelRc::from(Rc::new(VecModel::from(layers)))
                            );
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
                        BgMsg::TextLines(_tag, _lines) => {
                            // Legacy text handler — kept for OTA compatibility only
                        }
                        BgMsg::TdList(td_data) => {
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
                        BgMsg::ComboList(combo_data) => {
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
                        BgMsg::LeaderList(leader_data) => {
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
                        BgMsg::KoList(ko_data) => {
                            let model: Vec<KeyOverrideData> = ko_data.iter().enumerate().map(|(i, ko)| {
                                let trig_key = keycode::hid_key_name(ko[0]);
                                let trig_mod = keycode::mod_name(ko[1]);
                                let res_key = keycode::hid_key_name(ko[2]);
                                let res_mod = keycode::mod_name(ko[3]);
                                let trigger = if ko[1] != 0 {
                                    format!("{}+{}", trig_mod, trig_key)
                                } else {
                                    trig_key
                                };
                                let result = if ko[3] != 0 {
                                    format!("{}+{}", res_mod, res_key)
                                } else {
                                    res_key
                                };
                                KeyOverrideData {
                                    index: i as i32,
                                    trigger: SharedString::from(trigger),
                                    result: SharedString::from(result),
                                }
                            }).collect();
                            window.global::<AdvancedBridge>().set_key_overrides(
                                ModelRc::from(Rc::new(VecModel::from(model)))
                            );
                        }
                        BgMsg::BtStatus(lines) => {
                            let bt_text = lines.join("\n");
                            window.global::<AdvancedBridge>().set_bt_status(SharedString::from(bt_text));
                        }
                        BgMsg::TamaStatus(lines) => {
                            let text = lines.join("\n");
                            window.global::<AdvancedBridge>().set_tama_status(SharedString::from(text));
                        }
                        BgMsg::AutoshiftStatus(text) => {
                            window.global::<AdvancedBridge>().set_autoshift_status(SharedString::from(text));
                        }
                        BgMsg::Wpm(wpm) => {
                            window.global::<AppState>().set_wpm(wpm as i32);
                        }
                        BgMsg::OtaProgress(progress, msg) => {
                            let s = window.global::<SettingsBridge>();
                            s.set_ota_progress(progress);
                            s.set_ota_status(SharedString::from(msg));
                        }
                        BgMsg::OtaDone(result) => {
                            let s = window.global::<SettingsBridge>();
                            s.set_ota_flashing(false);
                            match result {
                                Ok(()) => {
                                    s.set_ota_progress(1.0);
                                    s.set_ota_status(SharedString::from("OTA complete!"));
                                }
                                Err(e) => {
                                    s.set_ota_status(SharedString::from(format!("OTA error: {}", e)));
                                }
                            }
                        }
                        BgMsg::ConfigProgress(progress, msg) => {
                            let s = window.global::<SettingsBridge>();
                            s.set_config_progress(progress);
                            s.set_config_status(SharedString::from(msg));
                        }
                        BgMsg::ConfigDone(result) => {
                            let s = window.global::<SettingsBridge>();
                            s.set_config_busy(false);
                            match result {
                                Ok(msg) => {
                                    s.set_config_progress(1.0);
                                    s.set_config_status(SharedString::from(msg));
                                }
                                Err(e) => {
                                    s.set_config_progress(0.0);
                                    s.set_config_status(SharedString::from(format!("Error: {}", e)));
                                }
                            }
                        }
                        BgMsg::MacroList(macros) => {
                            let model: Vec<MacroData> = macros.iter().map(|m| {
                                let steps_str: Vec<String> = m.steps.iter().map(|s| {
                                    if s.is_delay() { format!("T({})", s.delay_ms()) }
                                    else { format!("{}", keycode::hid_key_name(s.keycode)) }
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
                        if let Ok(r) = ser.send_binary(logic::binary_protocol::cmd::WPM_QUERY, &[]) {
                            let wpm = if r.payload.len() >= 2 {
                                u16::from_le_bytes([r.payload[0], r.payload[1]])
                            } else { 0 };
                            let _ = tx.send(BgMsg::Wpm(wpm));
                        }
                    });
                },
            );
        }

        // Keep timers alive
        // Layout auto-refresh timer (5s)
        let layout_timer = slint::Timer::default();
        {
            let window_weak3 = window.as_weak();
            layout_timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_secs(5),
                move || {
                    let Some(w) = window_weak3.upgrade() else { return };
                    let lb = w.global::<LayoutBridge>();
                    if !lb.get_auto_refresh() { return; }
                    let path = lb.get_file_path().to_string();
                    if path.is_empty() { return; }
                    if let Ok(json) = std::fs::read_to_string(&path) {
                        populate_layout_preview(&w, &json);
                    }
                },
            );
        }

        let _keep_timer = timer;
        let _keep_wpm = wpm_timer;
        let _keep_layout = layout_timer;
        window.run().unwrap();
    }
}
