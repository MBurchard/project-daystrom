use std::path::PathBuf;

use super::GameInfo;
use crate::use_log;

use_log!("GameDetect");

/// Path to Xsolla's launcher settings file, relative to the user's home directory.
const LAUNCHER_SETTINGS_PATH: &str =
    "Library/Preferences/Star Trek Fleet Command/launcher_settings.ini";

/// INI key (with `=` suffix) that holds the game installation directory.
const GAME_PATH_KEY: &str = "152033..GAME_PATH=";

/// Path to the game executable, relative to the install directory.
const EXECUTABLE_REL: &str =
    "Star Trek Fleet Command.app/Contents/MacOS/Star Trek Fleet Command";

/// Extract the GAME_PATH value from the launcher INI file.
/// Hand-rolled because rust-ini chokes on the binary REGION_INFO blob that Xsolla writes.
fn read_game_path(content: &str) -> Option<&str> {
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(GAME_PATH_KEY) {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_game_path_normal() {
        let ini = "[General]\n152033..GAME_PATH=//Users/me/Games/STFC/\n";
        assert_eq!(read_game_path(ini), Some("//Users/me/Games/STFC/"));
    }

    #[test]
    fn read_game_path_missing_key() {
        let ini = "[General]\nLANGUAGE=de\nAUTOUPDATE_ENABLED=true\n";
        assert_eq!(read_game_path(ini), None);
    }

    #[test]
    fn read_game_path_empty_content() {
        assert_eq!(read_game_path(""), None);
    }

    #[test]
    fn read_game_path_key_among_others() {
        let ini = "\
[General]
152033..GAME_INSTALLED=true
152033..GAME_PATH=/opt/stfc/
152033..GAME_TEMP_PATH=/tmp/stfc/
LANGUAGE=de";
        assert_eq!(read_game_path(ini), Some("/opt/stfc/"));
    }

    #[test]
    fn read_game_path_survives_binary_blob() {
        // Real-world INI with Xsolla's binary REGION_INFO that crashes rust-ini
        let ini = "\
[General]
152033..GAME_PATH=//Users/me/Games/STFC/
REGION_INFO=\"@Variant(\\0\\0\\0\\b\\0\\0)\"
LANGUAGE=de";
        assert_eq!(read_game_path(ini), Some("//Users/me/Games/STFC/"));
    }
}

/// Locate the STFC installation by reading Xsolla's launcher settings INI.
///
/// Returns `None` (with debug/warn logging) if the settings file is missing,
/// the game path key is absent, or the executable does not exist on disk.
pub fn detect() -> Option<GameInfo> {
    let home = dirs::home_dir()?;
    let ini_path = home.join(LAUNCHER_SETTINGS_PATH);
    log_debug!("Looking for launcher settings at {}", ini_path.display());

    let content = std::fs::read_to_string(&ini_path)
        .map_err(|e| log_debug!("Could not read launcher settings: {e}"))
        .ok()?;

    let raw_path = read_game_path(&content)?;
    log_debug!("Raw GAME_PATH value: {raw_path}");

    // Xsolla quirk: path may start with "//" instead of "/"
    let normalised = raw_path.strip_prefix('/').unwrap_or(raw_path);

    let install_dir = PathBuf::from(normalised);
    let executable = install_dir.join(EXECUTABLE_REL);

    if !executable.exists() {
        log_warn!(
            "Install directory found but executable missing: {}",
            executable.display()
        );
        return None;
    }

    Some(GameInfo {
        install_dir,
        executable,
    })
}
