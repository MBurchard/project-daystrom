use serde::Serialize;
use ts_rs::TS;

use crate::game;

/// STFC installation and entitlement status as returned to the frontend.
#[derive(Serialize, TS)]
#[ts(export)]
pub struct GameStatus {
    /// Whether STFC was found on this machine.
    pub installed: bool,
    /// Root directory of the game installation, if found.
    pub install_dir: Option<String>,
    /// Full path to the game executable, if found.
    pub executable: Option<String>,
    /// Whether all four required entitlements are set on the game executable.
    pub entitlements_ok: bool,
    /// Entitlement keys that are present and set to `true`.
    pub granted_entitlements: Vec<String>,
    /// Entitlement keys that are missing (empty when `entitlements_ok` is true).
    pub missing_entitlements: Vec<String>,
}

/// Detect the STFC installation and check its entitlements.
#[tauri::command]
pub fn get_game_status() -> GameStatus {
    match game::detect() {
        Some(info) => {
            let status = game::entitlements::check(&info.executable);
            GameStatus {
                installed: true,
                install_dir: Some(info.install_dir.display().to_string()),
                executable: Some(info.executable.display().to_string()),
                entitlements_ok: status.all_granted(),
                granted_entitlements: status.granted.iter().map(|s| s.to_string()).collect(),
                missing_entitlements: status.missing.iter().map(|s| s.to_string()).collect(),
            }
        }
        None => GameStatus {
            installed: false,
            install_dir: None,
            executable: None,
            entitlements_ok: false,
            granted_entitlements: vec![],
            missing_entitlements: vec![],
        },
    }
}
