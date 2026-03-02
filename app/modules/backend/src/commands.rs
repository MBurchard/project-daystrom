use serde::Serialize;
use ts_rs::TS;

use crate::game;
use crate::use_log;

use_log!("Commands");

/// STFC installation and entitlement status as returned to the frontend.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct GameStatus {
    /// Whether STFC was found on this machine.
    pub installed: bool,
    /// Root directory of the game installation, if found.
    pub install_dir: Option<String>,
    /// Full path to the game executable, if found.
    pub executable: Option<String>,
    /// Installed game version from the `.version` file, if available.
    pub game_version: Option<u32>,
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
    /// Whether the Scopely launcher is currently running.
    pub launcher_running: bool,
}

/// Detect the STFC installation and check its entitlements, mod availability and running state.
#[tauri::command]
pub fn get_game_status(app: tauri::AppHandle) -> GameStatus {
    let mod_available = game::find_mod_library(&app).is_some();

    let launcher_running = game::is_launcher_running();

    let result = match game::detect() {
        Some(info) => {
            let status = game::entitlements::check(&info.executable);
            let game_running = game::is_running(&info.executable);
            GameStatus {
                installed: true,
                install_dir: Some(info.install_dir.display().to_string()),
                executable: Some(info.executable.display().to_string()),
                game_version: info.installed_version,
                entitlements_ok: status.all_granted(),
                granted_entitlements: status.granted.iter().map(|s| s.to_string()).collect(),
                missing_entitlements: status.missing.iter().map(|s| s.to_string()).collect(),
                mod_available,
                game_running,
                launcher_running,
            }
        }
        None => GameStatus {
            installed: false,
            install_dir: None,
            executable: None,
            game_version: None,
            entitlements_ok: false,
            granted_entitlements: vec![],
            missing_entitlements: vec![],
            mod_available,
            game_running: false,
            launcher_running,
        },
    };

    if result.game_running || result.launcher_running {
        crate::monitor::start(app, result.game_running, result.launcher_running);
    }

    result
}

/// Re-sign the game executable with the required mod-injection entitlements.
///
/// Returns the refreshed game status so the frontend can update in one step.
#[tauri::command]
pub fn patch_entitlements(app: tauri::AppHandle) -> Result<GameStatus, String> {
    let info = game::detect().ok_or("STFC not found")?;

    if game::is_running(&info.executable) {
        return Err("Cannot patch entitlements while the game is running".to_string());
    }

    game::entitlements::patch(&info.executable)?;
    Ok(get_game_status(app))
}

/// Result of checking the Scopely update API for a game update.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct UpdateCheck {
    /// Currently installed game version from the `.version` file.
    pub installed_version: u32,
    /// Latest version reported by the Scopely update API, if reachable.
    pub remote_version: Option<u32>,
    /// Whether an update is available (remote > installed).
    pub update_available: bool,
}

/// Check whether a game update is available by comparing the local `.version` file
/// against the Scopely update API.
#[tauri::command]
pub fn check_for_update() -> Result<UpdateCheck, String> {
    let info = game::detect().ok_or("STFC not found")?;
    let installed = info.installed_version.ok_or("Could not read installed game version")?;

    match game::version::fetch_remote(installed) {
        Ok(Some(remote)) => Ok(UpdateCheck {
            installed_version: installed,
            remote_version: Some(remote),
            update_available: remote > installed,
        }),
        Ok(None) => Ok(UpdateCheck {
            installed_version: installed,
            remote_version: None,
            update_available: false,
        }),
        Err(e) => {
            log_warn!("Update check failed: {e}");
            Err(format!("Update check failed: {e}"))
        }
    }
}

/// Lightweight process check for polling. Only runs `pgrep`, no filesystem I/O.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct ProcessStatus {
    /// Whether the game process is currently running.
    pub game_running: bool,
    /// Whether the Scopely launcher is currently running.
    pub launcher_running: bool,
}

/// Open the Scopely launcher so the user can install an update.
///
/// Starts background process monitoring after a successful launch.
#[tauri::command]
pub fn launch_updater(app: tauri::AppHandle) -> Result<(), String> {
    game::launcher::open_updater()?;
    crate::monitor::start(app, false, true);
    Ok(())
}

/// Launch the game with the mod library injected.
///
/// Starts background process monitoring after a successful launch.
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

    game::launcher::launch(&info, &dylib)?;
    crate::monitor::start(app, true, false);
    Ok(())
}
