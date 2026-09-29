use crate::protocol::{self as protocol, keycode, layout::KeycapPos, layout_remap};
use crate::{
    KeycapData, KeyEntry, KeySelectorBridge, KeymapBridge, LayoutBridge, LayerInfo, MainWindow,
    SettingsBridge,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

/// Initialize all UI models from default state.
pub fn init_models(
    window: &MainWindow,
    keys: &[KeycapPos],
    saved_settings: &crate::protocol::settings::Settings,
) {
    let kb = window.global::<KeymapBridge>();
    kb.set_keycaps(ModelRc::from(build_keycap_model(keys)));
    kb.set_layers(ModelRc::from(build_layer_model(&[
        "Layer 0".into(), "Layer 1".into(), "Layer 2".into(), "Layer 3".into(),
    ])));
    let mut max_x: f32 = 0.0;
    let mut max_y: f32 = 0.0;
    for kp in keys {
        if kp.x + kp.w > max_x { max_x = kp.x + kp.w; }
        if kp.y + kp.h > max_y { max_y = kp.y + kp.h; }
    }
    kb.set_content_width(max_x);
    kb.set_content_height(max_y);

    // Available keyboard layouts
    let layouts: Vec<SharedString> = layout_remap::KeyboardLayout::all()
        .iter()
        .map(|l| SharedString::from(l.name()))
        .collect();
    window.global::<SettingsBridge>().set_available_layouts(
        ModelRc::from(Rc::new(VecModel::from(layouts))),
    );

    // Initial layout index
    let current = layout_remap::KeyboardLayout::from_name(&saved_settings.keyboard_layout);
    let idx = layout_remap::KeyboardLayout::all()
        .iter()
        .position(|l| *l == current)
        .unwrap_or(0);
    window.global::<SettingsBridge>().set_selected_layout_index(idx as i32);

    // Key selector
    let all_keys = build_key_entries_with_layout(&current);
    window.global::<KeySelectorBridge>().set_all_keys(ModelRc::from(all_keys.clone()));
    populate_key_categories(window, &all_keys, "");
}

pub fn build_keycap_model(keys: &[KeycapPos]) -> Rc<VecModel<KeycapData>> {
    let keycaps: Vec<KeycapData> = keys
        .iter()
        .enumerate()
        .map(|(idx, kp)| KeycapData {
            x: kp.x, y: kp.y, w: kp.w, h: kp.h,
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

pub fn build_layer_model(names: &[String]) -> Rc<VecModel<LayerInfo>> {
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

pub fn update_keycap_labels(
    keycap_model: &impl Model<Data = KeycapData>,
    keys: &[KeycapPos],
    keymap: &[Vec<u16>],
    layout: &layout_remap::KeyboardLayout,
) {
    for i in 0..keycap_model.row_count() {
        if i >= keys.len() { break; }
        let mut item = keycap_model.row_data(i).unwrap();
        let kp = &keys[i];
        let row = kp.row;
        let col = kp.col;

        if row < keymap.len() && col < keymap[row].len() {
            let code = keymap[row][col];
            let decoded = keycode::decode_keycode(code);
            let remapped = layout_remap::remap_key_label(layout, &decoded);
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

pub fn build_key_entries_with_layout(layout: &layout_remap::KeyboardLayout) -> Rc<VecModel<KeyEntry>> {
    let hid_entry = |code: u16, category: &str| -> KeyEntry {
        let base = keycode::hid_key_name(code as u8);
        let name = layout_remap::remap_key_label(layout, &base)
            .map(|s| s.to_string())
            .unwrap_or(base);
        KeyEntry { name: SharedString::from(name), code: code as i32, category: SharedString::from(category) }
    };
    let mut entries = Vec::new();

    for code in 0x04u16..=0x1D { entries.push(hid_entry(code, "Letter")); }
    for code in 0x1Eu16..=0x27 { entries.push(hid_entry(code, "Number")); }
    for code in [0x28u16, 0x29, 0x2A, 0x2B, 0x2C] { entries.push(hid_entry(code, "Control")); }
    for code in 0x2Du16..=0x38 { entries.push(hid_entry(code, "Symbol")); }
    // Shifted symbols as standalone keys — firmware MK 0x82xx = Shift + base
    // key in one press, for boards without a number row. Named per OS layout
    // (`!` on US, an honest `Shift+…` elsewhere); see layout_remap::shifted_label.
    for base in [0x1Eu8, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
                 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38] {
        entries.push(KeyEntry {
            name: SharedString::from(layout_remap::shifted_label(layout, base)),
            code: i32::from(0x8200u16 | u16::from(base)),
            category: SharedString::from("Symbol"),
        });
    }
    for code in 0x3Au16..=0x45 { entries.push(hid_entry(code, "Function")); }
    for code in [0x46u16, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52] {
        entries.push(hid_entry(code, "Navigation"));
    }
    for code in 0xE0u16..=0xE7 { entries.push(hid_entry(code, "Modifier")); }
    entries.push(KeyEntry {
        name: SharedString::from("Caps Lock"),
        code: 0x39,
        category: SharedString::from("Control"),
    });
    entries.push(hid_entry(0x32, "Symbol")); // Europe1 / ISO #~ (*µ on AZERTY)
    entries.push(hid_entry(0x64, "Symbol")); // Europe2 / ISO <> (<> on AZERTY)
    for code in 0x53u16..=0x63 { entries.push(hid_entry(code, "Keypad")); }
    for code in 0x68u16..=0x73 { entries.push(hid_entry(code, "Function")); }
    for code in [0x7Fu16, 0x80, 0x81] { entries.push(hid_entry(code, "Media")); }
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
    for i in 0u16..=7 {
        let code = (0x60 | i) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("TD {}", i)),
            code: code as i32,
            category: SharedString::from("Tap Dance"),
        });
    }
    for i in 0u16..=9 {
        let code = (i + 0x15) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("M{}", i)),
            code: code as i32,
            category: SharedString::from("Macro"),
        });
    }
    for i in 0u16..=9 {
        entries.push(KeyEntry {
            name: SharedString::from(format!("OSL {}", i)),
            code: 0x3100 + i as i32,
            category: SharedString::from("Layer"),
        });
    }
    for layer in 0u16..=9 {
        let code = (layer + 1) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("MO {}", layer)),
            code: code as i32,
            category: SharedString::from("Layer"),
        });
    }
    for layer in 0u16..=9 {
        let code = (layer + 0x0B) << 8;
        entries.push(KeyEntry {
            name: SharedString::from(format!("TO {}", layer)),
            code: code as i32,
            category: SharedString::from("Layer"),
        });
    }
    // Coffre Niphargus — un ESP32-P4 distinct, relié en SPI à la moitié gauche.
    // Ses touches ont leur propre section : ce ne sont pas des fonctions du
    // clavier mais des commandes envoyées à un autre appareil.
    for (code, name) in [
        (0x3E00u16, "Sec Confirm"), (0x3E01, "Chest Next"),
        (0x3E02, "OATH Prev"), (0x3E03, "OATH Next"), (0x3E04, "OATH Code"),
    ] {
        entries.push(KeyEntry {
            name: SharedString::from(name),
            code: code as i32,
            category: SharedString::from("Chest"),
        });
    }
    for (code, name) in [
        (0x3200u16, "Caps Word"), (0x3300, "Repeat"), (0x3400, "Leader"),
        (0x3900, "GEsc"), (0x3A00, "Layer Lock"), (0x3C00, "AS Toggle"),
        // K_WPM 0x3B00 n'est PAS listé : le firmware le documente comme un
        // usage interne (requête affichée à l'écran), pas destiné à une
        // keymap — et aucun code de main/ ne le traite. decode_keycode le
        // connaît quand même, pour qu'une keymap qui en contient reste lisible.
        //
        // Disp Next reste ici : c'est l'écran OLED du clavier. Les touches du
        // coffre sont dans leur propre section (voir plus bas).
        (0x3F00, "Disp Next"),
    ] {
        entries.push(KeyEntry {
            name: SharedString::from(name),
            code: code as i32,
            category: SharedString::from("Special"),
        });
    }
    entries.insert(0, KeyEntry {
        name: SharedString::from("None"),
        code: 0,
        category: SharedString::from("Special"),
    });

    Rc::new(VecModel::from(entries))
}

pub fn populate_key_categories(window: &MainWindow, all_keys: &VecModel<KeyEntry>, search: &str) {
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
    ks.set_cat_chest(set(filter("Chest")));
    ks.set_cat_td_macro(set(filter("TDMacro")));
}

/// Parse `json` into the editable buffer and render the Layout Preview.
/// Resets the selection. The JSON panel shows the (re)serialized buffer, so
/// what is exported always matches what is displayed.
pub fn populate_layout_preview(
    window: &MainWindow,
    json: &str,
    buffer: &Rc<std::cell::RefCell<Vec<KeycapPos>>>,
    sel: &Rc<std::cell::Cell<i32>>,
) {
    let lb = window.global::<LayoutBridge>();
    match protocol::layout::parse_json(json) {
        Ok(keys) => {
            lb.set_status(SharedString::from(format!("{} keys loaded", keys.len())));
            *buffer.borrow_mut() = keys;
            sel.set(-1);
            rebuild_layout_model(window, &buffer.borrow(), -1);
        }
        Err(e) => {
            lb.set_status(SharedString::from(format!("Parse error: {}", e)));
            lb.set_json_text(SharedString::from(json));
        }
    }
}

/// Rebuild the LayoutBridge render model, JSON text, and selected-key fields
/// from the editable buffer. `selected` is the index to highlight (-1 = none).
pub fn rebuild_layout_model(window: &MainWindow, keys: &[KeycapPos], selected: i32) {
    use protocol::layout::UNIT_PX;
    let lb = window.global::<LayoutBridge>();
    let keycaps: Vec<KeycapData> = keys.iter().enumerate().map(|(idx, kp)| KeycapData {
        x: kp.x, y: kp.y, w: kp.w, h: kp.h,
        rotation: kp.angle,
        rotation_cx: kp.w / 2.0, rotation_cy: kp.h / 2.0,
        label: SharedString::from(format!("R{}C{}", kp.row, kp.col)),
        sublabel: SharedString::default(),
        keycode: 0, color: slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a),
        heat: 0.0, selected: idx as i32 == selected, index: idx as i32,
    }).collect();
    let max_x = keys.iter().map(|k| k.x + k.w).fold(0.0f32, f32::max);
    let max_y = keys.iter().map(|k| k.y + k.h).fold(0.0f32, f32::max);
    lb.set_content_width(max_x + 20.0);
    lb.set_content_height(max_y + 20.0);
    lb.set_keycaps(ModelRc::from(Rc::new(VecModel::from(keycaps))));
    lb.set_json_text(SharedString::from(protocol::layout::serialize_json(keys)));
    lb.set_selected_index(selected);
    if let Some(k) = (selected >= 0).then(|| keys.get(selected as usize)).flatten() {
        lb.set_sel_x(SharedString::from(fmt_unit(k.x / UNIT_PX)));
        lb.set_sel_y(SharedString::from(fmt_unit(k.y / UNIT_PX)));
        lb.set_sel_rotation(SharedString::from(fmt_unit(k.angle)));
        lb.set_sel_row(SharedString::from(k.row.to_string()));
        lb.set_sel_col(SharedString::from(k.col.to_string()));
    }
}

/// Format a unit value with at most 2 decimals and no trailing zeros
/// (e.g. 1.5 -> "1.5", 2.0 -> "2", 1.25 -> "1.25").
fn fmt_unit(v: f32) -> String {
    format!("{}", (v * 100.0).round() / 100.0)
}
