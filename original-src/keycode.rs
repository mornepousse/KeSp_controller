/// Decode a raw 16-bit keycode into a human-readable string.
///
/// Covers all KaSe firmware keycode ranges: HID basic keys, layer switches,
/// macros, Bluetooth, one-shot, mod-tap, layer-tap, tap-dance, and more.
pub fn decode_keycode(raw: u16) -> String {
    // --- HID basic keycodes 0x00..=0xE7 ---
    if raw <= 0x00E7 {
        return hid_key_name(raw as u8);
    }

    // --- MO (Momentary Layer): 0x0100..=0x0A00, low byte == 0 ---
    if raw >= 0x0100 && raw <= 0x0A00 && (raw & 0xFF) == 0 {
        let layer = (raw >> 8) - 1;
        return format!("MO {layer}");
    }

    // --- TO (Toggle Layer): 0x0B00..=0x1400, low byte == 0 ---
    if raw >= 0x0B00 && raw <= 0x1400 && (raw & 0xFF) == 0 {
        let layer = (raw >> 8) - 0x0B;
        return format!("TO {layer}");
    }

    // --- MACRO: 0x1500..=0x2800, low byte == 0 ---
    if raw >= 0x1500 && raw <= 0x2800 && (raw & 0xFF) == 0 {
        let idx = (raw >> 8) - 0x14;
        return format!("M{idx}");
    }

    // --- BT keycodes ---
    match raw {
        0x2900 => return "BT Next".into(),
        0x2A00 => return "BT Prev".into(),
        0x2B00 => return "BT Pair".into(),
        0x2C00 => return "BT Disc".into(),
        0x2E00 => return "USB/BT".into(),
        0x2F00 => return "BT On/Off".into(),
        _ => {}
    }

    // --- OSM (One-Shot Mod): 0x3000..=0x30FF ---
    if raw >= 0x3000 && raw <= 0x30FF {
        let mods = (raw & 0xFF) as u8;
        return format!("OSM {}", mod_name(mods));
    }

    // --- OSL (One-Shot Layer): 0x3100..=0x310F ---
    if raw >= 0x3100 && raw <= 0x310F {
        let layer = raw & 0x0F;
        return format!("OSL {layer}");
    }

    // --- Fixed special codes ---
    match raw {
        0x3200 => return "Caps Word".into(),
        0x3300 => return "Repeat".into(),
        0x3400 => return "Leader".into(),
        0x3500 => return "Feed".into(),
        0x3600 => return "Play".into(),
        0x3700 => return "Sleep".into(),
        0x3800 => return "Meds".into(),
        0x3900 => return "GEsc".into(),
        0x3A00 => return "Layer Lock".into(),
        0x3C00 => return "AS Toggle".into(),
        _ => {}
    }

    // --- KO (Key Override) slots: 0x3D00..=0x3DFF ---
    if raw >= 0x3D00 && raw <= 0x3DFF {
        let slot = raw & 0xFF;
        return format!("KO {slot}");
    }

    // --- LT (Layer-Tap): 0x4000..=0x4FFF ---
    //     layout: 0x4LKK  where L = layer (0..F), KK = HID keycode
    if raw >= 0x4000 && raw <= 0x4FFF {
        let layer = (raw >> 8) & 0x0F;
        let kc = (raw & 0xFF) as u8;
        return format!("LT {} {}", layer, hid_key_name(kc));
    }

    // --- MT (Mod-Tap): 0x5000..=0x5FFF ---
    //     layout: 0x5MKK  where M = mod nibble (4 bits), KK = HID keycode
    if raw >= 0x5000 && raw <= 0x5FFF {
        let mods = ((raw >> 8) & 0x0F) as u8;
        let kc = (raw & 0xFF) as u8;
        return format!("MT {} {}", mod_name(mods), hid_key_name(kc));
    }

    // --- TD (Tap Dance): 0x6000..=0x6FFF ---
    if raw >= 0x6000 && raw <= 0x6FFF {
        let index = (raw >> 8) & 0x0F;
        return format!("TD {index}");
    }

    // --- Unknown ---
    format!("0x{raw:04X}")
}

/// Decode a modifier bitmask into a human-readable string.
///
/// Bits: 0x01=Ctrl, 0x02=Shift, 0x04=Alt, 0x08=GUI,
///       0x10=RCtrl, 0x20=RShift, 0x40=RAlt, 0x80=RGUI.
/// Multiple modifiers are joined with "+".
pub fn mod_name(mod_mask: u8) -> String {
    let mut parts = Vec::new();
    if mod_mask & 0x01 != 0 { parts.push("Ctrl"); }
    if mod_mask & 0x02 != 0 { parts.push("Shift"); }
    if mod_mask & 0x04 != 0 { parts.push("Alt"); }
    if mod_mask & 0x08 != 0 { parts.push("GUI"); }
    if mod_mask & 0x10 != 0 { parts.push("RCtrl"); }
    if mod_mask & 0x20 != 0 { parts.push("RShift"); }
    if mod_mask & 0x40 != 0 { parts.push("RAlt"); }
    if mod_mask & 0x80 != 0 { parts.push("RGUI"); }
    if parts.is_empty() {
        format!("0x{mod_mask:02X}")
    } else {
        parts.join("+")
    }
}

/// Map a single HID usage code (0x00..=0xE7) to a short readable name.
pub fn hid_key_name(code: u8) -> String {
    match code {
        // No key / transparent
        0x00 => "None",
        // 0x01 = ErrorRollOver, 0x02 = POSTFail, 0x03 = ErrorUndefined (not user-facing)
        0x01 => "ErrRollOver",
        0x02 => "POSTFail",
        0x03 => "ErrUndef",

        // Letters
        0x04 => "A",
        0x05 => "B",
        0x06 => "C",
        0x07 => "D",
        0x08 => "E",
        0x09 => "F",
        0x0A => "G",
        0x0B => "H",
        0x0C => "I",
        0x0D => "J",
        0x0E => "K",
        0x0F => "L",
        0x10 => "M",
        0x11 => "N",
        0x12 => "O",
        0x13 => "P",
        0x14 => "Q",
        0x15 => "R",
        0x16 => "S",
        0x17 => "T",
        0x18 => "U",
        0x19 => "V",
        0x1A => "W",
        0x1B => "X",
        0x1C => "Y",
        0x1D => "Z",

        // Number row
        0x1E => "1",
        0x1F => "2",
        0x20 => "3",
        0x21 => "4",
        0x22 => "5",
        0x23 => "6",
        0x24 => "7",
        0x25 => "8",
        0x26 => "9",
        0x27 => "0",

        // Common control keys
        0x28 => "Enter",
        0x29 => "Esc",
        0x2A => "Backspace",
        0x2B => "Tab",
        0x2C => "Space",

        // Punctuation / symbols
        0x2D => "-",
        0x2E => "=",
        0x2F => "[",
        0x30 => "]",
        0x31 => "\\",
        0x32 => "Europe1",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",

        // Caps Lock
        0x39 => "Caps Lock",

        // Function keys
        0x3A => "F1",
        0x3B => "F2",
        0x3C => "F3",
        0x3D => "F4",
        0x3E => "F5",
        0x3F => "F6",
        0x40 => "F7",
        0x41 => "F8",
        0x42 => "F9",
        0x43 => "F10",
        0x44 => "F11",
        0x45 => "F12",

        // Navigation / editing cluster
        0x46 => "PrtSc",
        0x47 => "ScrLk",
        0x48 => "Pause",
        0x49 => "Ins",
        0x4A => "Home",
        0x4B => "PgUp",
        0x4C => "Del",
        0x4D => "End",
        0x4E => "PgDn",

        // Arrow keys
        0x4F => "Right",
        0x50 => "Left",
        0x51 => "Down",
        0x52 => "Up",

        // Keypad
        0x53 => "NumLk",
        0x54 => "Num /",
        0x55 => "Num *",
        0x56 => "Num -",
        0x57 => "Num +",
        0x58 => "Num Enter",
        0x59 => "Num 1",
        0x5A => "Num 2",
        0x5B => "Num 3",
        0x5C => "Num 4",
        0x5D => "Num 5",
        0x5E => "Num 6",
        0x5F => "Num 7",
        0x60 => "Num 8",
        0x61 => "Num 9",
        0x62 => "Num 0",
        0x63 => "Num .",
        0x64 => "Europe2",
        0x65 => "Menu",
        0x66 => "Power",
        0x67 => "Num =",

        // F13-F24
        0x68 => "F13",
        0x69 => "F14",
        0x6A => "F15",
        0x6B => "F16",
        0x6C => "F17",
        0x6D => "F18",
        0x6E => "F19",
        0x6F => "F20",
        0x70 => "F21",
        0x71 => "F22",
        0x72 => "F23",
        0x73 => "F24",

        // Misc system keys
        0x74 => "Execute",
        0x75 => "Help",
        0x76 => "Menu2",
        0x77 => "Select",
        0x78 => "Stop",
        0x79 => "Again",
        0x7A => "Undo",
        0x7B => "Cut",
        0x7C => "Copy",
        0x7D => "Paste",
        0x7E => "Find",
        0x7F => "Mute",
        0x80 => "Vol Up",
        0x81 => "Vol Down",

        // Locking keys
        0x82 => "Lock Caps",
        0x83 => "Lock Num",
        0x84 => "Lock Scroll",

        // Keypad extras
        0x85 => "Num ,",
        0x86 => "Num =2",

        // International / Kanji
        0x87 => "Kanji1",
        0x88 => "Kanji2",
        0x89 => "Kanji3",
        0x8A => "Kanji4",
        0x8B => "Kanji5",
        0x8C => "Kanji6",
        0x8D => "Kanji7",
        0x8E => "Kanji8",
        0x8F => "Kanji9",

        // Language keys
        0x90 => "Lang1",
        0x91 => "Lang2",
        0x92 => "Lang3",
        0x93 => "Lang4",
        0x94 => "Lang5",
        0x95 => "Lang6",
        0x96 => "Lang7",
        0x97 => "Lang8",
        0x98 => "Lang9",

        // Rare system keys
        0x99 => "Alt Erase",
        0x9A => "SysReq",
        0x9B => "Cancel",
        0x9C => "Clear",
        0x9D => "Prior",
        0x9E => "Return",
        0x9F => "Separator",
        0xA0 => "Out",
        0xA1 => "Oper",
        0xA2 => "Clear Again",
        0xA3 => "CrSel",
        0xA4 => "ExSel",

        // 0xA5..=0xAF reserved / not defined in standard HID tables

        // Extended keypad
        0xB0 => "Num 00",
        0xB1 => "Num 000",
        0xB2 => "Thousands Sep",
        0xB3 => "Decimal Sep",
        0xB4 => "Currency",
        0xB5 => "Currency Sub",
        0xB6 => "Num (",
        0xB7 => "Num )",
        0xB8 => "Num {",
        0xB9 => "Num }",
        0xBA => "Num Tab",
        0xBB => "Num Bksp",
        0xBC => "Num A",
        0xBD => "Num B",
        0xBE => "Num C",
        0xBF => "Num D",
        0xC0 => "Num E",
        0xC1 => "Num F",
        0xC2 => "Num XOR",
        0xC3 => "Num ^",
        0xC4 => "Num %",
        0xC5 => "Num <",
        0xC6 => "Num >",
        0xC7 => "Num &",
        0xC8 => "Num &&",
        0xC9 => "Num |",
        0xCA => "Num ||",
        0xCB => "Num :",
        0xCC => "Num #",
        0xCD => "Num Space",
        0xCE => "Num @",
        0xCF => "Num !",
        0xD0 => "Num M Store",
        0xD1 => "Num M Recall",
        0xD2 => "Num M Clear",
        0xD3 => "Num M+",
        0xD4 => "Num M-",
        0xD5 => "Num M*",
        0xD6 => "Num M/",
        0xD7 => "Num +/-",
        0xD8 => "Num Clear",
        0xD9 => "Num ClrEntry",
        0xDA => "Num Binary",
        0xDB => "Num Octal",
        0xDC => "Num Decimal",
        0xDD => "Num Hex",

        // 0xDE..=0xDF reserved

        // Modifier keys
        0xE0 => "LCtrl",
        0xE1 => "LShift",
        0xE2 => "LAlt",
        0xE3 => "LGUI",
        0xE4 => "RCtrl",
        0xE5 => "RShift",
        0xE6 => "RAlt",
        0xE7 => "RGUI",

        // Anything else in 0x00..=0xFF not covered above
        _ => return format!("0x{code:02X}"),
    }
    .into()
}
