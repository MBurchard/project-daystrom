use std::path::Path;
use std::process::Command;

use super::GameInfo;
use crate::use_log;

use_log!("Launcher");

/// Path to the Scopely launcher application.
const LAUNCHER_APP: &str = "/Applications/Star Trek Fleet Command.app";

/// Launch the game with the mod library injected via DYLD environment variables.
///
/// The child process is spawned but not awaited — the game runs independently of Project Daystrom.
/// Returns an error if the game is already running or the process fails to spawn.
pub fn launch(game: &GameInfo, dylib: &Path) -> Result<(), String> {
    if super::is_running(&game.executable) {
        return Err("Game is already running".to_string());
    }

    let dylib_dir = dylib
        .parent()
        .ok_or_else(|| "Could not determine dylib directory".to_string())?;

    log_info!("Launching {} with mod {}", game.executable.display(), dylib.display());

    Command::new(&game.executable)
        .env("DYLD_INSERT_LIBRARIES", dylib)
        .env("DYLD_LIBRARY_PATH", dylib_dir)
        .spawn()
        .map_err(|e| {
            log_error!("Failed to spawn game process: {e}");
            "Failed to launch game (see log for details)".to_string()
        })?;

    log_info!("Game process spawned");
    Ok(())
}

/// Open the Scopely launcher so the user can install a game update.
///
/// Uses macOS `open` to launch the app. The launcher runs independently of Project Daystrom.
// TODO(windows): Use the appropriate launcher path and start mechanism on Windows.
pub fn open_updater() -> Result<(), String> {
    log_info!("Opening Scopely launcher for update");
    Command::new("open")
        .arg(LAUNCHER_APP)
        .spawn()
        .map_err(|e| {
            log_error!("Failed to open Scopely launcher: {e}");
            "Failed to open launcher (see log for details)".to_string()
        })?;
    Ok(())
}
