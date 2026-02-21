use std::path::Path;
use std::process::Command;

use super::GameInfo;
use crate::use_log;

use_log!("Launcher");

/// Launch the game with the mod library injected via DYLD environment variables.
///
/// The child process is spawned but not awaited — the game runs independently of Skynet.
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
        .map_err(|e| format!("Failed to launch game: {e}"))?;

    log_info!("Game process spawned");
    Ok(())
}
