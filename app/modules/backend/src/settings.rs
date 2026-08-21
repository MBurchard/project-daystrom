//! Persistent application settings stored in `{data_dir}/{identifier}/settings.toml`.
//!
//! Settings survive across app restarts. The file is created lazily on first writing.
//! Missing or corrupted files fall back to sensible defaults without error.
//!
//! When a setting changes, a [`SettingsEvent`] is broadcast to all subscribers via a [`broadcast`] channel.
//! Other modules (e.g. WebSocket) can subscribe to react to changes without coupling.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::broadcast;
use ts_rs::TS;

#[cfg(target_os = "windows")]
use crate::game;
use crate::use_log;

use_log!("Settings");

// ---- Settings events ------------------------------------------------------------

/// Event emitted when a setting changes.
///
/// Subscribers (e.g. the WebSocket bridge) receive these via [`subscribe`] and can react without direct coupling to
/// the settings module.
/// Each variant carries the new value and can describe itself via [`key`](Self::key) and [`value`](Self::value) for
/// generic forwarding.
#[derive(Clone, Debug)]
pub enum SettingsEvent {
    /// The in-game UI scale percentage changed (None = reset to default).
    GameUiScale(Option<u32>),
    /// The "auto-open sidebar" toggle changed (None = reset to default).
    AutoOpenSidebar(Option<bool>),
    /// The "auto-expand job queue" toggle changed (None = reset to default).
    AutoExpandJobQueue(Option<bool>),
    /// The "disable all banners" toggle changed (None = reset to default).
    BannersDisableAll(Option<bool>),
    /// The list of individually disabled banner type names changed (None = reset to default).
    BannersDisabledTypes(Option<Vec<String>>),
    /// The system view zoom distance changed (None = reset to default).
    SystemZoom(Option<u32>),
    /// The ship names visibility distance changed (None = reset to default).
    ShipNamesVisible(Option<u32>),
    /// The "skip reveal sequence" toggle changed (None = reset to default).
    SkipRevealSequence(Option<bool>),
    /// The "skip first popup" toggle changed (None = reset to default).
    SkipFirstPopup(Option<bool>),
    /// Keyboard shortcut overrides changed.
    Shortcuts(BTreeMap<String, String>),
    /// Cargo auto-open settings changed.
    CargoView(GameCargoViewSettings),
    /// Configured upper bounds for selected in-game sliders changed.
    SliderLimits(GameSliderLimitSettings),
}

impl SettingsEvent {
    /// Dotted key path identifying the setting (e.g. `game.ui.scale`).
    pub fn key(&self) -> &'static str {
        match self {
            Self::GameUiScale(_) => "game.ui.scale",
            Self::AutoOpenSidebar(_) => "game.ui.auto_open_sidebar",
            Self::AutoExpandJobQueue(_) => "game.ui.auto_expand_job_queue",
            Self::BannersDisableAll(_) => "game.banners.disable_all",
            Self::BannersDisabledTypes(_) => "game.banners.disabled_types",
            Self::SystemZoom(_) => "game.ui.system_zoom",
            Self::ShipNamesVisible(_) => "game.ui.ship_names_visible",
            Self::SkipRevealSequence(_) => "game.ui.skip_reveal_sequence",
            Self::SkipFirstPopup(_) => "game.ui.skip_first_popup",
            Self::Shortcuts(_) => "game.shortcuts",
            Self::CargoView(_) => "game.cargo_view",
            Self::SliderLimits(_) => "game.slider_limits",
        }
    }

    /// The new value as a JSON value. Returns `null` for `None` (reset to default).
    pub fn value(&self) -> serde_json::Value {
        match self {
            Self::GameUiScale(v) => serde_json::json!(v),
            Self::AutoOpenSidebar(v) => serde_json::json!(v),
            Self::AutoExpandJobQueue(v) => serde_json::json!(v),
            Self::BannersDisableAll(v) => serde_json::json!(v),
            Self::BannersDisabledTypes(v) => serde_json::json!(v),
            Self::SystemZoom(v) => serde_json::json!(v),
            Self::ShipNamesVisible(v) => serde_json::json!(v),
            Self::SkipRevealSequence(v) => serde_json::json!(v),
            Self::SkipFirstPopup(v) => serde_json::json!(v),
            Self::Shortcuts(v) => serde_json::json!(v),
            Self::CargoView(v) => serde_json::json!(v),
            Self::SliderLimits(v) => serde_json::json!(v),
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

// ---- Lenient deserialization ----------------------------------------------------

/// Deserialize an `Option<T>` that returns `None` for type mismatches instead of failing.
///
/// Standard serde fails the entire document when a field has the wrong type (e.g. `scale = "hello"` when `u32`
/// is expected). This deserializer catches the error and returns `None`, so one corrupt field does not destroy the
/// rest of the settings.
fn lenient_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(deserializer: D) -> Result<Option<T>, D::Error> {
    Ok(T::deserialize(deserializer).ok())
}

const STANDARD_RECRUIT_MAX: u32 = 150;

/// Deserialize the Standard Recruit limit leniently and enforce the highest verified safe batch size.
fn standard_recruit_max<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    Ok(u32::deserialize(deserializer).ok().map(|value| value.min(STANDARD_RECRUIT_MAX)))
}

// ---- Data model -----------------------------------------------------------------

/// UI-related hint counters for progressive notifications.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HintSettings {
    /// Number of times the user has seen a minimize-to-tray hint (dialogue or notification).
    #[serde(default)]
    pub minimize_to_tray: u32,
}

/// Saved window position and size (logical pixels, display-independent).
///
/// Stored under `[ui.window]` in the settings file. The scale factor divides physical pixel values from Tauri events
/// before storage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowSettings {
    /// Horizontal position of the window's top-left corner.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub x: Option<i32>,
    /// Vertical position of the window's top-left corner.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub y: Option<i32>,
    /// Window width.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<u32>,
    /// Window height.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub height: Option<u32>,
}

/// UI-related settings for the Daystrom app itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UiSettings {
    /// Explicitly selected application language; absent until the user chooses one.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub language: Option<AppLanguage>,
    /// Explicitly selected application theme; absent until the user chooses one.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub theme: Option<AppTheme>,
    /// Persisted zoom factor for the Daystrom application interface.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub zoom: Option<f64>,
    /// Revision of the application safety notice last acknowledged by the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_notice_revision: Option<u32>,
    /// Progressive hint counters.
    #[serde(default)]
    pub hints: HintSettings,
    /// Saved window geometry (absent on first launch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSettings>,
}

/// Languages supported by the Daystrom application interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AppLanguage {
    /// English application interface.
    #[default]
    En,
    /// German application interface.
    De,
    /// Klingon application interface.
    Tlh,
}

/// Visual themes supported by the Daystrom application interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AppTheme {
    /// Existing system-aware Daystrom appearance.
    #[default]
    Classic,
    /// Dark gold Omega appearance.
    Omega,
}

/// UI settings that are sent to the game mod via WebSocket.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameUiSettings {
    /// UI scale percentage (50-200). Applied as a multiplier on the original scale factor.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub scale: Option<u32>,
    /// System view zoom distance (1000-3000). Controls the default camera distance when entering a system.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub system_zoom: Option<u32>,
    /// Ship names visibility distance (1000-3000). Controls how far ship names stay visible.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub ship_names_visible: Option<u32>,
    /// Whether to auto-open the chat sidebar when the game starts.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_open_sidebar: Option<bool>,
    /// Whether to auto-expand the job queue panel from compact to full view.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_expand_job_queue: Option<bool>,
    /// Whether to skip the shop reveal sequence animation when opening loot boxes.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_reveal_sequence: Option<bool>,
    /// Whether to skip the first interstitial and suppress automatic purchase-offer popups.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_first_popup: Option<bool>,
}

/// Toast banner suppression settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameBannerSettings {
    /// Whether to suppress all toast banner notifications.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub disable_all: Option<bool>,
    /// List of specific banner type names to suppress (e.g. `["Victory", "Defeat"]`).
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub disabled_types: Option<Vec<String>>,
}

/// Cargo auto-open settings for the target viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameCargoViewSettings {
    /// Whether to auto-open cargo after selecting a target.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled: Option<bool>,
    /// Whether hostile targets should auto-open cargo.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_for_hostiles: Option<bool>,
    /// Whether armada targets should auto-open cargo.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_for_armadas: Option<bool>,
    /// Whether player stations should auto-open cargo.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub show_for_stations: Option<bool>,
    /// Whether player ships should auto-open cargo.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
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

/// Optional upper bounds for selected in-game sliders.
///
/// Missing values preserve the maximum supplied by the game. Configured values only extend that maximum; they never
/// reduce it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameSliderLimitSettings {
    /// Maximum number of Standard Recruit bundles selectable at once.
    #[serde(
        default,
        deserialize_with = "standard_recruit_max",
        skip_serializing_if = "Option::is_none"
    )]
    pub standard_recruit_max: Option<u32>,
    /// Maximum number of alliance-donation units selectable at once.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub alliance_donation_max: Option<u32>,
    /// Maximum number of Transporter Pattern exchanges selectable at once.
    #[serde(
        default,
        deserialize_with = "lenient_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub transporter_pattern_max: Option<u32>,
}

/// Game-related settings that are sent to the mod.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
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
    /// Optional upper bounds for selected in-game sliders.
    #[serde(default)]
    pub slider_limits: GameSliderLimitSettings,
    /// Keyboard shortcut overrides. Key = action name, value = bound key (empty = disabled).
    /// Absent keys use their default binding.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<String, String>,
}

/// Per-target log level overrides, preserved across settings saves.
///
/// Configured in `[log_levels.game]` and `[log_levels.app]` sections of settings.toml.
/// The settings module does not interpret these values; it only ensures they survive
/// load/save round-trips. The actual parsing happens in each crate's logging module.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LogLevelScopes {
    /// Game mod log level overrides (read by the mod's logger).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub game: BTreeMap<String, String>,
    /// Daystrom app log level overrides (read by the backend's logger).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub app: BTreeMap<String, String>,
}

impl LogLevelScopes {
    /// Returns `true` when no overrides are configured in either scope.
    fn is_empty(&self) -> bool {
        self.game.is_empty() && self.app.is_empty()
    }
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
    /// Per-target log level overrides for game mod and app backend.
    #[serde(default, skip_serializing_if = "LogLevelScopes::is_empty")]
    pub log_levels: LogLevelScopes,
}

/// Global in-memory copy, loaded once at startup.
static SETTINGS: Mutex<AppSettings> = Mutex::new(AppSettings {
    ui: UiSettings {
        language: None,
        theme: None,
        zoom: None,
        safety_notice_revision: None,
        hints: HintSettings { minimize_to_tray: 0 },
        window: None,
    },
    game: GameSettings {
        ui: GameUiSettings {
            scale: None,
            system_zoom: None,
            ship_names_visible: None,
            auto_open_sidebar: None,
            auto_expand_job_queue: None,
            skip_reveal_sequence: None,
            skip_first_popup: None,
        },
        banners: GameBannerSettings { disable_all: None, disabled_types: None },
        cargo_view: GameCargoViewSettings::default_values(),
        slider_limits: GameSliderLimitSettings {
            standard_recruit_max: None,
            alliance_donation_max: None,
            transporter_pattern_max: None,
        },
        shortcuts: BTreeMap::new(),
    },
    log_levels: LogLevelScopes {
        game: BTreeMap::new(),
        app: BTreeMap::new(),
    },
});

/// Language currently used by native dialogues, notifications, and tray controls.
static ACTIVE_APP_LANGUAGE: Mutex<AppLanguage> = Mutex::new(AppLanguage::En);

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
/// Falls back to default when the file is missing, empty, or contains invalid TOML.
/// Safe to call multiple times, later calls overwrite the in-memory state.
pub fn load() {
    let settings = settings_path().map(|p| load_from(&p)).unwrap_or_default();
    *ACTIVE_APP_LANGUAGE.lock().unwrap() = settings.ui.language.unwrap_or_default();
    *SETTINGS.lock().unwrap() = settings;
}

/// Whether a debounced save is already scheduled.
static SAVE_PENDING: AtomicBool = AtomicBool::new(false);

/// Handle of the most recent save thread. Tests join this to await completion.
static SAVE_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

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
    // Capture the path now so tests can reset PATH_OVERRIDE without racing the thread.
    let Some(path) = settings_path() else {
        SAVE_PENDING.store(false, Ordering::SeqCst);
        log_warn!("Cannot determine settings path");
        return;
    };
    let handle = std::thread::spawn(move || {
        std::thread::sleep(SAVE_DEBOUNCE);
        SAVE_PENDING.store(false, Ordering::SeqCst);
        let settings = SETTINGS.lock().unwrap().clone();
        if let Some(dir) = path.parent()
            && let Err(e) = fs::create_dir_all(dir)
        {
            log_error!("Failed to create settings directory: {e}");
            return;
        }
        match toml::to_string_pretty(&settings) {
            Ok(content) => {
                if let Err(e) = fs::write(&path, content) {
                    log_error!("Failed to write {}: {e}", path.display());
                }
            }
            Err(e) => log_error!("Failed to serialize settings: {e}"),
        }
    });
    *SAVE_HANDLE.lock().unwrap() = Some(handle);
}

/// Block until the current save thread has finished writing to disk.
///
/// Takes the stored [`JoinHandle`](std::thread::JoinHandle) and joins it.
/// Returns immediately when no save is in flight.
/// Called during app shutdown to prevent dropping a pending debounced write.
pub fn flush_saves() {
    let handle = SAVE_HANDLE.lock().unwrap().take();
    if let Some(h) = handle {
        h.join().expect("save thread panicked");
    }
}

/// Read the persisted settings bytes after all pending writes have completed.
///
/// `None` records that no settings file existed and allows rollback to restore that exact state.
pub(crate) fn snapshot_for_rollback() -> Result<Option<Vec<u8>>, String> {
    flush_saves();
    let path = settings_path().ok_or_else(|| "cannot determine settings path".to_string())?;
    match fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

/// Restore an exact settings snapshot captured before an application update.
pub(crate) fn restore_rollback_snapshot(snapshot: Option<&[u8]>) -> Result<(), String> {
    let path = settings_path().ok_or_else(|| "cannot determine settings path".to_string())?;
    match snapshot {
        Some(bytes) => {
            let content =
                std::str::from_utf8(bytes).map_err(|error| format!("settings backup is not UTF-8: {error}"))?;
            toml::from_str::<AppSettings>(content)
                .map_err(|error| format!("settings backup is not valid Daystrom settings: {error}"))?;
            if let Some(directory) = path.parent() {
                fs::create_dir_all(directory)
                    .map_err(|error| format!("could not create {}: {error}", directory.display()))?;
            }
            fs::write(&path, bytes).map_err(|error| format!("could not restore {}: {error}", path.display()))?;
        }
        None => match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("could not remove {}: {error}", path.display())),
        },
    }
    load();
    Ok(())
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

// ---- Safety notice API ---------------------------------------------------------

/// Current revision of the mandatory application safety notice.
const SAFETY_NOTICE_REVISION: u32 = 1;

/// Platform-specific paths referenced by the mandatory safety notice.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SafetyNoticeContext {
    /// Operating-system variant whose removal instructions apply.
    platform: SafetyNoticePlatform,
    /// Absolute application directories that may be removed after uninstalling Daystrom.
    cleanup_paths: Vec<String>,
    /// Absolute path to the deployed Windows mod library, when the game installation is known.
    mod_library_path: Option<String>,
}

/// Operating-system variants supported by the safety-notice removal instructions.
#[derive(Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
#[allow(dead_code)] // The serialized contract includes platform-specific variants.
pub enum SafetyNoticePlatform {
    /// Windows removal instructions and paths.
    Windows,
    /// macOS removal instructions and paths.
    Macos,
    /// Unsupported platform without removal instructions.
    Other,
}

/// Convert an optional path into its user-facing absolute representation.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn display_path(path: Option<PathBuf>) -> Option<String> {
    path.map(|value| value.display().to_string())
}

/// Return the platform-specific paths needed by the safety notice.
#[tauri::command]
pub fn get_safety_notice_context() -> SafetyNoticeContext {
    #[cfg(target_os = "windows")]
    {
        let identifier = env!("TAURI_IDENTIFIER");
        let cleanup_paths = [
            display_path(dirs::data_dir().map(|path| path.join(identifier))),
            display_path(dirs::data_local_dir().map(|path| path.join(identifier))),
        ]
        .into_iter()
        .flatten()
        .collect();
        let mod_library_path = display_path(game::detect().map(|info| info.install_dir.join("version.dll")));
        SafetyNoticeContext {
            platform: SafetyNoticePlatform::Windows,
            cleanup_paths,
            mod_library_path,
        }
    }

    #[cfg(target_os = "macos")]
    {
        let identifier = env!("TAURI_IDENTIFIER");
        let cleanup_paths = [
            display_path(dirs::data_dir().map(|path| path.join(identifier))),
            display_path(dirs::home_dir().map(|path| path.join("Library/Logs").join(identifier))),
        ]
        .into_iter()
        .flatten()
        .collect();
        SafetyNoticeContext {
            platform: SafetyNoticePlatform::Macos,
            cleanup_paths,
            mod_library_path: None,
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    SafetyNoticeContext {
        platform: SafetyNoticePlatform::Other,
        cleanup_paths: Vec::new(),
        mod_library_path: None,
    }
}

/// Return whether a safety-notice revision still needs acknowledgement.
fn safety_notice_required_for(acknowledged_revision: Option<u32>) -> bool {
    acknowledged_revision.unwrap_or_default() < SAFETY_NOTICE_REVISION
}

/// Return whether the current safety notice must be shown before using Daystrom.
#[tauri::command]
pub fn is_safety_notice_required() -> bool {
    let acknowledged_revision = SETTINGS.lock().unwrap().ui.safety_notice_revision;
    safety_notice_required_for(acknowledged_revision)
}

/// Acknowledge the current safety notice.
#[tauri::command]
pub fn acknowledge_safety_notice() {
    update(|settings| {
        if settings.ui.safety_notice_revision == Some(SAFETY_NOTICE_REVISION) {
            return false;
        }
        settings.ui.safety_notice_revision = Some(SAFETY_NOTICE_REVISION);
        true
    });
}

// ---- Hint settings API ----------------------------------------------------------

/// Maximum value for the minimize hint counter.
///
/// Once this threshold is reached, the hint level is [`Silent`](crate::HintLevel::Silent) and further increments
/// would only waste disk writes.
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

// ---- Window settings API --------------------------------------------------------

/// Return the persisted application-interface zoom factor, if any.
pub fn get_ui_zoom() -> Option<f64> {
    SETTINGS.lock().unwrap().ui.zoom
}

/// Persist the application-interface zoom factor.
pub fn set_ui_zoom(zoom: f64) {
    update(|settings| {
        if settings.ui.zoom == Some(zoom) {
            return false;
        }
        settings.ui.zoom = Some(zoom);
        true
    });
}

/// Return the persisted application theme or the Classic default.
#[tauri::command]
pub fn get_app_theme() -> AppTheme {
    SETTINGS.lock().unwrap().ui.theme.unwrap_or_default()
}

/// Persist an explicit application theme selection.
#[tauri::command]
pub fn set_app_theme(theme: AppTheme) {
    update(|settings| {
        if settings.ui.theme == Some(theme) {
            return false;
        }
        settings.ui.theme = Some(theme);
        true
    });
}

/// Return the saved window geometry, if any.
///
/// Returns `None` on the first launch (no `[ui.window]` section in the settings file).
pub fn get_window_settings() -> Option<WindowSettings> {
    SETTINGS.lock().unwrap().ui.window.clone()
}

/// Save the current window geometry (debounced).
///
/// Calls with zero width or height are ignored.
pub fn save_window_state(x: i32, y: i32, width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    update(|s| {
        let new = WindowSettings {
            x: Some(x),
            y: Some(y),
            width: Some(width),
            height: Some(height),
        };
        let changed = s.ui.window.as_ref() != Some(&new);
        if changed {
            s.ui.window = Some(new);
        }
        changed
    });
}

// ---- Game settings API ----------------------------------------------------------

/// Resolve the application language from a persisted choice or a raw system locale.
fn resolve_app_language(language: Option<AppLanguage>, system_locale: Option<&str>) -> AppLanguage {
    language.unwrap_or_else(|| {
        let primary = system_locale
            .and_then(|locale| locale.split(['-', '_']).next())
            .unwrap_or_default();
        match primary.to_ascii_lowercase().as_str() {
            "de" => AppLanguage::De,
            "tlh" => AppLanguage::Tlh,
            _ => AppLanguage::En,
        }
    })
}

/// Return the persisted application language or derive it from the supplied system locale.
fn resolve_and_activate_app_language(system_locale: Option<&str>) -> AppLanguage {
    let language = SETTINGS.lock().unwrap().ui.language;
    let language = resolve_app_language(language, system_locale);
    *ACTIVE_APP_LANGUAGE.lock().unwrap() = language;
    language
}

/// Persist and activate an explicit application language selection.
fn set_app_language_value(language: AppLanguage) {
    *ACTIVE_APP_LANGUAGE.lock().unwrap() = language;
    update(|settings| {
        if settings.ui.language == Some(language) {
            return false;
        }
        settings.ui.language = Some(language);
        true
    });
}

/// Return the persisted application language or derive it from the supplied system locale.
#[tauri::command]
pub fn get_app_language(app: tauri::AppHandle, system_locale: Option<String>) -> AppLanguage {
    let language = resolve_and_activate_app_language(system_locale.as_deref());
    crate::refresh_native_menu_labels(&app);
    language
}

/// Persist an explicit application language selection.
#[tauri::command]
pub fn set_app_language(app: tauri::AppHandle, language: AppLanguage) {
    set_app_language_value(language);
    crate::refresh_native_menu_labels(&app);
}

/// Return the language currently used by native application surfaces.
pub(crate) fn active_app_language() -> AppLanguage {
    *ACTIVE_APP_LANGUAGE.lock().unwrap()
}

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
        let old = s.game.clone();
        s.game = settings.clone();

        if s.game == old {
            return;
        }

        let mut events = Vec::new();
        if s.game.ui.scale != old.ui.scale {
            events.push(SettingsEvent::GameUiScale(s.game.ui.scale));
        }
        if s.game.ui.auto_open_sidebar != old.ui.auto_open_sidebar {
            events.push(SettingsEvent::AutoOpenSidebar(s.game.ui.auto_open_sidebar));
        }
        if s.game.ui.auto_expand_job_queue != old.ui.auto_expand_job_queue {
            events.push(SettingsEvent::AutoExpandJobQueue(s.game.ui.auto_expand_job_queue));
        }
        if s.game.banners.disable_all != old.banners.disable_all {
            events.push(SettingsEvent::BannersDisableAll(s.game.banners.disable_all));
        }
        if s.game.banners.disabled_types != old.banners.disabled_types {
            events.push(SettingsEvent::BannersDisabledTypes(s.game.banners.disabled_types.clone()));
        }
        if s.game.ui.system_zoom != old.ui.system_zoom {
            events.push(SettingsEvent::SystemZoom(s.game.ui.system_zoom));
        }
        if s.game.ui.ship_names_visible != old.ui.ship_names_visible {
            events.push(SettingsEvent::ShipNamesVisible(s.game.ui.ship_names_visible));
        }
        if s.game.ui.skip_reveal_sequence != old.ui.skip_reveal_sequence {
            events.push(SettingsEvent::SkipRevealSequence(s.game.ui.skip_reveal_sequence));
        }
        if s.game.ui.skip_first_popup != old.ui.skip_first_popup {
            events.push(SettingsEvent::SkipFirstPopup(s.game.ui.skip_first_popup));
        }
        if s.game.shortcuts != old.shortcuts {
            events.push(SettingsEvent::Shortcuts(s.game.shortcuts.clone()));
        }
        if s.game.cargo_view != old.cargo_view {
            events.push(SettingsEvent::CargoView(s.game.cargo_view.clone()));
        }
        if s.game.slider_limits != old.slider_limits {
            events.push(SettingsEvent::SliderLimits(s.game.slider_limits.clone()));
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
                SettingsEvent::GameUiScale(v) => log_debug!("UI scale set to {v:?}"),
                SettingsEvent::AutoOpenSidebar(v) => log_debug!("Auto-open sidebar set to {v:?}"),
                SettingsEvent::AutoExpandJobQueue(v) => log_debug!("Auto-expand job queue set to {v:?}"),
                SettingsEvent::BannersDisableAll(v) => log_debug!("Disable all banners set to {v:?}"),
                SettingsEvent::BannersDisabledTypes(v) => log_debug!("Disabled banner types: {v:?}"),
                SettingsEvent::SystemZoom(v) => log_debug!("System zoom set to {v:?}"),
                SettingsEvent::ShipNamesVisible(v) => log_debug!("Ship names visible set to {v:?}"),
                SettingsEvent::SkipRevealSequence(v) => log_debug!("Skip reveal sequence set to {v:?}"),
                SettingsEvent::SkipFirstPopup(v) => log_debug!("Skip first popup set to {v:?}"),
                SettingsEvent::Shortcuts(v) => log_debug!("Shortcuts changed: {v:?}"),
                SettingsEvent::CargoView(v) => log_debug!("Cargo view settings changed: {v:?}"),
                SettingsEvent::SliderLimits(v) => log_debug!("Slider limits changed: {v:?}"),
            }
        }
    }
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// Serialize tests that touch the global [`SETTINGS`] state.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire [`TEST_LOCK`], recovering from poisoning left by a panicked test.
    ///
    /// The lock only serializes tests; it does not protect shared data. Ignoring the poison lets
    /// remaining tests run even when a previous test panicked. Any lingering save thread from a
    /// crashed test is drained before the new test proceeds.
    fn lock_tests() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        flush_saves();
        guard
    }

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
        assert_eq!(settings.ui.language, None);
        assert_eq!(settings.ui.theme, None);
        assert_eq!(settings.ui.zoom, None);
        assert_eq!(settings.ui.safety_notice_revision, None);
    }

    #[test]
    fn safety_notice_requires_the_current_revision() {
        assert!(safety_notice_required_for(None));
        assert!(safety_notice_required_for(Some(SAFETY_NOTICE_REVISION - 1)));
        assert!(!safety_notice_required_for(Some(SAFETY_NOTICE_REVISION)));
        assert!(!safety_notice_required_for(Some(SAFETY_NOTICE_REVISION + 1)));
    }

    #[test]
    fn application_language_uses_persisted_selection_or_supported_locale_family() {
        assert_eq!(resolve_app_language(Some(AppLanguage::En), Some("de-DE")), AppLanguage::En);
        assert_eq!(resolve_app_language(None, Some("de-DE")), AppLanguage::De);
        assert_eq!(resolve_app_language(None, Some("DE_at")), AppLanguage::De);
        assert_eq!(resolve_app_language(None, Some("de-CH")), AppLanguage::De);
        assert_eq!(resolve_app_language(None, Some("tlh-Latn")), AppLanguage::Tlh);
        assert_eq!(resolve_app_language(None, Some("en-DE")), AppLanguage::En);
        assert_eq!(resolve_app_language(None, None), AppLanguage::En);
    }

    #[test]
    fn explicit_application_language_is_persisted() {
        let _lock = lock_tests();
        let path = use_temp_path("application_language");
        *SETTINGS.lock().unwrap() = AppSettings::default();

        set_app_language_value(AppLanguage::Tlh);
        flush_saves();

        assert_eq!(resolve_and_activate_app_language(Some("en-US")), AppLanguage::Tlh);
        assert!(fs::read_to_string(path).unwrap().contains("language = \"tlh\""));

        set_app_language_value(AppLanguage::Tlh);
        *SETTINGS.lock().unwrap() = AppSettings::default();
        reset_path_override();
    }

    #[test]
    fn serde_round_trip() {
        let settings = AppSettings {
            ui: UiSettings {
                language: None,
                theme: None,
                zoom: None,
                safety_notice_revision: Some(SAFETY_NOTICE_REVISION),
                hints: HintSettings { minimize_to_tray: 42 },
                window: None,
            },
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let parsed: AppSettings = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.ui.hints.minimize_to_tray, 42);
        assert_eq!(parsed.ui.safety_notice_revision, Some(SAFETY_NOTICE_REVISION));
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
    fn invalid_application_language_falls_back_without_discarding_settings() {
        let parsed: AppSettings =
            toml::from_str("[ui]\nlanguage = \"elvish\"\n\n[ui.hints]\nminimize_to_tray = 3\n").unwrap();

        assert_eq!(parsed.ui.language, None);
        assert_eq!(parsed.ui.hints.minimize_to_tray, 3);
    }

    #[test]
    fn application_theme_defaults_and_persists_explicit_selection() {
        let _lock = lock_tests();
        let path = use_temp_path("application_theme");
        *SETTINGS.lock().unwrap() = AppSettings::default();

        assert_eq!(get_app_theme(), AppTheme::Classic);
        set_app_theme(AppTheme::Omega);
        flush_saves();

        assert_eq!(get_app_theme(), AppTheme::Omega);
        assert!(fs::read_to_string(path).unwrap().contains("theme = \"omega\""));

        set_app_theme(AppTheme::Omega);
        *SETTINGS.lock().unwrap() = AppSettings::default();
        reset_path_override();
    }

    #[test]
    fn invalid_application_theme_falls_back_without_discarding_settings() {
        let parsed: AppSettings =
            toml::from_str("[ui]\ntheme = \"borg\"\n\n[ui.hints]\nminimize_to_tray = 3\n").unwrap();

        assert_eq!(parsed.ui.theme, None);
        assert_eq!(parsed.ui.hints.minimize_to_tray, 3);
    }

    #[test]
    fn log_levels_survive_round_trip() {
        let toml_str = "\
            [game.ui]\nscale = 80\nauto_open_sidebar = true\n\n\
            [log_levels.game]\nChatFrame = \"Debug\"\n\n\
            [log_levels.app]\nWebSocket = \"Trace\"\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.log_levels.game.get("ChatFrame").unwrap(), "Debug");
        assert_eq!(parsed.log_levels.app.get("WebSocket").unwrap(), "Trace");

        // Re-serialize and parse again: log_levels must survive
        let serialized = toml::to_string_pretty(&parsed).unwrap();
        let reparsed: AppSettings = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed.log_levels.game.get("ChatFrame").unwrap(), "Debug");
        assert_eq!(reparsed.log_levels.app.get("WebSocket").unwrap(), "Trace");
        assert_eq!(reparsed.game.ui.scale, Some(80));
    }

    #[test]
    fn log_levels_omitted_when_empty() {
        let settings = AppSettings::default();
        let serialized = toml::to_string_pretty(&settings).unwrap();
        assert!(!serialized.contains("log_levels"));
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
        let _lock = lock_tests();
        let path = use_temp_path("save_create");

        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 5;
        save();
        flush_saves();

        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 5"));

        // Clean-up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn save_creates_parent_directories() {
        let _lock = lock_tests();
        let dir = test_dir("save_parents");
        let path = dir.join("nested").join("deep").join("settings.toml");
        *PATH_OVERRIDE.lock().unwrap() = Some(path.clone());

        save();
        flush_saves();

        assert!(path.exists());

        // Clean-up
        reset_path_override();
    }

    #[test]
    fn save_then_load_round_trip() {
        let _lock = lock_tests();
        let path = use_temp_path("save_load_rt");

        let original = AppSettings {
            ui: UiSettings {
                language: Some(AppLanguage::De),
                theme: Some(AppTheme::Omega),
                zoom: Some(1.2),
                safety_notice_revision: Some(SAFETY_NOTICE_REVISION),
                hints: HintSettings { minimize_to_tray: 99 },
                window: None,
            },
            ..Default::default()
        };
        *SETTINGS.lock().unwrap() = original.clone();
        save();
        flush_saves();

        let loaded = load_from(&path);
        assert_eq!(loaded, original);

        // Clean-up
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
        let _lock = lock_tests();
        reset_path_override();
        let path = settings_path().expect("settings_path should return Some");
        assert_eq!(path.file_name().unwrap(), "settings.toml");
        assert!(path.to_string_lossy().contains(env!("TAURI_IDENTIFIER")));
    }

    // -- Public API (global state + real path) --

    #[test]
    fn load_populates_global_state() {
        let _lock = lock_tests();
        let path = use_temp_path("load_global");
        fs::write(&path, "[ui.hints]\nminimize_to_tray = 7\n").unwrap();

        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 999;
        load();

        assert_eq!(minimize_hint_count(), 7, "load() should read from temp file");

        // Clean-up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn minimize_hint_count_reads_global() {
        let _lock = lock_tests();
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 42;

        assert_eq!(minimize_hint_count(), 42);

        // Clean-up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
    }

    #[test]
    fn save_and_increment_round_trip() {
        let _lock = lock_tests();
        let path = use_temp_path("save_increment_rt");

        // Set a known state, save (debounced), wait for flush
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 3;
        save();
        flush_saves();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 3"));

        // Increment (also debounced), verify in-memory immediately, on-disk after flush
        increment_minimize_hint();
        assert_eq!(minimize_hint_count(), 4);
        flush_saves();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("minimize_to_tray = 4"));

        // Clean-up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    #[test]
    fn increment_stops_at_max() {
        let _lock = lock_tests();
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

        // Drain the save thread spawned by the first increment before resetting the path override.
        flush_saves();

        // Clean-up
        SETTINGS.lock().unwrap().ui.hints.minimize_to_tray = 0;
        reset_path_override();
    }

    // -- Game UI settings --

    #[test]
    fn default_scale_is_none() {
        assert_eq!(GameUiSettings::default().scale, None);
    }

    #[test]
    fn game_settings_serde_round_trip() {
        let toml_str = "[game.ui]\nscale = 150\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.ui.scale, Some(150));

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("scale = 150"));
    }

    #[test]
    fn game_settings_missing_uses_defaults() {
        let parsed: AppSettings = toml::from_str("[ui.hints]\nminimize_to_tray = 2\n").unwrap();
        assert_eq!(parsed.game.ui.scale, None);
    }

    #[test]
    fn set_game_settings_updates_and_persists() {
        let _lock = lock_tests();
        let path = use_temp_path("game_set_persist");

        let mut new_settings = get_game_settings();
        new_settings.ui.scale = Some(75);
        set_game_settings(new_settings);

        // Not yet on disk (debounced)
        assert!(!path.exists(), "save should be debounced, not immediate");

        // Wait for debounce to flush
        flush_saves();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("scale = 75"));
        assert_eq!(get_game_settings().ui.scale, Some(75));

        // Clean-up
        SETTINGS.lock().unwrap().game = GameSettings::default();
        reset_path_override();
    }

    #[test]
    fn set_game_settings_unchanged_skips_save() {
        let _lock = lock_tests();
        let _path = use_temp_path("game_set_noop");

        let current = get_game_settings();
        set_game_settings(current);

        // No save should be scheduled for identical settings
        flush_saves();
        assert!(!_path.exists(), "no write expected when nothing changed");

        // Clean-up
        reset_path_override();
    }

    // -- Window settings --

    #[test]
    fn window_settings_none_by_default() {
        let settings = AppSettings::default();
        assert!(settings.ui.window.is_none());
    }

    #[test]
    fn window_settings_serde_round_trip_ignores_legacy_maximized_value() {
        let toml_str = "[ui.window]\nx = 100\ny = 200\nwidth = 1024\nheight = 768\nmaximized = false\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        let ws = parsed.ui.window.as_ref().unwrap();
        assert_eq!((ws.x, ws.y, ws.width, ws.height), (Some(100), Some(200), Some(1024), Some(768)),);

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("[ui.window]"));
        assert!(serialized.contains("x = 100"));
        assert!(!serialized.contains("maximized"));
    }

    #[test]
    fn window_settings_omitted_when_none() {
        let settings = AppSettings::default();
        let serialized = toml::to_string_pretty(&settings).unwrap();
        assert!(!serialized.contains("[ui.window]"));
    }

    #[test]
    fn save_window_state_stores_bounds() {
        let _lock = lock_tests();
        let _path = use_temp_path("window_bounds");

        save_window_state(100, 200, 800, 600);
        let ws = get_window_settings().expect("should have window settings");
        assert_eq!((ws.x, ws.y, ws.width, ws.height), (Some(100), Some(200), Some(800), Some(600)),);

        // Wait for debounced save thread to complete before clean-up.
        flush_saves();
        SETTINGS.lock().unwrap().ui.window = None;
        reset_path_override();
    }

    #[test]
    fn save_window_state_ignores_zero_size() {
        let _lock = lock_tests();
        let _path = use_temp_path("window_zero");

        save_window_state(100, 200, 800, 600);
        save_window_state(0, 0, 0, 0);

        let ws = get_window_settings().unwrap();
        assert_eq!((ws.width, ws.height), (Some(800), Some(600)));

        // Wait for debounced save thread to complete before clean-up.
        flush_saves();
        SETTINGS.lock().unwrap().ui.window = None;
        reset_path_override();
    }

    #[test]
    fn save_window_state_unchanged_skips_save() {
        let _lock = lock_tests();
        let _path = use_temp_path("window_noop");

        save_window_state(100, 200, 800, 600);
        // Same values again: should not trigger a disk write
        let changed = update(|s| {
            let new = WindowSettings {
                x: Some(100),
                y: Some(200),
                width: Some(800),
                height: Some(600),
            };
            let changed = s.ui.window.as_ref() != Some(&new);
            if changed {
                s.ui.window = Some(new);
            }
            changed
        });
        assert!(!changed);

        // Wait for debounced save thread to complete before clean-up.
        flush_saves();
        SETTINGS.lock().unwrap().ui.window = None;
        reset_path_override();
    }

    #[test]
    fn window_settings_disk_round_trip() {
        let _lock = lock_tests();
        let path = use_temp_path("window_disk_rt");

        save_window_state(476, 412, 1329, 915);
        flush_saves();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[ui.window]"));
        assert!(content.contains("x = 476"));
        assert!(content.contains("width = 1329"));

        let loaded = load_from(&path);
        let ws = loaded.ui.window.expect("should have window section");
        assert_eq!((ws.x, ws.y, ws.width, ws.height), (Some(476), Some(412), Some(1329), Some(915)),);

        // Clean-up
        SETTINGS.lock().unwrap().ui.window = None;
        reset_path_override();
    }

    #[test]
    fn set_game_settings_debounce_coalesces_rapid_changes() {
        let _lock = lock_tests();
        let path = use_temp_path("game_set_coalesce");

        // Rapid slider movement: many values in quick succession
        for scale in [60, 70, 80, 90] {
            set_game_settings(GameSettings {
                ui: GameUiSettings { scale: Some(scale), ..Default::default() },
                ..Default::default()
            });
        }

        // Wait for debounce to flush
        flush_saves();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("scale = 90"));

        // Clean-up
        SETTINGS.lock().unwrap().game = GameSettings::default();
        reset_path_override();
    }

    #[test]
    fn auto_open_sidebar_defaults_to_none() {
        assert_eq!(GameUiSettings::default().auto_open_sidebar, None);
    }

    #[test]
    fn auto_open_sidebar_serde_round_trip() {
        let toml_str = "[game.ui]\nscale = 100\nauto_open_sidebar = true\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.ui.auto_open_sidebar, Some(true));
    }

    #[test]
    fn system_zoom_defaults_to_none() {
        assert_eq!(GameUiSettings::default().system_zoom, None);
    }

    #[test]
    fn system_zoom_serde_round_trip() {
        let toml_str = "[game.ui]\nsystem_zoom = 1500\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.ui.system_zoom, Some(1500));

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("system_zoom = 1500"));
    }

    #[test]
    fn ship_names_visible_defaults_to_none() {
        assert_eq!(GameUiSettings::default().ship_names_visible, None);
    }

    #[test]
    fn ship_names_visible_serde_round_trip() {
        let toml_str = "[game.ui]\nship_names_visible = 2500\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.ui.ship_names_visible, Some(2500));

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("ship_names_visible = 2500"));
    }

    #[test]
    fn cargo_view_defaults() {
        let settings = GameCargoViewSettings::default();
        assert_eq!(settings.enabled, Some(false));
        assert_eq!(settings.show_for_hostiles, Some(true));
        assert_eq!(settings.show_for_armadas, Some(true));
        assert_eq!(settings.show_for_stations, Some(true));
        assert_eq!(settings.show_for_players, Some(false));
    }

    #[test]
    fn cargo_view_serde_round_trip() {
        let toml_str = "\
            [game.cargo_view]\n\
            enabled = true\n\
            show_for_hostiles = false\n\
            show_for_armadas = true\n\
            show_for_stations = true\n\
            show_for_players = false\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.cargo_view.enabled, Some(true));
        assert_eq!(parsed.game.cargo_view.show_for_hostiles, Some(false));

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("[game.cargo_view]"));
        assert!(serialized.contains("enabled = true"));
        assert!(serialized.contains("show_for_hostiles = false"));
    }

    #[test]
    fn slider_limits_default_to_game_values() {
        let settings = GameSliderLimitSettings::default();
        assert_eq!(settings.standard_recruit_max, None);
        assert_eq!(settings.alliance_donation_max, None);
        assert_eq!(settings.transporter_pattern_max, None);
    }

    #[test]
    fn slider_limits_serde_round_trip() {
        let toml_str = "\
            [game.slider_limits]\n\
            standard_recruit_max = 500\n\
            alliance_donation_max = 80\n\
            transporter_pattern_max = 120\n";
        let parsed: AppSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.game.slider_limits.standard_recruit_max, Some(150));
        assert_eq!(parsed.game.slider_limits.alliance_donation_max, Some(80));
        assert_eq!(parsed.game.slider_limits.transporter_pattern_max, Some(120));

        let serialized = toml::to_string_pretty(&parsed).unwrap();
        assert!(serialized.contains("[game.slider_limits]"));
        assert!(serialized.contains("standard_recruit_max = 150"));
        assert!(serialized.contains("alliance_donation_max = 80"));
        assert!(serialized.contains("transporter_pattern_max = 120"));
    }

    #[test]
    fn rollback_snapshot_restores_exact_persisted_settings() {
        let _lock = lock_tests();
        let path = use_temp_path("rollback_snapshot_restores_exact_persisted_settings");
        let original = toml::to_string_pretty(&AppSettings::default()).unwrap();
        fs::write(&path, &original).unwrap();
        let snapshot = snapshot_for_rollback().unwrap();
        fs::write(&path, "[ui.hints]\nminimize_to_tray = 4\n").unwrap();

        restore_rollback_snapshot(snapshot.as_deref()).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        reset_path_override();
    }

    #[test]
    fn absent_rollback_snapshot_removes_successor_settings() {
        let _lock = lock_tests();
        let path = use_temp_path("absent_rollback_snapshot_removes_successor_settings");
        fs::write(&path, toml::to_string_pretty(&AppSettings::default()).unwrap()).unwrap();

        restore_rollback_snapshot(None).unwrap();

        assert!(!path.exists());
        reset_path_override();
    }
}
