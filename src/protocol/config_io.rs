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
}

impl KeyboardConfig {
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}
