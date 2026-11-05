//! Key catalogue shown by the palette and the key selector, plus the Mod-Tap
//! and Layer-Tap builders. Numbers come from MECHANICS.md.

use crate::protocol::keycode;
use crate::protocol::layout_remap::{self, KeyboardLayout};

#[derive(Clone, Debug, PartialEq)]
pub struct KeyEntry {
    pub name: String,
    pub code: u16,
    pub category: &'static str,
}

/// Keys the firmware has for tap dances and macros (`TAP_DANCE_MAX_SLOTS`, `MAX_MACROS`).
const TD_KEYS: u16 = 16;
pub const MACRO_KEYS: u16 = 20;
/// Longest macro name the firmware keeps, in bytes (`MAX_MACRO_NAME_LENGTH` - 1).
pub const MACRO_NAME_BYTES: usize = 15;
/// Steps a macro can hold (`MACRO_MAX_STEPS`). More are dropped without a word.
pub const MACRO_STEPS: usize = 24;
/// Slots for combos, leader sequences and overrides.
pub const LIST_SLOTS: u8 = 16;

/// Lowest slot the list doesn't use: a new item never overwrites an existing one.
pub fn free_slot(used: impl Iterator<Item = u8>) -> u8 {
    let used: Vec<u8> = used.collect();
    (0..=u8::MAX).find(|i| !used.contains(i)).unwrap_or(0)
}

/// Cut a macro name to what the firmware keeps, never inside a letter: it cuts on
/// bytes, which would otherwise leave half an accented letter on the keyboard.
pub fn fit_macro_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if out.len() + ch.len_utf8() > MACRO_NAME_BYTES {
            break;
        }
        out.push(ch);
    }
    out
}

/// Sections of the selector, in display order: (title, categories it groups).
pub const SECTIONS: [(&str, &[&str]); 10] = [
    ("Letters", &["Letter"]),
    ("Numbers", &["Number"]),
    ("Modifiers", &["Modifier"]),
    ("Navigation", &["Navigation", "Control"]),
    ("Function Keys", &["Function"]),
    ("Symbols", &["Symbol"]),
    ("Layers", &["Layer"]),
    ("Special / BT / Media", &["Special", "Bluetooth", "Media"]),
    ("Chest (P4)", &["Chest"]),
    ("Tap Dance / Macros", &["Tap Dance", "Macro"]),
];

/// Modifiers a Mod-Tap can hold. The firmware keeps 4 bits for them, so the
/// right-hand ones don't fit (see MECHANICS.md).
pub const MOD_NAMES: [&str; 4] = ["Ctrl", "Shift", "Alt", "GUI"];
pub const MT_KEYS: [&str; 52] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "Space",
    "Enter", "Esc", "Tab", "Bksp", "-", "=", "[", "]", "\\", ";", "'", "`", ",", ".", "/",
];
/// One-shot and Layer + Mod keep a full byte of modifiers: the right-hand ones fit.
pub const ALL_MODS: [&str; 8] = ["Ctrl", "Shift", "Alt", "GUI", "RCtrl", "RShift", "RAlt", "RGUI"];
pub const LT_KEYS: [&str; 10] = ["Space", "Enter", "Esc", "Bksp", "Tab", "A", "B", "C", "D", "E"];

/// Mod-Tap keycode: 0x5000 | modifier nibble << 8 | HID key.
pub fn mod_tap_code(mod_idx: usize, key_idx: usize) -> u16 {
    let mod_bits: u16 = match mod_idx {
        0 => 0x01,
        1 => 0x02,
        2 => 0x04,
        3 => 0x08,
        _ => 0x02,
    };
    0x5000 | (mod_bits << 8) | builder_key(key_idx)
}

/// One-shot modifier: firmware K_OSM = 0x3000 | modifier byte.
pub fn one_shot_code(mod_idx: usize) -> u16 {
    0x3000 | (1u16 << mod_idx.min(7))
}

/// Layer held with a modifier: firmware K_LM = 0x7000 | modifier byte << 4 | layer.
pub fn layer_mod_code(layer: usize, mod_idx: usize) -> u16 {
    0x7000 | ((1u16 << mod_idx.min(7)) << 4) | ((layer as u16) & 0x0F)
}

/// Modifier + key in one press: firmware K_MK = 0x8000 | modifier nibble << 8 | key.
pub fn mod_key_code(mod_idx: usize, key_idx: usize) -> u16 {
    0x8000 | ((1u16 << mod_idx.min(3)) << 8) | builder_key(key_idx)
}

/// HID usage of an entry of `MT_KEYS`.
fn builder_key(key_idx: usize) -> u16 {
    match key_idx {
        0..=25 => 0x04 + key_idx as u16,
        26..=35 => 0x1E + (key_idx - 26) as u16,
        36 => 0x2C,
        37 => 0x28,
        38 => 0x29,
        39 => 0x2B,
        40 => 0x2A,
        41 => 0x2D,
        42 => 0x2E,
        43 => 0x2F,
        44 => 0x30,
        45 => 0x31,
        46 => 0x33,
        47 => 0x34,
        48 => 0x35,
        49 => 0x36,
        50 => 0x37,
        51 => 0x38,
        _ => 0x04,
    }
}

/// Layer-Tap keycode: 0x4000 | layer << 8 | HID key.
pub fn layer_tap_code(layer: usize, key_idx: usize) -> u16 {
    let hid: u16 = match key_idx {
        0 => 0x2C,
        1 => 0x28,
        2 => 0x29,
        3 => 0x2A,
        4 => 0x2B,
        5..=9 => 0x04 + (key_idx - 5) as u16,
        _ => 0x2C,
    };
    0x4000 | (((layer as u16) & 0x0F) << 8) | hid
}

/// Label of any keycode under the chosen OS layout.
pub fn label_for(code: u16, layout: &KeyboardLayout) -> String {
    let decoded = keycode::decode_keycode(code);
    layout_remap::remap_key_label(layout, &decoded)
        .map(|s| s.to_string())
        .unwrap_or(decoded)
}

/// Rough family of a keycode, used to tint keycaps.
pub fn family(code: u16) -> &'static str {
    match code {
        0 => "none",
        0xE0..=0xE7 => "mod",
        0x01..=0xFF => "basic",
        0x0100..=0x1400 | 0x3100..=0x310F | 0x3A00 => "layer",
        0x1500..=0x2800 | 0x6000..=0x6FFF => "macro",
        0x4000..=0x4FFF | 0x7000..=0x7FFF => "layer",
        0x5000..=0x5FFF | 0x3000..=0x30FF => "mod",
        0x8000..=0x8FFF => "basic",
        _ => "special",
    }
}

pub fn catalog(layout: &KeyboardLayout) -> Vec<KeyEntry> {
    let hid = |code: u16, category: &'static str| {
        let base = keycode::hid_key_name(code as u8);
        let name = layout_remap::remap_key_label(layout, &base)
            .map(|s| s.to_string())
            .unwrap_or(base);
        KeyEntry { name, code, category }
    };
    let named = |code: u16, name: &str, category: &'static str| KeyEntry {
        name: name.to_string(),
        code,
        category,
    };
    let mut out = vec![named(0, "None", "Special")];
    out.extend((0x04u16..=0x1D).map(|c| hid(c, "Letter")));
    out.extend((0x1Eu16..=0x27).map(|c| hid(c, "Number")));
    out.extend([0x28u16, 0x29, 0x2A, 0x2B, 0x2C].map(|c| hid(c, "Control")));
    out.extend((0x2Du16..=0x38).map(|c| hid(c, "Symbol")));
    // Shifted symbols as standalone keys: firmware MK 0x82xx = Shift + base key.
    for base in [
        0x1Eu8, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2D, 0x2E, 0x2F, 0x30, 0x31,
        0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
    ] {
        out.push(KeyEntry {
            name: layout_remap::shifted_label(layout, base),
            code: 0x8200 | u16::from(base),
            category: "Symbol",
        });
    }
    out.extend((0x3Au16..=0x45).map(|c| hid(c, "Function")));
    out.extend((0x46u16..=0x52).map(|c| hid(c, "Navigation")));
    out.extend((0xE0u16..=0xE7).map(|c| hid(c, "Modifier")));
    out.push(named(0x39, "Caps Lock", "Control"));
    out.push(hid(0x32, "Symbol")); // Europe1 / ISO #~
    out.push(hid(0x64, "Symbol")); // Europe2 / ISO <>
    out.extend((0x53u16..=0x63).map(|c| hid(c, "Keypad")));
    out.extend((0x68u16..=0x73).map(|c| hid(c, "Function")));
    out.extend([0x7Fu16, 0x80, 0x81].map(|c| hid(c, "Media")));
    for (code, name) in [
        (0x2900u16, "BT Next"),
        (0x2A00, "BT Prev"),
        (0x2B00, "BT Pair"),
        (0x2C00, "BT Disc"),
        (0x2E00, "USB/BT"),
        (0x2F00, "BT On/Off"),
    ] {
        out.push(named(code, name, "Bluetooth"));
    }
    for i in 0u16..TD_KEYS {
        out.push(named((0x60 | i) << 8, &format!("TD {i}"), "Tap Dance"));
    }
    for i in 0u16..MACRO_KEYS {
        out.push(named((i + 0x15) << 8, &format!("M{i}"), "Macro"));
    }
    for i in 0u16..=9 {
        out.push(named(0x3100 + i, &format!("OSL {i}"), "Layer"));
    }
    for i in 0u16..=9 {
        out.push(named((i + 1) << 8, &format!("MO {i}"), "Layer"));
    }
    for i in 0u16..=9 {
        out.push(named((i + 0x0B) << 8, &format!("TO {i}"), "Layer"));
    }
    for (code, name) in [
        (0x3E00u16, "Sec Confirm"),
        (0x3E01, "Chest Next"),
        (0x3E02, "OATH Prev"),
        (0x3E03, "OATH Next"),
        (0x3E04, "OATH Code"),
    ] {
        out.push(named(code, name, "Chest"));
    }
    // K_WPM 0x3B00 is deliberately absent: the firmware treats it as internal.
    for (code, name) in [
        (0x3200u16, "Caps Word"),
        (0x3300, "Repeat"),
        (0x3400, "Leader"),
        (0x3900, "GEsc"),
        (0x3A00, "Layer Lock"),
        (0x3C00, "AS Toggle"),
        (0x3F00, "Disp Next"),
    ] {
        out.push(named(code, name, "Special"));
    }
    out
}

/// Entries of one section that match the search text.
pub fn section_entries<'a>(
    all: &'a [KeyEntry],
    categories: &[&str],
    search: &str,
) -> Vec<&'a KeyEntry> {
    let needle = search.to_lowercase();
    all.iter()
        .filter(|e| categories.contains(&e.category))
        .filter(|e| {
            needle.is_empty()
                || e.name.to_lowercase().contains(&needle)
                || e.category.to_lowercase().contains(&needle)
        })
        .collect()
}

/// HID usage of a physical key, for the "press a key to assign it" mode.
/// Modifier keys never reach this: egui reports them as state, not as keys.
pub fn hid_from_egui(key: egui::Key) -> Option<u16> {
    use egui::Key as K;
    Some(match key {
        K::A => 0x04, K::B => 0x05, K::C => 0x06, K::D => 0x07, K::E => 0x08, K::F => 0x09,
        K::G => 0x0A, K::H => 0x0B, K::I => 0x0C, K::J => 0x0D, K::K => 0x0E, K::L => 0x0F,
        K::M => 0x10, K::N => 0x11, K::O => 0x12, K::P => 0x13, K::Q => 0x14, K::R => 0x15,
        K::S => 0x16, K::T => 0x17, K::U => 0x18, K::V => 0x19, K::W => 0x1A, K::X => 0x1B,
        K::Y => 0x1C, K::Z => 0x1D,
        K::Num1 => 0x1E, K::Num2 => 0x1F, K::Num3 => 0x20, K::Num4 => 0x21, K::Num5 => 0x22,
        K::Num6 => 0x23, K::Num7 => 0x24, K::Num8 => 0x25, K::Num9 => 0x26, K::Num0 => 0x27,
        K::Enter => 0x28, K::Escape => 0x29, K::Backspace => 0x2A, K::Tab => 0x2B,
        K::Space => 0x2C, K::Minus => 0x2D, K::Equals => 0x2E, K::OpenBracket => 0x2F,
        K::CloseBracket => 0x30, K::Backslash => 0x31, K::Semicolon => 0x33, K::Quote => 0x34,
        K::Backtick => 0x35, K::Comma => 0x36, K::Period => 0x37, K::Slash => 0x38,
        K::F1 => 0x3A, K::F2 => 0x3B, K::F3 => 0x3C, K::F4 => 0x3D, K::F5 => 0x3E, K::F6 => 0x3F,
        K::F7 => 0x40, K::F8 => 0x41, K::F9 => 0x42, K::F10 => 0x43, K::F11 => 0x44,
        K::F12 => 0x45,
        K::Insert => 0x49, K::Home => 0x4A, K::PageUp => 0x4B, K::Delete => 0x4C, K::End => 0x4D,
        K::PageDown => 0x4E, K::ArrowRight => 0x4F, K::ArrowLeft => 0x50, K::ArrowDown => 0x51,
        K::ArrowUp => 0x52,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_tap_matches_the_firmware_layout_and_decodes_back() {
        // Shift + A
        assert_eq!(mod_tap_code(1, 0), 0x5204);
        assert_eq!(keycode::decode_keycode(mod_tap_code(1, 0)), "MT Shift A");
        // Ctrl + Space
        assert_eq!(mod_tap_code(0, 36), 0x512C);
    }

    #[test]
    fn every_mod_tap_offered_is_one_the_firmware_recognises_with_its_modifier() {
        for (i, name) in MOD_NAMES.iter().enumerate() {
            let code = mod_tap_code(i, 0);
            // K_IS_MT and K_MT_MOD in the firmware's key definitions.
            assert_eq!(code & 0xF000, 0x5000, "{name}");
            assert_eq!((code >> 8) & 0x0F, 1 << i, "{name}");
            assert_eq!(keycode::decode_keycode(code), format!("MT {name} A"));
        }
    }

    #[test]
    fn mod_tap_keys_match_the_table_of_the_previous_program() {
        // Table 0xa927c0 of the previous program, for indexes 36 to 51.
        let table = [
            0x2C, 0x28, 0x29, 0x2B, 0x2A, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
        ];
        for (i, &hid) in table.iter().enumerate() {
            assert_eq!(mod_tap_code(1, 36 + i) & 0xFF, hid, "index {}", 36 + i);
        }
        assert_eq!(mod_tap_code(1, 0) & 0xFF, 0x04);
        assert_eq!(mod_tap_code(1, 35) & 0xFF, 0x27);
    }

    #[test]
    fn one_shot_layer_mod_and_mod_key_match_the_firmware_and_decode_back() {
        assert_eq!(one_shot_code(0), 0x3001);
        assert_eq!(keycode::decode_keycode(one_shot_code(5)), "OSM RShift");
        // K_LM(1, Ctrl)
        assert_eq!(layer_mod_code(1, 0), 0x7011);
        assert_eq!(keycode::decode_keycode(layer_mod_code(2, 7)), "LM 2 RGUI");
        // K_MK(Ctrl, C): one press, both together.
        assert_eq!(mod_key_code(0, 2), 0x8106);
        assert_eq!(keycode::decode_keycode(mod_key_code(0, 2)), "Ctrl+C");
    }

    #[test]
    fn every_macro_and_tap_dance_of_the_firmware_has_a_key() {
        let all = catalog(&KeyboardLayout::Qwerty);
        assert_eq!(section_entries(&all, &["Macro"], "").len(), 20);
        assert_eq!(section_entries(&all, &["Tap Dance"], "").len(), 16);
        // MACRO_20 in the firmware's definitions.
        assert!(all.iter().any(|e| e.code == 0x2800 && e.name == "M19"));
        assert!(all.iter().any(|e| e.code == 0x6F00 && e.name == "TD 15"));
        assert_eq!(keycode::decode_keycode(0x6F00), "TD 15");
        assert_eq!(keycode::decode_keycode(0x2800), "M19");
    }

    #[test]
    fn a_new_item_takes_the_lowest_free_slot() {
        assert_eq!(free_slot([0u8, 2].into_iter()), 1);
        assert_eq!(free_slot(std::iter::empty()), 0);
    }

    #[test]
    fn macro_names_are_cut_on_whole_letters_within_the_firmware_limit() {
        assert_eq!(fit_macro_name("Copier"), "Copier");
        assert_eq!(fit_macro_name("0123456789abcdefgh"), "0123456789abcde");
        // é takes 2 bytes: the 15th byte would split it.
        let cut = fit_macro_name("01234567890123é");
        assert_eq!(cut, "01234567890123");
        assert!(cut.len() <= MACRO_NAME_BYTES);
    }

    #[test]
    fn layer_tap_masks_the_layer_and_decodes_back() {
        assert_eq!(layer_tap_code(1, 0), 0x412C);
        assert_eq!(keycode::decode_keycode(layer_tap_code(3, 1)), "LT 3 Enter");
        assert_eq!(layer_tap_code(0x1F, 5) & 0x0F00, 0x0F00);
    }

    #[test]
    fn catalog_lists_chest_keys_in_their_own_section_and_hides_wpm() {
        let all = catalog(&KeyboardLayout::Qwerty);
        let chest = section_entries(&all, &["Chest"], "");
        assert_eq!(chest.len(), 5);
        assert!(chest.iter().all(|e| (0x3E00..=0x3E04).contains(&e.code)));
        assert!(!all.iter().any(|e| e.code == 0x3B00));
    }

    #[test]
    fn search_filters_by_name_or_category() {
        let all = catalog(&KeyboardLayout::Qwerty);
        assert_eq!(section_entries(&all, &["Tap Dance", "Macro"], "td 3").len(), 1);
        assert_eq!(section_entries(&all, &["Tap Dance", "Macro"], "macro").len(), 20);
        assert!(section_entries(&all, &["Letter"], "zzz").is_empty());
    }

    #[test]
    fn physical_keys_map_to_hid_usages() {
        assert_eq!(hid_from_egui(egui::Key::A), Some(0x04));
        assert_eq!(hid_from_egui(egui::Key::ArrowUp), Some(0x52));
        assert_eq!(keycode::hid_key_name(hid_from_egui(egui::Key::Space).unwrap() as u8), "Space");
    }
}
