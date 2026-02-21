use serde::Serialize;
use ts_rs::TS;

use crate::game;
use crate::use_log;

use_log!("Commands");

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
    /// Whether the mod dylib was found in the app's resource directory.
    pub mod_available: bool,
    /// Whether the game process is currently running.
    pub game_running: bool,
}

/// Detect the STFC installation and check its entitlements, mod availability and running state.
#[tauri::command]
pub fn get_game_status(app: tauri::AppHandle) -> GameStatus {
    let mod_available = game::find_mod_library(&app).is_some();

    match game::detect() {
        Some(info) => {
            let status = game::entitlements::check(&info.executable);
            let game_running = game::is_running(&info.executable);
            GameStatus {
                installed: true,
                install_dir: Some(info.install_dir.display().to_string()),
                executable: Some(info.executable.display().to_string()),
                entitlements_ok: status.all_granted(),
                granted_entitlements: status.granted.iter().map(|s| s.to_string()).collect(),
                missing_entitlements: status.missing.iter().map(|s| s.to_string()).collect(),
                mod_available,
                game_running,
            }
        }
        None => GameStatus {
            installed: false,
            install_dir: None,
            executable: None,
            entitlements_ok: false,
            granted_entitlements: vec![],
            missing_entitlements: vec![],
            mod_available,
            game_running: false,
        },
    }
}

/// Re-sign the game executable with the required mod-injection entitlements.
#[tauri::command]
pub fn patch_entitlements() -> Result<(), String> {
    let info = game::detect().ok_or("STFC not found")?;

    if game::is_running(&info.executable) {
        return Err("Cannot patch entitlements while the game is running".to_string());
    }

    game::entitlements::patch(&info.executable)
}

/// Launch the game with the mod library injected.
#[tauri::command]
pub fn launch_game(app: tauri::AppHandle) -> Result<(), String> {
    let info = game::detect().ok_or("STFC not found")?;

    let dylib = game::find_mod_library(&app)
        .ok_or("Mod library not found — run build:mod first")?;

    let status = game::entitlements::check(&info.executable);
    if !status.all_granted() {
        let names: Vec<_> = status.missing.iter()
            .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
            .collect();
        return Err(format!("Missing entitlements: {} — patch them first", names.join(", ")));
    }

    game::launcher::launch(&info, &dylib)
}
