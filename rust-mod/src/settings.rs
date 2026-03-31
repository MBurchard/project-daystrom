//! Game settings received from Daystrom.
//!
//! The mod keeps a local copy of the settings with the same defaults as Daystrom.
//! On WebSocket connection, the mod requests a full sync (`settings.sync`).
//! Incremental updates (`settings.update`) patch individual fields afterwards.

use std::sync::{Mutex, OnceLock};

use log::debug;
use serde::{Deserialize, Deserializer};

// ---- Lenient deserialisation -----------------------------------------------

/// Deserialize an `Option<T>` that returns `None` for type mismatches instead of failing.
fn lenient_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Ok(T::deserialize(deserializer).ok())
}

// ---- Data model ------------------------------------------------------------

/// UI settings that control in-game appearance.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GameUiSettings {
    /// UI scale percentage (50-200). Applied as a multiplier on the original scale factor.
    #[serde(default, deserialize_with = "lenient_option")]
    pub scale: Option<u32>,
    /// Whether to auto-open the chat sidebar on game start.
    #[serde(default, deserialize_with = "lenient_option")]
    pub auto_open_sidebar: Option<bool>,
    /// Whether to auto-expand the job queue panel from compact to full view.
    #[serde(default, deserialize_with = "lenient_option")]
    pub auto_expand_job_queue: Option<bool>,
    /// Whether to suppress all toast banner notifications.
    #[serde(default, deserialize_with = "lenient_option")]
    pub disable_all_banners: Option<bool>,
    /// List of specific banner type names to suppress (e.g. `["Victory", "Defeat"]`).
    #[serde(default, deserialize_with = "lenient_option")]
    pub disabled_banner_types: Option<Vec<String>>,
}

/// Game settings received from Daystrom.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GameSettings {
    /// In-game UI appearance.
    #[serde(default)]
    pub ui: GameUiSettings,
}

// ---- Global state ----------------------------------------------------------

/// Global settings state, lazily initialized with defaults.
static SETTINGS: OnceLock<Mutex<GameSettings>> = OnceLock::new();

/// Access the global settings, initializing with defaults on the first call.
fn state() -> &'static Mutex<GameSettings> {
    SETTINGS.get_or_init(|| Mutex::new(GameSettings::default()))
}

// ---- Public API ------------------------------------------------------------

/// Current UI scale percentage (50-200, default 100).
pub fn get_scale() -> u32 {
    state().lock().unwrap().ui.scale.unwrap_or(100)
}

/// Whether the chat sidebar should be auto-opened on game start.
pub fn auto_open_sidebar() -> bool {
    state().lock().unwrap().ui.auto_open_sidebar.unwrap_or(false)
}

/// Whether the job queue panel should be auto-expanded from compact to full view.
pub fn auto_expand_job_queue() -> bool {
    state().lock().unwrap().ui.auto_expand_job_queue.unwrap_or(false)
}

/// Whether all toast banner notifications should be suppressed.
pub fn disable_all_banners() -> bool {
    state().lock().unwrap().ui.disable_all_banners.unwrap_or(false)
}

/// List of specific banner type names to suppress.
pub fn disabled_banner_types() -> Vec<String> {
    state().lock().unwrap().ui.disabled_banner_types.clone().unwrap_or_default()
}

/// Replace all settings with a full snapshot from Daystrom (`settings.sync`).
pub fn apply_sync(settings: GameSettings) {
    debug!(target: "Settings", "Sync: {settings:?}");
    // Scoped block: release the Mutex before side-effect hooks,
    // which call getters and would deadlock on the same lock.
    { *state().lock().unwrap() = settings; }
    crate::hooks::ui_scale::apply_current_scale();
    crate::hooks::chat_frame::on_settings_synced();
    crate::hooks::job_queue::on_settings_synced();
    crate::hooks::toast_banner::on_settings_changed();
}

/// Patch individual settings from an incremental update (`settings.update`).
///
/// Keys use the same dotted notation as Daystrom's [`SettingsEvent::key`] (e.g. `game.ui.scale`).
pub fn apply_update(key: &str, value: &serde_json::Value) {
    let mut s = state().lock().unwrap();
    match key {
        "game.ui.scale" => {
            let new_scale = value.as_u64().map(|v| v as u32);
            debug!(target: "Settings", "Update: game.ui.scale = {new_scale:?}");
            s.ui.scale = new_scale;
            // Release the Mutex before apply_current_scale(),
            // which calls get_scale() and would deadlock on the same lock.
            drop(s);
            crate::hooks::ui_scale::apply_current_scale();
        }
        "game.ui.auto_open_sidebar" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.ui.auto_open_sidebar = {new_val:?}");
            s.ui.auto_open_sidebar = new_val;
        }
        "game.ui.auto_expand_job_queue" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.ui.auto_expand_job_queue = {new_val:?}");
            s.ui.auto_expand_job_queue = new_val;
        }
        "game.ui.disable_all_banners" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.ui.disable_all_banners = {new_val:?}");
            s.ui.disable_all_banners = new_val;
            drop(s);
            crate::hooks::toast_banner::on_settings_changed();
        }
        "game.ui.disabled_banner_types" => {
            let new_val = value.as_array().map(|arr| {
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            });
            debug!(target: "Settings", "Update: game.ui.disabled_banner_types = {new_val:?}");
            s.ui.disabled_banner_types = new_val;
            drop(s);
            crate::hooks::toast_banner::on_settings_changed();
        }
        other => {
            debug!(target: "Settings", "Unknown setting: {other}");
        }
    }
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that share the global SETTINGS state.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Reset state to defaults before each test.
    fn reset() {
        *state().lock().unwrap() = GameSettings::default();
    }

    #[test]
    fn default_scale_is_none() {
        assert_eq!(GameSettings::default().ui.scale, None);
    }

    #[test]
    fn apply_sync_replaces_state() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        let settings = GameSettings {
            ui: GameUiSettings { scale: Some(150), ..Default::default() },
        };
        apply_sync(settings.clone());

        assert_eq!(state().lock().unwrap().clone(), settings);
    }

    #[test]
    fn apply_update_patches_scale() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.scale", &serde_json::json!(75));
        assert_eq!(state().lock().unwrap().ui.scale, Some(75));
    }

    #[test]
    fn apply_update_ignores_unknown_key() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.nonexistent", &serde_json::json!(42));
        assert_eq!(state().lock().unwrap().clone(), GameSettings::default());
    }

    #[test]
    fn apply_update_ignores_wrong_type() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.scale", &serde_json::json!("not a number"));
        assert_eq!(state().lock().unwrap().ui.scale, None);
    }

    #[test]
    fn deserialize_from_json() {
        let json = r#"{"ui": {"scale": 125}}"#;
        let settings: GameSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.ui.scale, Some(125));
    }

    #[test]
    fn deserialize_missing_fields_uses_defaults() {
        let settings: GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.ui.scale, None);
        assert_eq!(settings.ui.auto_open_sidebar, None);
    }

    #[test]
    fn auto_open_sidebar_defaults_to_none() {
        assert_eq!(GameUiSettings::default().auto_open_sidebar, None);
    }

    #[test]
    fn auto_open_sidebar_getter() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert!(!auto_open_sidebar());
        state().lock().unwrap().ui.auto_open_sidebar = Some(true);
        assert!(auto_open_sidebar());

        reset();
    }

    #[test]
    fn apply_update_patches_auto_open_sidebar() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.auto_open_sidebar", &serde_json::json!(true));
        assert_eq!(state().lock().unwrap().ui.auto_open_sidebar, Some(true));

        reset();
    }
}
