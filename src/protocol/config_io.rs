/// Import/export keyboard configuration as JSON.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct KeyboardConfig {
    pub version: u32,
    pub layer_names: Vec<String>,
    /// keymaps[layer][row][col] = keycode (u16)
    pub keymaps: Vec<Vec<Vec<u16>>>,
    pub tap_dances: Vec<TdConfig>,
    pub combos: Vec<ComboConfig>,
    pub key_overrides: Vec<KoConfig>,
    pub leaders: Vec<LeaderConfig>,
    pub macros: Vec<MacroConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct TdConfig {
    pub index: u8,
    pub actions: [u16; 4],
}

#[derive(Serialize, Deserialize)]
pub struct ComboConfig {
    pub index: u8,
    pub r1: u8,
    pub c1: u8,
    pub r2: u8,
    pub c2: u8,
    pub result: u16,
}

#[derive(Serialize, Deserialize)]
pub struct KoConfig {
    pub trigger_key: u8,
    pub trigger_mod: u8,
    pub result_key: u8,
    pub result_mod: u8,
}

#[derive(Serialize, Deserialize)]
pub struct LeaderConfig {
    pub index: u8,
    pub sequence: Vec<u8>,
    pub result: u8,
    pub result_mod: u8,
}

#[derive(Serialize, Deserialize)]
pub struct MacroConfig {
    pub slot: u8,
    pub name: String,
    /// Steps as "kc:mod,kc:mod,..." hex string
    pub steps: String,
    /// Keys pressed together. Absent from files saved by older versions.
    #[serde(default)]
    pub keys: Vec<u8>,
}

impl KeyboardConfig {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_macro_of_keys_survives_a_save_and_reload() {
        let c = KeyboardConfig {
            version: 1,
            layer_names: vec![],
            keymaps: vec![],
            tap_dances: vec![],
            combos: vec![],
            key_overrides: vec![],
            leaders: vec![],
            macros: vec![MacroConfig { slot: 0, name: "Copier".into(), steps: String::new(), keys: vec![0xE0, 0x06] }],
        };
        let back = KeyboardConfig::from_json(&c.to_json().unwrap()).unwrap();
        assert_eq!(back.macros[0].keys, vec![0xE0, 0x06]);
    }

    #[test]
    fn a_file_from_an_older_version_still_loads() {
        let old = r#"{"version":1,"layer_names":[],"keymaps":[],"tap_dances":[],"combos":[],
            "key_overrides":[],"leaders":[],"macros":[{"slot":0,"name":"m","steps":"06:00"}]}"#;
        let c = KeyboardConfig::from_json(old).unwrap();
        assert!(c.macros[0].keys.is_empty());
    }
}
