use serde::Serialize;
use tauri::Manager;
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
    /// Whether the mod library was found in the app's resource directory.
    pub mod_available: bool,
    /// Whether the mod is deployed and ready (macOS: entitlements OK, Windows: DLL up to date).
    pub mod_deployed: bool,
    /// Whether the mod DLL exists but is outdated (hash mismatch). Always `false` on macOS.
    pub mod_outdated: bool,
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

            GameStatus {
                installed: true,
                install_dir: Some(info.install_dir.display().to_string()),
                executable: Some(info.executable.display().to_string()),
                game_version: info.installed_version,
                entitlements_ok: status.all_granted(),
                granted_entitlements: status.granted.iter().map(|s| s.to_string()).collect(),
                missing_entitlements: status.missing.iter().map(|s| s.to_string()).collect(),
                mod_available,
                mod_deployed,
                mod_outdated,
                game_running,
                launcher_running,
            }
        }
        None => {
            log_warn!("STFC not found, game features will be unavailable");
            GameStatus {
                installed: false,
                install_dir: None,
                executable: None,
                game_version: None,
                entitlements_ok: false,
                granted_entitlements: vec![],
                missing_entitlements: vec![],
                mod_available,
                mod_deployed: false,
                mod_outdated: false,
                game_running: false,
                launcher_running,
            }
        }
    };

    if result.game_running || result.launcher_running {
        crate::monitor::start(app, result.game_running, result.launcher_running);
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
        return Ok(get_game_status(window.app_handle().clone()));
    }

    #[cfg(target_os = "macos")]
    {
        // TODO: macOS mod is injected via DYLD at launch, nothing to remove from disk
    }

    #[cfg(target_os = "windows")]
    {
        game::remove_mod(&info.install_dir)?;
    }

    Ok(get_game_status(window.app_handle().clone()))
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
/// On macOS, checks entitlements before launching. On Windows, auto-deploys the DLL if needed.
/// Starts background process monitoring after a successful launch.
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
    crate::monitor::start(app, true, false);
    Ok(())
}
