use std::path::Path;
use std::process::{Child, Command};

use super::GameInfo;
use crate::use_log;

use_log!("Launcher");

/// Path to the Scopely launcher application on macOS.
#[cfg(target_os = "macos")]
const LAUNCHER_APP: &str = "/Applications/Star Trek Fleet Command.app";

/// Launch the game with the mod library injected via DYLD environment variables.
///
/// The child process is spawned but not awaited — the game runs independently of Project Daystrom.
/// Returns an error if the game is already running or the process fails to spawn.
#[cfg(target_os = "macos")]
pub fn launch(game: &GameInfo, mod_library: &Path, profile: Option<&str>) -> Result<Child, String> {
    let lib_dir = mod_library
        .parent()
        .ok_or_else(|| "Could not determine mod library directory".to_string())?;

    log_info!("Launching {} with mod {}", game.executable.display(), mod_library.display());

    let mut cmd = Command::new(&game.executable);
    cmd.current_dir(&game.install_dir)
        .env("DYLD_INSERT_LIBRARIES", mod_library)
        .env("DYLD_LIBRARY_PATH", lib_dir);
    if let Some(p) = profile {
        log_info!("Setting DAYSTROM_PROFILE={p}");
        cmd.env("DAYSTROM_PROFILE", p);
    }
    let child = cmd.spawn().map_err(|e| {
        log_error!("Failed to spawn game process: {e}");
        "Failed to launch game (see log for details)".to_string()
    })?;

    log_info!("Game process spawned (PID {})", child.id());
    Ok(child)
}

/// Launch the game on Windows with automatic mod DLL deployment.
///
/// If `version.dll` is missing or outdated in the game directory, the bundled DLL is copied before spawning
/// the game process.
/// Windows loads `version.dll` from the application directory automatically (DLL proxy injection).
///
/// `profile` sets the `DAYSTROM_PROFILE` environment variable for the game process:
/// - `None`: first start (Registry import mode)
/// - `Some("106_Nabor")`: known profile
/// - `Some("new_account")`: new account mode
#[cfg(target_os = "windows")]
pub fn launch(game: &GameInfo, mod_library: &Path, profile: Option<&str>) -> Result<Child, String> {
    // Auto-deploy: copy the bundled DLL if missing or outdated
    match super::check_mod_deployment(&game.install_dir, mod_library) {
        super::ModDeploymentState::UpToDate => {}
        super::ModDeploymentState::Outdated | super::ModDeploymentState::NotDeployed => {
            log_info!("Deploying mod DLL to {}", game.install_dir.display());
            super::deploy_mod(&game.install_dir, mod_library)?;
        }
    }

    log_info!("Launching {}", game.executable.display());

    let mut cmd = Command::new(&game.executable);
    cmd.current_dir(&game.install_dir);
    if let Some(p) = profile {
        log_info!("Setting DAYSTROM_PROFILE={p}");
        cmd.env("DAYSTROM_PROFILE", p);
    }
    let child = cmd.spawn().map_err(|e| {
        log_error!("Failed to spawn game process: {e}");
        "Failed to launch game (see log for details)".to_string()
    })?;

    log_info!("Game process spawned (PID {})", child.id());
    Ok(child)
}

/// Stub — game launching is not yet supported on this platform.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn launch(_game: &GameInfo, _mod_library: &Path, _profile: Option<&str>) -> Result<Child, String> {
    Err("Game launching is not yet supported on this platform".to_string())
}

/// Open the Scopely launcher so the user can install a game update.
///
/// On macOS, uses `open` to launch the `.app` bundle.
/// On Windows, locates the launcher executable via `find_launcher()` and spawns it directly.
pub fn open_updater() -> Result<(), String> {
    log_info!("Opening Scopely launcher for update");

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(LAUNCHER_APP)
            .spawn()
            .map_err(|e| {
                log_error!("Failed to open Scopely launcher: {e}");
                "Failed to open launcher (see log for details)".to_string()
            })?;
    }

    #[cfg(target_os = "windows")]
    {
        let launcher = super::windows::find_launcher()
            .ok_or_else(|| "Could not locate the Scopely launcher on this system".to_string())?;
        Command::new(&launcher)
            .spawn()
            .map_err(|e| {
                log_error!("Failed to open Scopely launcher: {e}");
                "Failed to open launcher (see log for details)".to_string()
            })?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        return Err("Opening the launcher is not supported on this platform".to_string());
    }

    Ok(())
}
