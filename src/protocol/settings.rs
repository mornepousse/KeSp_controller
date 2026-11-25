/// Persistent settings for KaSe Controller.
///
/// - **Native**: `kase_settings.json` next to the executable.
/// - **WASM**: `localStorage` under key `"kase_settings"`.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_layout")]
    pub keyboard_layout: String,
    /// Keyboard model drawn while no keyboard is connected.
    #[serde(default = "default_board")]
    pub board: String,
    /// The Practice tab's progress, kept as it writes it.
    #[serde(default)]
    pub practice: serde_json::Value,
}

fn default_board() -> String {
    "kase_v2".to_string()
}

fn default_layout() -> String {
    "QWERTY".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            keyboard_layout: default_layout(),
            board: default_board(),
            practice: serde_json::Value::Null,
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

    const FILE: &str = "kase_settings.json";

    /// Where version 2 kept the file: next to the executable. A Nix or system
    /// install puts the program in a read-only folder, where every save was lost.
    fn legacy_path() -> PathBuf {
        let exe = std::env::current_exe().unwrap_or_default();
        let parent_dir = exe.parent().unwrap_or(std::path::Path::new("."));
        parent_dir.join(FILE)
    }

    /// The user's own config folder on Linux: `$XDG_CONFIG_HOME`, else `~/.config`.
    pub(super) fn xdg_config_dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        match (xdg.filter(|x| x.starts_with('/')), home.filter(|h| !h.is_empty())) {
            (Some(x), _) => Some(PathBuf::from(x)),
            (None, Some(h)) => Some(PathBuf::from(h).join(".config")),
            _ => None,
        }
    }

    fn user_config_dir() -> Option<PathBuf> {
        let var = |k: &str| std::env::var(k).ok();
        if cfg!(windows) {
            var("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            var("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            xdg_config_dir(var("XDG_CONFIG_HOME").as_deref(), var("HOME").as_deref())
        }
    }

    /// The settings file: in the user's config folder; for tests, in a temporary one
    /// so they never touch the person's real settings.
    fn settings_path() -> PathBuf {
        let dir = if cfg!(test) {
            Some(std::env::temp_dir().join("kesp-controller-tests"))
        } else {
            user_config_dir().map(|d| d.join("kesp-controller"))
        };
        dir.map_or_else(legacy_path, |d| d.join(FILE))
    }

    pub fn load() -> Settings {
        let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok());
        // The first time, version 2's file is picked up where it was.
        let legacy = if cfg!(test) { None } else { read(legacy_path()) };
        read(settings_path()).or(legacy).unwrap_or_default()
    }

    pub fn save(settings: &Settings) {
        let Ok(json) = serde_json::to_string_pretty(settings) else { return };
        let path = settings_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, &json).is_err() && !cfg!(test) {
            let _ = std::fs::write(legacy_path(), json);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::native_settings::xdg_config_dir;
    use std::path::PathBuf;

    #[test]
    fn settings_go_to_the_users_config_folder() {
        assert_eq!(xdg_config_dir(Some("/xdg"), Some("/home/m")), Some(PathBuf::from("/xdg")));
        assert_eq!(xdg_config_dir(None, Some("/home/m")), Some(PathBuf::from("/home/m/.config")));
        // A relative XDG path is invalid by the spec: it is ignored.
        assert_eq!(xdg_config_dir(Some("rel"), Some("/home/m")), Some(PathBuf::from("/home/m/.config")));
        assert_eq!(xdg_config_dir(None, None), None);
    }

    #[test]
    fn a_saved_setting_comes_back() {
        let mut s = super::load();
        s.board = "niphargus".into();
        super::save(&s);
        assert_eq!(super::load().board, "niphargus");
    }
}
