/// Persistent settings for KaSe Controller.
///
/// - **Native**: `kase_settings.json` next to the executable.
/// - **WASM**: `localStorage` under key `"kase_settings"`.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_layout")]
    pub keyboard_layout: String,
}

fn default_layout() -> String {
    "QWERTY".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keyboard_layout: default_layout(),
        }
    }
}

// =============================================================================
// Native: JSON file next to executable
// =============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod native_settings {
    use super::Settings;
    use std::path::PathBuf;

    /// Config file path: next to the executable.
    fn settings_path() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        let parent_dir = exe.parent().unwrap_or(std::path::Path::new("."));
        parent_dir.join("kase_settings.json")
    }

    pub fn load() -> Settings {
        let path = settings_path();
        let json_content = std::fs::read_to_string(&path).ok();
        let parsed = json_content.and_then(|s| serde_json::from_str(&s).ok());
        parsed.unwrap_or_default()
    }

    pub fn save(settings: &Settings) {
        let path = settings_path();
        let json_result = serde_json::to_string_pretty(settings);
        if let Ok(json) = json_result {
            let _ = std::fs::write(path, json);
        }
    }
}

// =============================================================================
// WASM: browser localStorage
// =============================================================================

#[cfg(target_arch = "wasm32")]
mod web_settings {
    use super::Settings;

    const STORAGE_KEY: &str = "kase_settings";

    /// Get localStorage. Returns None if not in a browser context.
    fn get_storage() -> Option<web_sys::Storage> {
        let window = web_sys::window()?;
        window.local_storage().ok().flatten()
    }

    pub fn load() -> Settings {
        let storage = match get_storage() {
            Some(s) => s,
            None => return Settings::default(),
        };

        let json_option = storage.get_item(STORAGE_KEY).ok().flatten();
        let parsed = json_option.and_then(|s| serde_json::from_str(&s).ok());
        parsed.unwrap_or_default()
    }

    pub fn save(settings: &Settings) {
        let storage = match get_storage() {
            Some(s) => s,
            None => return,
        };

        let json_result = serde_json::to_string(settings);
        if let Ok(json) = json_result {
            let _ = storage.set_item(STORAGE_KEY, &json);
        }
    }
}

// =============================================================================
// Public interface
// =============================================================================

/// Load settings from persistent storage.
/// Returns `Settings::default()` if none found.
pub fn load() -> Settings {
    #[cfg(not(target_arch = "wasm32"))]
    return native_settings::load();

    #[cfg(target_arch = "wasm32")]
    return web_settings::load();
}

/// Save settings to persistent storage. Fails silently.
pub fn save(settings: &Settings) {
    #[cfg(not(target_arch = "wasm32"))]
    native_settings::save(settings);

    #[cfg(target_arch = "wasm32")]
    web_settings::save(settings);
}
