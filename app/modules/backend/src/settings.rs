//! Persistent application settings stored in `{data_dir}/{identifier}/settings.toml`.
//!
//! Settings survive across app restarts. The file is created lazily on first writing. Missing or
//! corrupted files fall back to sensible defaults without error.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::use_log;

use_log!("Settings");

/// UI-related hint counters for progressive notifications.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HintSettings {
    /// Number of times the user has seen a minimize-to-tray hint (dialogue or notification).
    #[serde(default)]
    pub minimize_to_tray: u32,
}

/// UI-related settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiSettings {
    /// Progressive hint counters.
    #[serde(default)]
    pub hints: HintSettings,
}

/// Application settings that are persisted to disk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// UI behaviour and appearance.
    #[serde(default)]
    pub ui: UiSettings,
}

/// Global in-memory copy, loaded once at startup.
static SETTINGS: Mutex<AppSettings> = Mutex::new(AppSettings {
    ui: UiSettings {
        hints: HintSettings { minimize_to_tray: 0 },
    },
});

/// Override for the settings file path, used exclusively by tests.
#[cfg(test)]
static PATH_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Platform-specific settings directory.
///
/// - macOS: `~/Library/Application Support/{identifier}/`
/// - Windows: `%APPDATA%/{identifier}/`
fn settings_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(env!("TAURI_IDENTIFIER")))
}

/// Full path to the settings file.
///
/// Combines [`settings_dir`] with the fixed filename `settings.toml`. In tests, returns the
/// override path if one was set via [`PATH_OVERRIDE`].
fn settings_path() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(p) = PATH_OVERRIDE.lock().unwrap().clone() {
        return Some(p);
    }
    Some(settings_dir()?.join("settings.toml"))
}

/// Load settings from a TOML file at the given path.
///
/// Returns [`AppSettings::default()`] when the file is missing, empty, or contains invalid TOML.
fn load_from(path: &Path) -> AppSettings {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| {
            toml::from_str::<AppSettings>(&content)
                .map_err(|e| log_warn!("Failed to parse {}: {e}", path.display()))
                .ok()
        })
        .unwrap_or_default()
}

/// Persist settings to the given path.
///
/// Creates parent directories if they do not exist. Errors are logged but not propagated.
fn save_to(path: &Path, settings: &AppSettings) {
    if let Some(dir) = path.parent() {
        if let Err(e) = fs::create_dir_all(dir) {
            log_error!("Failed to create settings directory: {e}");
            return;
        }
    }

    match toml::to_string_pretty(settings) {
        Ok(content) => {
            if let Err(e) = fs::write(path, content) {
                log_error!("Failed to write {}: {e}", path.display());
            }
        }
        Err(e) => log_error!("Failed to serialize settings: {e}"),
    }
}

/// Load settings from the disk into the global state.
///
/// Falls back to default when the file is missing, empty, or contains invalid TOML. Safe to call
/// multiple times; later calls overwrite the in-memory state.
pub fn load() {
    let settings = settings_path()
        .map(|p| load_from(&p))
        .unwrap_or_default();
    *SETTINGS.lock().unwrap() = settings;
}

/// Persist the current in-memory settings to disk.
///
/// Creates the settings directory if it does not exist. Errors are logged but not propagated 
/// because failing to save a hint counter should never crash the app.
pub fn save() {
    let settings = SETTINGS.lock().unwrap().clone();
    let Some(path) = settings_path() else {
        log_warn!("Cannot determine settings path");
        return;
    };
    save_to(&path, &settings);
}

/// Return the current minimize-to-tray hint count.
///
/// Used by [`crate::minimize_to_tray`] to decide which hint level to show.
pub fn minimize_hint_count() -> u32 {
    SETTINGS.lock().unwrap().ui.hints.minimize_to_tray
}

/// Maximum value for the minimize hint counter.
///
/// Once this threshold is reached the hint level is [`Silent`](crate::HintLevel::Silent) and
/// further increments would only waste disk writes.
const MINIMIZE_HINT_MAX: u32 = 5;

/// Increment the minimize-to-tray hint count and persist to disk.
///
/// Called after each minimize-to-tray action so the hint becomes less intrusive over time.
/// Stops incrementing (and writing) once [`MINIMIZE_HINT_MAX`] is reached.
pub fn increment_minimize_hint() {
    {
        let mut s = SETTINGS.lock().unwrap();
        if s.ui.hints.minimize_to_tray >= MINIMIZE_HINT_MAX {
            return;
        }
        s.ui.hints.minimize_to_tray += 1;
    }
    save();
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serialize tests that touch the global [`SETTINGS`] state.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Create a fresh temporary directory for a test.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("daystrom_test_settings_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Point [`settings_path`] to a temp file and return the path. Caller must hold `TEST_LOCK`.
    fn use_temp_path(name: &str) -> PathBuf {
        let path = test_dir(name).join("settings.toml");
        *PATH_OVERRIDE.lock().unwrap() = Some(path.clone());
        path
    }

    /// Reset the path override. Caller must hold `TEST_LOCK`.
    fn reset_path_override() {
        *PATH_OVERRIDE.lock().unwrap() = None;
    }

    // -- AppSettings serde --

    #[test]
    fn default_has_zero_hint_count() {
        let settings = AppSettings::default();
        assert_eq!(settings.ui.hints.minimize_to_tray, 0);
    }

    #[test]
    fn serde_round_trip() {
        let settings = AppSettings { ui: UiSettings { hints: HintSettings { minimize_to_tray: 42 } } };
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: AppSettings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.ui.hints.minimize_to_tray, 42);
    }

    #[test]
    fn deserialize_missing_fields_uses_defaults() {
        // Empty TOML table: all fields should fall back to serde defaults
        let parsed: AppSettings = toml::from_str("").unwrap();
        assert_eq!(parsed.ui.hints.minimize_to_tray, 0);
    }

    #[test]
    fn deserialize_extra_fields_ignored() {
        let toml_str = "[ui.hints]\nminimize_to_tray = 3\n\n[extra]\nunknown_field = true\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.ui.hints.minimize_to_tray, 3);
    }

    #[test]
    fn deserialize_invalid_toml_is_error() {
        let result = toml::from_str::<AppSettings>("not valid { toml }}}");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_wrong_type_is_error() {
        // minimize_to_tray expects u32, not a string
        let result = toml::from_str::<AppSettings>("[ui.hints]\nminimize_to_tray = \"hello\"");
        assert!(result.is_err());
    }

    // -- load_from --

    #[test]
    fn load_from_missing_file() {
        let dir = test_dir("load_missing");
        let result = load_from(&dir.join("nonexistent.toml"));
        assert_eq!(result, AppSettings::default());
    }

    #[test]
    fn load_from_empty_file() {
        let dir = test_dir("load_empty");
        let path = dir.join("settings.toml");
        fs::write(&path, "").unwrap();
        let result = load_from(&path);
        assert_eq!(result, AppSettings::default());
    }

    #[test]
    fn load_from_valid_file() {
        let dir = test_dir("load_valid");
        let path = dir.join("settings.toml");
        fs::write(&path, "[ui.hints]\nminimize_to_tray = 7\n").unwrap();
        let result = load_from(&path);
        assert_eq!(result.ui.hints.minimize_to_tray, 7);
    }

    #[test]
    fn load_from_invalid_file() {
        let dir = test_dir("load_invalid");
        let path = dir.join("settings.toml");
        fs::write(&path, "{{garbage}}").unwrap();
        let result = load_from(&path);
        assert_eq!(result, AppSettings::default());
    }

    // -- save_to --

    #[test]
    fn save_to_creates_file() {
        let dir = test_dir("save_create");
        let path = dir.join("settings.toml");
        let settings = AppSettings { ui: UiSettings { hints: HintSettings { minimize_to_tray: 5 } } };
        save_to(&path, &settings);

        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 5"));
    }

    #[test]
    fn save_to_creates_parent_directories() {
        let dir = test_dir("save_parents");
        let path = dir.join("nested").join("deep").join("settings.toml");
        let settings = AppSettings { ui: UiSettings { hints: HintSettings { minimize_to_tray: 1 } } };
        save_to(&path, &settings);

        assert!(path.exists());
    }

    #[test]
    fn save_then_load_round_trip() {
        let dir = test_dir("save_load_rt");
        let path = dir.join("settings.toml");
        let original = AppSettings { ui: UiSettings { hints: HintSettings { minimize_to_tray: 99 } } };
        save_to(&path, &original);
        let loaded = load_from(&path);
        assert_eq!(loaded, original);
    }

    // -- Path resolution --

    #[test]
    fn settings_dir_returns_some() {
        // dirs::data_dir() is always available on macOS and Windows
        assert!(settings_dir().is_some());
    }

    #[test]
    fn settings_path_ends_with_settings_toml() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_path_override();
        let path = settings_path().expect("settings_path should return Some");
        assert_eq!(path.file_name().unwrap(), "settings.toml");
        assert!(path.to_string_lossy().contains(env!("TAURI_IDENTIFIER")));
    }

    // -- Public API (global state + real path) --

    #[test]
    fn load_populates_global_state() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("load_global");
        fs::write(&path, "[ui.hints]\nminimize_to_tray = 7\n").unwrap();

        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 999;
        load();

        assert_eq!(minimize_hint_count(), 7, "load() should read from temp file");

        // Clean up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn minimize_hint_count_reads_global() {
        let _lock = TEST_LOCK.lock().unwrap();
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 42;

        assert_eq!(minimize_hint_count(), 42);

        // Clean up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
    }

    #[test]
    fn save_and_increment_round_trip() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("save_increment_rt");

        // Set a known state, save, verify
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 3;
        save();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 3"));

        // Increment, verify in-memory and on-disk
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), 4);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 4"));

        // Clean up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn increment_stops_at_max() {
        let _lock = TEST_LOCK.lock().unwrap();
        let _path = use_temp_path("increment_max");

        // Just below the cap: increment succeeds
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = MINIMIZE_HINT_MAX - 1;
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), MINIMIZE_HINT_MAX);

        // At the cap: increment is a no-op
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), MINIMIZE_HINT_MAX);

        // Well above the cap (e.g. from an old settings file): still a no-op
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 100;
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), 100);

        // Clean up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }
}
