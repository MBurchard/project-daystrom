//! Profile state store with automatic change-based event emission.
//!
//! Scans the Daystrom config directory for profile TOML files (created by the mod) and exposes the list of known
//! profiles to the frontend. The monitor triggers a
//! re-scan every 60 seconds.

use std::fs;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use ts_rs::TS;

use crate::ui_error::UiErrorCode;
use crate::use_log;

use_log!("Profiles");

/// Information about a single player profile, derived from the TOML filename and contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ProfileInfo {
    /// Player name (from filename, e.g. "Nabor").
    pub name: String,
    /// Server/universe ID (from filename, e.g. 106).
    pub server: i32,
    /// Profile stem used for launching (e.g. "106_Nabor").
    pub stem: String,
    /// Whether this is the primary profile (imported from Unity).
    pub primary: bool,
}

/// Minimal TOML structure for reading the profile type.
#[derive(Deserialize)]
struct TomlProfil {
    #[serde(default)]
    profil: TomlProfilSection,
}

/// The `[profil]` section, only the fields we need for scanning.
#[derive(Default, Deserialize)]
struct TomlProfilSection {
    #[serde(default, rename = "type")]
    profile_type: Option<String>,
}

/// Internal ordering metadata excluded from frontend-visible state equality.
#[derive(Clone, Copy, Debug, Default, Eq)]
struct ProcessRevision(u64);

impl PartialEq for ProcessRevision {
    /// Revisions order competing updates but never represent a visible state change.
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// State of all known profiles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ProfileState {
    /// All detected profile files.
    pub profiles: Vec<ProfileInfo>,
    /// Profile stems of game instances currently running (launched by Daystrom).
    pub running_profiles: Vec<String>,
    /// Profile stems still waiting within their initial mod-handshake grace period.
    pub starting_profiles: Vec<String>,
    /// Whether a game process is running that was NOT launched by Daystrom.
    pub external_game_running: bool,
    /// Whether Daystrom is waiting for a running game to restore its launch identity.
    pub game_origin_pending: bool,
    /// Whether a running game has not established a validated Daystrom mod connection.
    pub mod_connection_missing: bool,
    /// Latest tracked-game revision applied to the backend-owned state.
    #[serde(skip)]
    #[ts(skip)]
    process_revision: ProcessRevision,
}

/// Global profile state.
static STATE: Mutex<ProfileState> = Mutex::new(ProfileState {
    profiles: Vec::new(),
    running_profiles: Vec::new(),
    starting_profiles: Vec::new(),
    external_game_running: false,
    game_origin_pending: false,
    mod_connection_missing: false,
    process_revision: ProcessRevision(0),
});

/// Return a snapshot of the current profile state.
pub fn get() -> ProfileState {
    STATE.lock().unwrap().clone()
}

/// Update the profile state and emit a `profile-status` event if anything changed.
pub fn update(app: &tauri::AppHandle, updater: impl FnOnce(&mut ProfileState)) {
    if let Some(payload) = crate::state_update::update_if_changed(&STATE, updater) {
        log_debug!("Profile state changed, emitting to frontend");
        let _ = app.emit("profile-status", payload);
    }
}

/// Apply process-derived state unless a newer tracked-game snapshot was already stored.
pub fn update_from_process(app: &tauri::AppHandle, revision: u64, updater: impl FnOnce(&mut ProfileState)) -> bool {
    let mut applied = false;
    update(app, |state| {
        applied = apply_process_update(state, revision, updater);
    });
    applied
}

/// Apply one process-derived update only when its source snapshot is not stale.
fn apply_process_update(state: &mut ProfileState, revision: u64, updater: impl FnOnce(&mut ProfileState)) -> bool {
    if revision < state.process_revision.0 {
        return false;
    }
    updater(state);
    state.process_revision = ProcessRevision(revision);
    true
}

/// Scan the config directory for profile TOML files.
///
/// Parses the filename (e.g. `106_Nabor.toml`) to extract server ID and player name.
/// Does not open or parse the file contents.
pub fn scan_profiles() -> Vec<ProfileInfo> {
    let Some(dir) = profile_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut profiles = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(mut info) = parse_profile_filename(stem) {
            // Read the profile type from the TOML contents
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(parsed) = toml::from_str::<TomlProfil>(&content)
            {
                info.primary = parsed.profil.profile_type.as_deref() == Some("primary");
            }
            profiles.push(info);
        }
    }

    // Primary first, then by server and name
    profiles.sort_by(|a, b| {
        b.primary
            .cmp(&a.primary)
            .then(a.server.cmp(&b.server))
            .then(a.name.cmp(&b.name))
    });
    profiles
}

/// Parse a profile filename stem (without `.toml`) into a `ProfileInfo`.
///
/// Expected format: `{server}_{name}`, e.g. `106_Nabor`.
/// Returns `None` if the format doesn't match.
fn parse_profile_filename(stem: &str) -> Option<ProfileInfo> {
    let underscore = stem.find('_')?;
    let server_str = &stem[..underscore];
    let name = &stem[underscore + 1..];

    if name.is_empty() {
        return None;
    }

    let server: i32 = server_str.parse().ok()?;
    Some(ProfileInfo {
        name: name.to_string(),
        server,
        stem: stem.to_string(),
        primary: false,
    })
}

/// Determine the profile directory (same as the Daystrom config directory).
///
/// - macOS: `~/Library/Application Support/mbur.project-daystrom/`
/// - Windows: `{APPDATA}/mbur.project-daystrom/`
fn profile_dir() -> Option<std::path::PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join(env!("TAURI_IDENTIFIER")))
}

/// Resolve a profile file only when its stem belongs to the current backend-owned profile state.
fn known_profile_path(
    dir: &std::path::Path,
    profiles: &[ProfileInfo],
    stem: &str,
) -> Result<std::path::PathBuf, UiErrorCode> {
    profiles
        .iter()
        .find(|profile| profile.stem == stem)
        .map(|profile| dir.join(format!("{}.toml", profile.stem)))
        .ok_or(UiErrorCode::ProfileNotFound)
}

/// Delete one known local profile file, treating an already absent file as successfully deleted.
fn remove_profile_file(path: &std::path::Path) -> Result<(), UiErrorCode> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            log_warn!("Could not delete local profile {}: {error}", path.display());
            Err(UiErrorCode::ProfileDeletionFailed)
        }
    }
}

// ---- Tauri Command --------------------------------------------------------

/// Return the current profile state for the frontend.
#[tauri::command]
pub fn get_cached_profile_state() -> ProfileState {
    get()
}

/// Delete the selected profile and its local login data without contacting Scopely.
#[tauri::command]
pub fn delete_local_profile(app: tauri::AppHandle, stem: String) -> Result<(), UiErrorCode> {
    let state = get();
    if crate::game::is_game_running() || !state.running_profiles.is_empty() {
        return Err(UiErrorCode::GameRunning);
    }

    let dir = profile_dir().ok_or(UiErrorCode::ProfileDeletionFailed)?;
    let path = known_profile_path(&dir, &state.profiles, &stem)?;
    remove_profile_file(&path)?;

    update(&app, |state| {
        state.profiles.retain(|profile| profile.stem != stem);
        state.running_profiles.retain(|running| running != &stem);
    });
    log_info!("Deleted local profile {stem}");
    Ok(())
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_updates_reject_stale_snapshots() {
        let mut state = ProfileState::default();
        let mut stale_update_ran = false;

        assert!(apply_process_update(&mut state, 2, |state| {
            state.mod_connection_missing = false;
            state.running_profiles = vec!["106_Nabor".to_string()];
        }));
        assert!(!apply_process_update(&mut state, 1, |state| {
            stale_update_ran = true;
            state.mod_connection_missing = true;
            state.running_profiles = vec!["initial".to_string()];
        }));

        assert!(!stale_update_ran);
        assert_eq!(state.running_profiles, vec!["106_Nabor"]);
        assert!(!state.mod_connection_missing);
        assert_eq!(state.process_revision.0, 2);
    }

    #[test]
    fn process_revision_is_not_serialized() {
        let state = ProfileState {
            process_revision: ProcessRevision(7),
            ..ProfileState::default()
        };

        let serialized = serde_json::to_value(state).unwrap();

        assert!(serialized.get("process_revision").is_none());
    }

    #[test]
    fn process_revision_does_not_change_profile_state_equality() {
        let current = ProfileState::default();
        let newer = ProfileState {
            process_revision: ProcessRevision(1),
            ..ProfileState::default()
        };

        assert_eq!(current, newer);
    }

    #[test]
    fn process_revision_does_not_emit_a_changed_snapshot() {
        let state = Mutex::new(ProfileState::default());

        let changed = crate::state_update::update_if_changed(&state, |state| {
            state.process_revision = ProcessRevision(1);
        });

        assert!(changed.is_none());
        assert_eq!(state.lock().unwrap().process_revision.0, 1);
    }

    /// Build a profile suitable for path-resolution tests.
    fn profile(stem: &str) -> ProfileInfo {
        ProfileInfo {
            name: "Test Account".to_string(),
            server: 1,
            stem: stem.to_string(),
            primary: false,
        }
    }

    #[test]
    fn parse_valid_filename() {
        let info = parse_profile_filename("106_Nabor").unwrap();
        assert_eq!(info.name, "Nabor");
        assert_eq!(info.server, 106);
        assert_eq!(info.stem, "106_Nabor");
    }

    #[test]
    fn parse_name_with_underscores() {
        let info = parse_profile_filename("42_My_Alt_Account").unwrap();
        assert_eq!(info.name, "My_Alt_Account");
        assert_eq!(info.server, 42);
    }

    #[test]
    fn parse_invalid_no_underscore() {
        assert!(parse_profile_filename("settings").is_none());
    }

    #[test]
    fn parse_invalid_no_number() {
        assert!(parse_profile_filename("abc_Nabor").is_none());
    }

    #[test]
    fn parse_invalid_empty_name() {
        assert!(parse_profile_filename("106_").is_none());
    }

    #[test]
    fn resolves_only_stems_owned_by_profile_state() {
        let dir = std::path::Path::new("profiles");
        let profiles = vec![profile("1_TestAccount")];

        assert_eq!(
            known_profile_path(dir, &profiles, "1_TestAccount").unwrap(),
            dir.join("1_TestAccount.toml")
        );
        assert_eq!(
            known_profile_path(dir, &profiles, "../foreign"),
            Err(UiErrorCode::ProfileNotFound)
        );
    }

    #[test]
    fn removes_known_profile_file_and_accepts_an_absent_file() {
        let dir = std::env::temp_dir().join(format!("daystrom-profile-deletion-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("1_TestAccount.toml");
        fs::write(&path, "[profil]").unwrap();

        assert_eq!(remove_profile_file(&path), Ok(()));
        assert!(!path.exists());
        assert_eq!(remove_profile_file(&path), Ok(()));
        fs::remove_dir_all(dir).unwrap();
    }
}
