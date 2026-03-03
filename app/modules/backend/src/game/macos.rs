use std::path::PathBuf;

use crate::use_log;

use_log!("GameDetect");

/// Path to the Scopely launcher settings file, relative to the user's home directory.
const LAUNCHER_SETTINGS_PATH: &str =
    "Library/Preferences/Star Trek Fleet Command/launcher_settings.ini";

/// Path to the game executable, relative to the install directory.
const EXECUTABLE_REL: &str =
    "Star Trek Fleet Command.app/Contents/MacOS/Star Trek Fleet Command";

/// Locate the STFC installation by reading the Scopely launcher settings INI.
///
/// Returns the install directory and executable path as a tuple, or `None`
/// (with debug/warn logging) if the settings file is missing, the game path
/// key is absent, or the executable does not exist on disk.
pub fn detect() -> Option<(PathBuf, PathBuf)> {
    let home = dirs::home_dir()?;
    let ini_path = home.join(LAUNCHER_SETTINGS_PATH);
    log_debug!("Looking for launcher settings at {}", ini_path.display());

    let content = std::fs::read_to_string(&ini_path)
        .map_err(|e| log_debug!("Could not read launcher settings: {e}"))
        .ok()?;

    let raw_path = super::read_game_path(&content)?;
    log_debug!("Raw GAME_PATH value: {raw_path}");

    // Scopely launcher quirk: path may start with "//" instead of "/"
    let normalised = if raw_path.starts_with("//") {
        raw_path.strip_prefix('/').unwrap_or(raw_path)
    } else {
        raw_path
    };

    let install_dir = PathBuf::from(normalised);
    let executable = install_dir.join(EXECUTABLE_REL);

    if !executable.exists() {
        log_warn!(
            "Install directory found but executable missing: {}",
            executable.display()
        );
        return None;
    }

    Some((install_dir, executable))
}
