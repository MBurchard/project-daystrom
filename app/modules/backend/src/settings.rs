//! Persistent application settings stored in `{data_dir}/{identifier}/settings.toml`.
//!
//! Settings survive across app restarts. The file is created lazily on first writing.
//! Missing or corrupted files fall back to sensible defaults without error.
//!
//! When a setting changes, a [`SettingsEvent`] is broadcast to all subscribers via a [`broadcast`] channel.
//! Other modules (e.g. WebSocket) can subscribe to react to changes without coupling.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use ts_rs::TS;

use crate::use_log;

use_log!("Settings");

// ---- Settings events ------------------------------------------------------------

/// Event emitted when a setting changes.
///
/// Subscribers (e.g. the WebSocket bridge) receive these via [`subscribe`] and can react without
/// direct coupling to the settings module. Each variant carries the new value and can describe
/// itself via [`key`](Self::key) and [`value`](Self::value) for generic forwarding.
#[derive(Clone, Debug)]
pub enum SettingsEvent {
    /// The in-game UI scale percentage changed.
    GameUiScale(u32),
}

impl SettingsEvent {
    /// Dotted key path identifying the setting (e.g. `game.ui.scale`).
    pub fn key(&self) -> &'static str {
        match self {
            Self::GameUiScale(_) => "game.ui.scale",
        }
    }

    /// The new value as a JSON value.
    pub fn value(&self) -> serde_json::Value {
        match self {
            Self::GameUiScale(scale) => serde_json::json!(scale),
        }
    }
}

/// Broadcast channel for settings change notifications.
static EVENT_TX: std::sync::OnceLock<broadcast::Sender<SettingsEvent>> = std::sync::OnceLock::new();

/// Get or initialize the broadcast sender.
fn event_tx() -> &'static broadcast::Sender<SettingsEvent> {
    EVENT_TX.get_or_init(|| broadcast::channel(16).0)
}

/// Subscribe to settings change events.
///
/// Returns a receiver that yields [`SettingsEvent`] values whenever a setting is modified.
pub fn subscribe() -> broadcast::Receiver<SettingsEvent> {
    event_tx().subscribe()
}

// ---- Data model -----------------------------------------------------------------

/// UI-related hint counters for progressive notifications.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HintSettings {
    /// Number of times the user has seen a minimize-to-tray hint (dialogue or notification).
    #[serde(default)]
    pub minimize_to_tray: u32,
}

/// UI-related settings for the Daystrom app itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiSettings {
    /// Progressive hint counters.
    #[serde(default)]
    pub hints: HintSettings,
}

/// UI settings that are sent to the game mod via WebSocket.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameUiSettings {
    /// UI scale percentage (50–200). Applied as a multiplier on the original scale factor.
    #[serde(default = "default_scale")]
    pub scale: u32,
}

/// Default UI scale: 100% (no change).
const fn default_scale() -> u32 {
    100
}

impl Default for GameUiSettings {
    fn default() -> Self {
        Self { scale: default_scale() }
    }
}

/// Game-related settings that are sent to the mod.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameSettings {
    /// In-game UI appearance.
    #[serde(default)]
    pub ui: GameUiSettings,
}

/// Application settings that are persisted to disk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Daystrom app UI behaviour and appearance.
    #[serde(default)]
    pub ui: UiSettings,
    /// Game mod settings (sent via WebSocket).
    #[serde(default)]
    pub game: GameSettings,
}

/// Global in-memory copy, loaded once at startup.
static SETTINGS: Mutex<AppSettings> = Mutex::new(AppSettings {
    ui: UiSettings {
        hints: HintSettings { minimize_to_tray: 0 },
    },
    game: GameSettings {
        ui: GameUiSettings { scale: 100 },
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

/// Whether a debounced save is already scheduled.
static SAVE_PENDING: AtomicBool = AtomicBool::new(false);

/// Delay before a debounced save writes to disk.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Persist the current in-memory settings to disk (debounced).
///
/// If no save is pending, spawns a background thread that waits [`SAVE_DEBOUNCE`] and then writes the current
/// in-memory settings. Multiple calls within the debounce window are coalesced into a single write.
/// This avoids flooding the disk when a slider generates rapid updates.
fn save() {
    if SAVE_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(SAVE_DEBOUNCE);
        SAVE_PENDING.store(false, Ordering::SeqCst);
        let settings = SETTINGS.lock().unwrap().clone();
        let Some(path) = settings_path() else {
            log_warn!("Cannot determine settings path");
            return;
        };
        if let Some(dir) = path.parent() {
            if let Err(e) = fs::create_dir_all(dir) {
                log_error!("Failed to create settings directory: {e}");
                return;
            }
        }
        match toml::to_string_pretty(&settings) {
            Ok(content) => {
                if let Err(e) = fs::write(&path, content) {
                    log_error!("Failed to write {}: {e}", path.display());
                }
            }
            Err(e) => log_error!("Failed to serialise settings: {e}"),
        }
    });
}

/// Mutate the in-memory settings and schedule a debounced save to disk.
///
/// The closure receives a mutable reference to the settings. If it returns `true`, a save is scheduled.
/// If `false` (e.g. no actual change), the disk write is skipped. Returns whether the closure reported a change.
fn update(f: impl FnOnce(&mut AppSettings) -> bool) -> bool {
    let changed = f(&mut SETTINGS.lock().unwrap());
    if changed {
        save();
    }
    changed
}

// ---- Hint settings API ----------------------------------------------------------

/// Maximum value for the minimize hint counter.
///
/// Once this threshold is reached, the hint level is [`Silent`](crate::HintLevel::Silent) and
/// further increments would only waste disk writes.
const MINIMIZE_HINT_MAX: u32 = 5;

/// Return the current minimize-to-tray hint count.
///
/// Used by [`crate::minimize_to_tray`] to decide which hint level to show.
pub fn minimize_hint_count() -> u32 {
    SETTINGS.lock().unwrap().ui.hints.minimize_to_tray
}

/// Increment the minimize-to-tray hint count and persist to disk.
///
/// Called after each minimize-to-tray action, so the hint becomes less intrusive over time.
/// Stops incrementing (and writing) once [`MINIMIZE_HINT_MAX`] is reached.
pub fn increment_minimize_hint() {
    update(|s| {
        if s.ui.hints.minimize_to_tray >= MINIMIZE_HINT_MAX {
            return false;
        }
        s.ui.hints.minimize_to_tray += 1;
        true
    });
}

// ---- Game settings API ----------------------------------------------------------

/// Return the current game settings.
#[tauri::command]
pub fn get_game_settings() -> GameSettings {
    SETTINGS.lock().unwrap().game.clone()
}

/// Apply new game settings, diff against the current state, and emit events for changes.
///
/// The in-memory state is updated immediately under a single lock to avoid race conditions.
/// The disk write is debounced so rapid slider movements do not flood the filesystem.
/// Only changed fields trigger [`SettingsEvent`] emissions.
#[tauri::command]
pub fn set_game_settings(settings: GameSettings) {
    let events = {
        let mut s = SETTINGS.lock().unwrap();
        if s.game == settings {
            return;
        }
        let old = s.game.clone();
        s.game = settings.clone();

        let mut events = Vec::new();
        if settings.ui.scale != old.ui.scale {
            events.push(SettingsEvent::GameUiScale(settings.ui.scale));
        }
        events
    };

    save();
    for event in &events {
        let _ = event_tx().send(event.clone());
    }
    if log::log_enabled!(log::Level::Debug) {
        for event in &events {
            match event {
                SettingsEvent::GameUiScale(scale) => log_debug!("UI scale set to {scale}%"),
            }
        }
    }
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
        let settings = AppSettings { ui: UiSettings { hints: HintSettings { minimize_to_tray: 42 } }, ..Default::default() };
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

    // -- save --

    #[test]
    fn save_creates_file() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("save_create");

        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 5;
        save();
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));

        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 5"));

        // Clean up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn save_creates_parent_directories() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = test_dir("save_parents");
        let path = dir.join("nested").join("deep").join("settings.toml");
        *PATH_OVERRIDE.lock().unwrap() = Some(path.clone());

        save();
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));

        assert!(path.exists());

        // Clean up
        reset_path_override();
    }

    #[test]
    fn save_then_load_round_trip() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("save_load_rt");

        let original = AppSettings {
            ui: UiSettings { hints: HintSettings { minimize_to_tray: 99 } },
            ..Default::default()
        };
        *SETTINGS.lock().unwrap() = original.clone();
        save();
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));

        let loaded = load_from(&path);
        assert_eq!(loaded, original);

        // Clean up
        *SETTINGS.lock().unwrap() = AppSettings::default();
        reset_path_override();
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

        // Set a known state, save (debounced), wait for flush
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 3;
        save();
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 3"));

        // Increment (also debounced), verify in-memory immediately, on-disk after flush
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), 4);
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));
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

    // -- Game UI settings --

    #[test]
    fn default_ui_scale_is_100() {
        assert_eq!(GameUiSettings::default().scale, 100);
    }

    #[test]
    fn game_settings_serde_round_trip() {
        let toml_str = "[game.ui]\nscale = 150\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.ui.scale, 150);

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("scale = 150"));
    }

    #[test]
    fn game_settings_missing_uses_defaults() {
        let parsed: AppSettings = toml::from_str("[ui.hints]\nminimize_to_tray = 2\n").unwrap();
        assert_eq!(parsed.game.ui.scale, 100);
    }

    #[test]
    fn set_game_settings_updates_and_persists() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("game_set_persist");

        let mut new_settings = get_game_settings();
        new_settings.ui.scale = 75;
        set_game_settings(new_settings);

        // Not yet on disk (debounced)
        assert!(!path.exists(), "save should be debounced, not immediate");

        // Wait for debounce to flush
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("scale = 75"));
        assert_eq!(get_game_settings().ui.scale, 75);

        // Clean up
        SETTINGS.lock().unwrap().game = GameSettings::default();
        reset_path_override();
    }

    #[test]
    fn set_game_settings_unchanged_skips_save() {
        let _lock = TEST_LOCK.lock().unwrap();
        let _path = use_temp_path("game_set_noop");

        let current = get_game_settings();
        set_game_settings(current);

        // No save should be scheduled for identical settings
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));
        assert!(!_path.exists(), "no write expected when nothing changed");

        // Clean up
        reset_path_override();
    }

    #[test]
    fn set_game_settings_debounce_coalesces_rapid_changes() {
        let _lock = TEST_LOCK.lock().unwrap();
        let path = use_temp_path("game_set_coalesce");

        // Rapid slider movement: many values in quick succession
        for scale in [60, 70, 80, 90] {
            set_game_settings(GameSettings { ui: GameUiSettings { scale } });
        }

        // Wait for debounce to flush
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(100));
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("scale = 90"));

        // Clean up
        SETTINGS.lock().unwrap().game = GameSettings::default();
        reset_path_override();
    }
}
