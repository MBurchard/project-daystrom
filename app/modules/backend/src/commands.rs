use std::thread;

use serde::Serialize;
use tauri::{Emitter, Manager};
#[cfg(target_os = "windows")]
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
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
    /// Installed game version from the `.version` file, if available.
    pub game_version: Option<u32>,
    /// Whether the mod library was found in the app's resource directory.
    pub mod_available: bool,
    /// Whether the mod can be installed or updated (game found and mod library bundled).
    pub mod_installable: bool,
    /// Whether the mod is deployed and ready (macOS: entitlements OK, Windows: DLL up to date).
    pub mod_deployed: bool,
    /// Whether the mod DLL exists but is outdated (hash mismatch). Always `false` on macOS.
    pub mod_outdated: bool,
    /// Whether the mod can be removed from disk (Windows: DLL deployed or outdated, macOS: always false).
    pub mod_removable: bool,
    /// Whether the game process is currently running.
    pub game_running: bool,
    /// Whether the Scopely launcher is currently running.
    pub launcher_running: bool,
}

/// Detect the STFC installation and check its entitlements, mod availability, and running state.
#[tauri::command]
pub fn get_game_status(app: tauri::AppHandle) -> GameStatus {
    let mod_library = game::find_mod_library(&app);
    let mod_available = mod_library.is_some();

    match &mod_library {
        Some(path) => log_info!("Mod library found: {}", path.display()),
        None => log_warn!("Mod library not bundled, run pnpm build:mod"),
    }

    let launcher_running = game::is_launcher_running();

    let result = match game::detect() {
        Some(info) => {
            match info.installed_version {
                Some(v) => log_info!("STFC found (v{v}): {}", info.executable.display()),
                None => log_info!("STFC found: {}", info.executable.display()),
            }

            let status = game::entitlements::check(&info.executable);
            if status.all_granted() {
                log_info!("Entitlements OK, mod injection ready");
            } else {
                let names: Vec<_> = status.missing.iter()
                    .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
                    .collect();
                log_warn!("Missing entitlements: {}", names.join(", "));
            }

            let game_running = game::is_running(&info.executable);

            // macOS: mod is "deployed" when entitlements are OK (injection via DYLD)
            // Windows: mod is deployed when the DLL is copied and up to date
            #[cfg(target_os = "macos")]
            let (mod_deployed, mod_outdated) = (status.all_granted(), false);
            #[cfg(target_os = "windows")]
            let (mod_deployed, mod_outdated) = mod_library.as_ref().map(|lib| {
                match game::check_mod_deployment(&info.install_dir, lib) {
                    game::ModDeploymentState::UpToDate => (true, false),
                    game::ModDeploymentState::Outdated => (false, true),
                    game::ModDeploymentState::NotDeployed => (false, false),
                }
            }).unwrap_or((false, false));
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let (mod_deployed, mod_outdated) = (false, false);

            // macOS: nothing to remove (DYLD injection), Windows: DLL exists on disk
            #[cfg(target_os = "macos")]
            let mod_removable = false;
            #[cfg(target_os = "windows")]
            let mod_removable = mod_deployed || mod_outdated;
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mod_removable = false;

            GameStatus {
                installed: true,
                game_version: info.installed_version,
                mod_available,
                mod_installable: mod_available,
                mod_deployed,
                mod_outdated,
                mod_removable,
                game_running,
                launcher_running,
            }
        }
        None => {
            log_warn!("STFC not found, game features will be unavailable");
            GameStatus {
                installed: false,
                game_version: None,
                mod_available,
                mod_installable: false,
                mod_deployed: false,
                mod_outdated: false,
                mod_removable: false,
                game_running: false,
                launcher_running,
            }
        }
    };

    // Kick off an async update check if the game is installed
    if result.installed {
        thread::spawn(move || {
            match check_for_update() {
                Ok(check) => { let _ = app.emit("update-check", check); }
                Err(_) => { let _ = app.emit("update-check-failed", ()); }
            }
        });
    }

    result
}

/// Prepare the mod for use: patch entitlements on macOS, deploy the DLL on Windows.
///
/// Returns the refreshed game status so the frontend can update in one step.
#[tauri::command]
pub fn prepare_mod(app: tauri::AppHandle) -> Result<GameStatus, String> {
    let info = game::detect().ok_or("STFC not found")?;

    if game::is_running(&info.executable) {
        return Err("Cannot prepare mod while the game is running".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        game::entitlements::patch(&info.executable)?;
    }

    #[cfg(target_os = "windows")]
    {
        let mod_library = game::find_mod_library(&app)
            .ok_or("Mod library not found — run build:mod first")?;
        game::deploy_mod(&info.install_dir, &mod_library)?;
    }

    Ok(get_game_status(app))
}

/// Remove the deployed mod from the game directory after user confirmation.
///
/// Shows a warning dialogue explaining that the game will only be launchable via the Scopely Launcher afterwards.
/// Returns the refreshed game status regardless of whether the user confirmed or cancelled.
#[tauri::command]
pub fn remove_mod(window: tauri::WebviewWindow) -> Result<GameStatus, String> {
    // macOS: mod is injected via DYLD at launch, nothing to remove from disk
    #[cfg(not(target_os = "windows"))]
    #[allow(clippy::needless_return)]
    {
        log_warn!("remove_mod called on macOS, this should not happen");
        return Ok(get_game_status(window.app_handle().clone()));
    }

    #[cfg(target_os = "windows")]
    {
        let info = game::detect().ok_or("STFC not found")?;

        if game::is_running(&info.executable) {
            return Err("Cannot remove mod while the game is running".to_string());
        }

        let confirmed = window.dialog()
            .message("Remove the Community Mod?\n\n\
                      After removal, the game can only be launched through the Scopely Launcher.")
            .title("Remove Mod")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Remove".into(), "Cancel".into()))
            .blocking_show();

        if !confirmed {
            log_info!("Mod removal cancelled by user");
            return Ok(get_game_status(window.app_handle().clone()));
        }

        log_info!("User confirmed mod removal");
        game::remove_mod(&info.install_dir)?;
        Ok(get_game_status(window.app_handle().clone()))
    }
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

/// Check whether a game update is available by comparing the local `.version` file against the Scopely update API.
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

/// Lightweight process check for polling. Only runs process listing (`pgrep`/`tasklist`), no filesystem I/O.
#[derive(Clone, Serialize, TS)]
#[ts(export)]
pub struct ProcessStatus {
    /// Whether the game process is currently running.
    pub game_running: bool,
    /// Whether the Scopely launcher is currently running.
    pub launcher_running: bool,
}

/// Open the Scopely launcher so the user can install an update.
#[tauri::command]
pub fn launch_updater(_app: tauri::AppHandle) -> Result<(), String> {
    game::launcher::open_updater()?;
    Ok(())
}

/// Launch the game with the mod library injected.
///
/// On macOS, checks entitlements before launching. On Windows, auto-deploys the DLL if needed.
#[tauri::command]
pub fn launch_game(app: tauri::AppHandle) -> Result<(), String> {
    let info = game::detect().ok_or("STFC not found")?;

    let mod_library = game::find_mod_library(&app)
        .ok_or("Mod library not found — run build:mod first")?;

    // macOS: entitlements must be patched before launching
    #[cfg(target_os = "macos")]
    {
        let status = game::entitlements::check(&info.executable);
        if !status.all_granted() {
            let names: Vec<_> = status.missing.iter()
                .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
                .collect();
            return Err(format!("Missing entitlements: {} — patch them first", names.join(", ")));
        }
    }

    game::launcher::launch(&info, &mod_library)?;
    Ok(())
}
