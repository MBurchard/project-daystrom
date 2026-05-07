//! Game settings received from Daystrom.
//!
//! The mod keeps a local copy of the settings with the same defaults as Daystrom.
//! On WebSocket connection, the mod requests a full sync (`settings.sync`).
//! Incremental updates (`settings.update`) patch individual fields afterwards.

use std::sync::{Mutex, OnceLock};

use log::debug;
use serde::{Deserialize, Deserializer};

// ---- Lenient deserialization -----------------------------------------------

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
    /// System view zoom distance (1000-3000). Controls the default camera distance when entering a system.
    #[serde(default, deserialize_with = "lenient_option")]
    pub system_zoom: Option<u32>,
    /// Ship names visibility distance (1000-3000). Controls how far ship names stay visible.
    #[serde(default, deserialize_with = "lenient_option")]
    pub ship_names_visible: Option<u32>,
    /// Whether to auto-open the chat sidebar on game start.
    #[serde(default, deserialize_with = "lenient_option")]
    pub auto_open_sidebar: Option<bool>,
    /// Whether to auto-expand the job queue panel from compact to full view.
    #[serde(default, deserialize_with = "lenient_option")]
    pub auto_expand_job_queue: Option<bool>,
    /// Whether to skip the shop reveal sequence animation when opening loot boxes.
    #[serde(default, deserialize_with = "lenient_option")]
    pub skip_reveal_sequence: Option<bool>,
    /// Whether to skip the first interstitial popup (ad) after game start.
    #[serde(default, deserialize_with = "lenient_option")]
    pub skip_first_popup: Option<bool>,
}

/// Toast banner suppression settings.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GameBannerSettings {
    /// Whether to suppress all toast banner notifications.
    #[serde(default, deserialize_with = "lenient_option")]
    pub disable_all: Option<bool>,
    /// List of specific banner type names to suppress (e.g. `["Victory", "Defeat"]`).
    #[serde(default, deserialize_with = "lenient_option")]
    pub disabled_types: Option<Vec<String>>,
}

/// Cargo auto-open settings for the target viewer.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GameCargoViewSettings {
    /// Whether to auto-open cargo after selecting a target.
    #[serde(default, deserialize_with = "lenient_option")]
    pub enabled: Option<bool>,
    /// Whether hostile targets should auto-open cargo.
    #[serde(default, deserialize_with = "lenient_option")]
    pub show_for_hostiles: Option<bool>,
    /// Whether armada targets should auto-open cargo.
    #[serde(default, deserialize_with = "lenient_option")]
    pub show_for_armadas: Option<bool>,
    /// Whether player stations should auto-open cargo.
    #[serde(default, deserialize_with = "lenient_option")]
    pub show_for_stations: Option<bool>,
    /// Whether player ships should auto-open cargo.
    #[serde(default, deserialize_with = "lenient_option")]
    pub show_for_players: Option<bool>,
}

impl Default for GameCargoViewSettings {
    fn default() -> Self {
        Self::default_values()
    }
}

impl GameCargoViewSettings {
    const fn default_values() -> Self {
        Self {
            enabled: Some(false),
            show_for_hostiles: Some(true),
            show_for_armadas: Some(true),
            show_for_stations: Some(true),
            show_for_players: Some(false),
        }
    }
}

/// Game settings received from Daystrom.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct GameSettings {
    /// In-game UI appearance.
    #[serde(default)]
    pub ui: GameUiSettings,
    /// Toast banner suppression.
    #[serde(default)]
    pub banners: GameBannerSettings,
    /// Cargo auto-open behavior.
    #[serde(default)]
    pub cargo_view: GameCargoViewSettings,
    /// Keyboard shortcut overrides. Key = action name, value = bound key (empty = disabled).
    #[serde(default)]
    pub shortcuts: std::collections::BTreeMap<String, String>,
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

/// Whether to skip the shop reveal sequence animation.
pub fn skip_reveal_sequence() -> bool {
    state().lock().unwrap().ui.skip_reveal_sequence.unwrap_or(true)
}

/// Whether to skip the first interstitial popup after game start.
pub fn skip_first_popup() -> bool {
    state().lock().unwrap().ui.skip_first_popup.unwrap_or(true)
}

/// Whether all toast banner notifications should be suppressed.
pub fn disable_all_banners() -> bool {
    state().lock().unwrap().banners.disable_all.unwrap_or(false)
}

/// List of specific banner type names to suppress.
pub fn disabled_banner_types() -> Vec<String> {
    state().lock().unwrap().banners.disabled_types.clone().unwrap_or_default()
}

/// Whether cargo should auto-open after selecting a target.
pub fn cargo_view_enabled() -> bool {
    state().lock().unwrap().cargo_view.enabled.unwrap_or(false)
}

/// Whether hostile targets should auto-open cargo.
pub fn show_cargo_for_hostiles() -> bool {
    state().lock().unwrap().cargo_view.show_for_hostiles.unwrap_or(true)
}

/// Whether armada targets should auto-open cargo.
pub fn show_cargo_for_armadas() -> bool {
    state().lock().unwrap().cargo_view.show_for_armadas.unwrap_or(true)
}

/// Whether player stations should auto-open cargo.
pub fn show_cargo_for_stations() -> bool {
    state().lock().unwrap().cargo_view.show_for_stations.unwrap_or(true)
}

/// Whether player ships should auto-open cargo.
pub fn show_cargo_for_players() -> bool {
    state().lock().unwrap().cargo_view.show_for_players.unwrap_or(false)
}

/// System view zoom distance (1000-3000, default 1000).
pub fn get_system_zoom() -> u32 {
    state().lock().unwrap().ui.system_zoom.unwrap_or(1000)
}

/// Ship names visibility distance (1000-3000, default 1800).
pub fn get_ship_names_visible() -> u32 {
    state().lock().unwrap().ui.ship_names_visible.unwrap_or(1800)
}

/// Key binding for the main action (engage, mine, warp).
///
/// Returns `Some("Space")` by default. Returns `None` when explicitly disabled (empty string).
pub fn trigger_main_action() -> Option<String> {
    match state().lock().unwrap().shortcuts.get("trigger_main_action").map(|s| s.as_str()) {
        Some("") => None,
        Some(key) => Some(key.to_string()),
        None => Some("Space".to_string()),
    }
}

/// Replace all settings with a full snapshot from Daystrom (`settings.sync`).
pub fn apply_sync(settings: GameSettings) {
    debug!(target: "Settings", "Sync: {settings:?}");
    // Scoped block: release the Mutex before side effect hooks, which call getters and would deadlock on the same lock.
    { *state().lock().unwrap() = settings; }
    crate::hooks::ui_scale::apply_current_scale();
    crate::hooks::chat_frame::on_settings_synced();
    crate::hooks::job_queue::on_settings_synced();
    crate::hooks::toast_banner::on_settings_changed();
    crate::hooks::system_zoom::on_settings_changed();
    crate::hooks::hotkeys::on_shortcuts_changed();
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
            // Release the Mutex before apply_current_scale(), which calls get_scale() and would deadlock.
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
        "game.ui.skip_reveal_sequence" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.ui.skip_reveal_sequence = {new_val:?}");
            s.ui.skip_reveal_sequence = new_val;
        }
        "game.ui.skip_first_popup" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.ui.skip_first_popup = {new_val:?}");
            s.ui.skip_first_popup = new_val;
        }
        "game.banners.disable_all" => {
            let new_val = value.as_bool();
            debug!(target: "Settings", "Update: game.banners.disable_all = {new_val:?}");
            s.banners.disable_all = new_val;
            drop(s);
            crate::hooks::toast_banner::on_settings_changed();
        }
        "game.banners.disabled_types" => {
            let new_val = value.as_array().map(|arr| {
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
            });
            debug!(target: "Settings", "Update: game.banners.disabled_types = {new_val:?}");
            s.banners.disabled_types = new_val;
            drop(s);
            crate::hooks::toast_banner::on_settings_changed();
        }
        "game.ui.system_zoom" => {
            let new_val = value.as_u64().map(|v| v as u32);
            debug!(target: "Settings", "Update: game.ui.system_zoom = {new_val:?}");
            s.ui.system_zoom = new_val;
            drop(s);
            crate::hooks::system_zoom::on_settings_changed();
        }
        "game.ui.ship_names_visible" => {
            let new_val = value.as_u64().map(|v| v as u32);
            debug!(target: "Settings", "Update: game.ui.ship_names_visible = {new_val:?}");
            s.ui.ship_names_visible = new_val;
            drop(s);
            crate::hooks::system_zoom::on_settings_changed();
        }
        "game.shortcuts" => {
            let new_val: std::collections::BTreeMap<String, String> =
                serde_json::from_value(value.clone()).unwrap_or_default();
            debug!(target: "Settings", "Update: game.shortcuts = {new_val:?}");
            s.shortcuts = new_val;
            drop(s);
            crate::hooks::hotkeys::on_shortcuts_changed();
        }
        "game.cargo_view" => {
            let new_val = serde_json::from_value::<GameCargoViewSettings>(value.clone()).ok();
            debug!(target: "Settings", "Update: game.cargo_view = {new_val:?}");
            if let Some(new_val) = new_val {
                s.cargo_view = new_val;
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
            ui: GameUiSettings { scale: Some(150), ..Default::default() },
            ..Default::default()
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

    #[test]
    fn system_zoom_getter_default() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert_eq!(get_system_zoom(), 1000);

        reset();
    }

    #[test]
    fn apply_update_patches_system_zoom() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.system_zoom", &serde_json::json!(1500));
        assert_eq!(state().lock().unwrap().ui.system_zoom, Some(1500));

        reset();
    }

    #[test]
    fn ship_names_visible_getter_default() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert_eq!(get_ship_names_visible(), 1800);

        reset();
    }

    #[test]
    fn apply_update_patches_ship_names_visible() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.ship_names_visible", &serde_json::json!(2500));
        assert_eq!(state().lock().unwrap().ui.ship_names_visible, Some(2500));

        reset();
    }

    #[test]
    fn skip_reveal_sequence_defaults_to_true() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert!(skip_reveal_sequence());

        reset();
    }

    #[test]
    fn apply_update_patches_skip_reveal_sequence() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.skip_reveal_sequence", &serde_json::json!(true));
        assert_eq!(state().lock().unwrap().ui.skip_reveal_sequence, Some(true));

        reset();
    }

    #[test]
    fn skip_first_popup_defaults_to_true() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert!(skip_first_popup());

        reset();
    }

    #[test]
    fn apply_update_patches_skip_first_popup() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update("game.ui.skip_first_popup", &serde_json::json!(false));
        assert_eq!(state().lock().unwrap().ui.skip_first_popup, Some(false));

        reset();
    }

    #[test]
    fn cargo_view_defaults() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert!(!cargo_view_enabled());
        assert!(show_cargo_for_hostiles());
        assert!(show_cargo_for_armadas());
        assert!(show_cargo_for_stations());
        assert!(!show_cargo_for_players());

        reset();
    }

    #[test]
    fn apply_update_patches_cargo_view() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        apply_update(
            "game.cargo_view",
            &serde_json::json!({
                "enabled": true,
                "show_for_hostiles": false,
                "show_for_armadas": true,
                "show_for_stations": false,
                "show_for_players": true
            }),
        );

        assert!(cargo_view_enabled());
        assert!(!show_cargo_for_hostiles());
        assert!(show_cargo_for_armadas());
        assert!(!show_cargo_for_stations());
        assert!(show_cargo_for_players());

        reset();
    }
}
