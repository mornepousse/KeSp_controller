#![allow(dead_code)]
/// CDC protocol command helpers for KaSe keyboard firmware.

// Text-based query commands
pub const CMD_TAP_DANCE: &str = "TD?";
pub const CMD_COMBOS: &str = "COMBO?";
pub const CMD_LEADER: &str = "LEADER?";
pub const CMD_KEY_OVERRIDE: &str = "KO?";
pub const CMD_BT_STATUS: &str = "BT?";
pub const CMD_WPM: &str = "WPM?";
pub const CMD_TAMA: &str = "TAMA?";
pub const CMD_MACROS_TEXT: &str = "MACROS?";
pub const CMD_FEATURES: &str = "FEATURES?";
pub const CMD_KEYSTATS: &str = "KEYSTATS?";
pub const CMD_BIGRAMS: &str = "BIGRAMS?";

pub fn cmd_set_key(layer: u8, row: u8, col: u8, keycode: u16) -> String {
    format!("SETKEY {},{},{},{:04X}", layer, row, col, keycode)
}

pub fn cmd_set_layer_name(layer: u8, name: &str) -> String {
    format!("LAYOUTNAME{}:{}", layer, name)
}

pub fn cmd_bt_switch(slot: u8) -> String {
    format!("BT SWITCH {}", slot)
}

pub fn cmd_trilayer(l1: u8, l2: u8, l3: u8) -> String {
    format!("TRILAYER {},{},{}", l1, l2, l3)
}

pub fn cmd_macroseq(slot: u8, name: &str, steps: &str) -> String {
    format!("MACROSEQ {};{};{}", slot, name, steps)
}

pub fn cmd_macro_del(slot: u8) -> String {
    format!("MACRODEL {}", slot)
}

pub fn cmd_comboset(index: u8, r1: u8, c1: u8, r2: u8, c2: u8, result: u8) -> String {
    format!("COMBOSET {};{},{},{},{},{:02X}", index, r1, c1, r2, c2, result)
}

pub fn cmd_combodel(index: u8) -> String {
    format!("COMBODEL {}", index)
}

pub fn cmd_koset(index: u8, trig_key: u8, trig_mod: u8, res_key: u8, res_mod: u8) -> String {
    format!("KOSET {};{:02X},{:02X},{:02X},{:02X}", index, trig_key, trig_mod, res_key, res_mod)
}

pub fn cmd_kodel(index: u8) -> String {
    format!("KODEL {}", index)
}

pub fn cmd_leaderset(index: u8, sequence: &[u8], result: u8, result_mod: u8) -> String {
    let seq_hex: Vec<String> = sequence.iter().map(|k| format!("{:02X}", k)).collect();
    let seq_str = seq_hex.join(",");
    format!("LEADERSET {};{};{:02X},{:02X}", index, seq_str, result, result_mod)
}

pub fn cmd_leaderdel(index: u8) -> String {
    format!("LEADERDEL {}", index)
}
