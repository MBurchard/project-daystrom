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
///
/// Standard serde fails the entire document when a field has the wrong type (e.g. `scale = "hello"` when `u32`
/// is expected). This deserializer catches the error and returns `None`, so one corrupt field does not destroy the
/// rest of the settings.
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

/// Replace all settings with a full snapshot from Daystrom (`settings.sync`).
pub fn apply_sync(settings: GameSettings) {
    debug!(target: "Settings", "Sync: {settings:?}");
    // Scoped block: release the Mutex before apply_current_scale(),
    // which calls get_scale() and would deadlock on the same lock.
    { *state().lock().unwrap() = settings; }
    crate::hooks::ui_scale::apply_current_scale();
}

/// Patch individual settings from an incremental update (`settings.update`).
///
/// Keys use the same dotted notation as Daystrom's [`SettingsEvent::key`] (e.g. `game.ui.scale`).
pub fn apply_update(key: &str, value: &serde_json::Value) {
    let mut s = state().lock().unwrap();
    match key {
        "game.ui.scale" => {
            if let Some(scale) = value.as_u64().map(|v| v as u32) {
                debug!(target: "Settings", "Update: game.ui.scale = {scale}");
                s.ui.scale = Some(scale);
                // Release the Mutex before apply_current_scale(),
                // which calls get_scale() and would deadlock on the same lock.
                drop(s);
                crate::hooks::ui_scale::apply_current_scale();
            }
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
            ui: GameUiSettings { scale: Some(150) },
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
    }
}
