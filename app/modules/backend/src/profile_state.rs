//! Profile state store with automatic change-based event emission.
//!
//! Scans the Daystrom config directory for profile TOML files (created by the mod) and exposes the list of known
//! profiles to the frontend. The monitor triggers a
//! re-scan every 60 seconds.

use std::fs;
use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;
use ts_rs::TS;

use crate::use_log;

use_log!("Profiles");

/// Information about a single player profile, derived from the TOML filename.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ProfileInfo {
    /// Player name (from filename, e.g. "Nabor").
    pub name: String,
    /// Server/universe ID (from filename, e.g. 106).
    pub server: i32,
    /// TOML filename (e.g. "106_Nabor.toml").
    pub filename: String,
}

/// State of all known profiles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ProfileState {
    /// All detected profile files.
    pub profiles: Vec<ProfileInfo>,
}

/// Global profile state.
static STATE: Mutex<ProfileState> = Mutex::new(ProfileState {
    profiles: Vec::new(),
});

/// Return a snapshot of the current profile state.
pub fn get() -> ProfileState {
    STATE.lock().unwrap().clone()
}

/// Update the profile state and emit a `profile-status` event if anything changed.
pub fn update(app: &tauri::AppHandle, updater: impl FnOnce(&mut ProfileState)) {
    let changed = {
        let mut state = STATE.lock().unwrap();
        let old = state.clone();
        updater(&mut state);
        if *state != old {
            Some(state.clone())
        } else {
            None
        }
    };

    if let Some(payload) = changed {
        log_debug!("Profile state changed, emitting to frontend");
        let _ = app.emit("profile-status", payload);
    }
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
        if let Some(info) = parse_profile_filename(stem) {
            profiles.push(info);
        }
    }

    profiles.sort_by(|a, b| a.server.cmp(&b.server).then(a.name.cmp(&b.name)));
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
        filename: format!("{stem}.toml"),
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

// ---- Tauri Command --------------------------------------------------------

/// Return the current profile state for the frontend.
#[tauri::command]
pub fn get_cached_profile_state() -> ProfileState {
    get()
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_filename() {
        let info = parse_profile_filename("106_Nabor").unwrap();
        assert_eq!(info.name, "Nabor");
        assert_eq!(info.server, 106);
        assert_eq!(info.filename, "106_Nabor.toml");
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
}
