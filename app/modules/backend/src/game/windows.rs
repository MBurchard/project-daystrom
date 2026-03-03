use std::path::PathBuf;

use crate::use_log;

use_log!("GameDetect");

/// Path to the Scopely launcher settings file, relative to `%LOCALAPPDATA%`.
const LAUNCHER_SETTINGS_PATH: &str =
    "Star Trek Fleet Command/launcher_settings.ini";

/// Name of the game executable on Windows.
const EXECUTABLE_NAME: &str = "prime.exe";

/// Name of the Scopely launcher executable on Windows.
const LAUNCHER_EXECUTABLE: &str = "launcher.exe";

/// Path to the Scopely launcher directory, relative to `%LOCALAPPDATA%`.
const LAUNCHER_DIR: &str = "Star Trek Fleet Command";

/// Registry uninstall key where the Scopely launcher registers itself.
const UNINSTALL_REG_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Star Trek Fleet Command";

/// Read the game install directory from the Scopely launcher settings INI.
///
/// Parses `%LOCALAPPDATA%\Star Trek Fleet Command\launcher_settings.ini` and extracts
/// the `GAME_PATH` value. Returns `None` if the file or key is missing.
fn read_install_dir() -> Option<PathBuf> {
    let local_app_data = dirs::data_local_dir()?;
    let ini_path = local_app_data.join(LAUNCHER_SETTINGS_PATH);
    log_debug!("Looking for launcher settings at {}", ini_path.display());

    let content = std::fs::read_to_string(&ini_path)
        .map_err(|e| log_debug!("Could not read launcher settings: {e}"))
        .ok()?;

    let raw_path = super::read_game_path(&content)?;
    log_debug!("Raw GAME_PATH value: {raw_path}");
    Some(PathBuf::from(raw_path))
}

/// Locate the Scopely launcher executable on Windows.
///
/// Searches in three places (in order):
/// 1. Relative to the game install path from `launcher_settings.ini` (two levels up from `GAME_PATH`)
/// 2. Standard `%LOCALAPPDATA%\Star Trek Fleet Command\` directory
/// 3. Registry `InstallLocation` from the uninstall key
///
/// Returns `None` if none of the methods finds the launcher.
pub fn find_launcher() -> Option<PathBuf> {
    // 1. Derive from GAME_PATH: the launcher sits in the STFC root, two levels above the game dir
    //    (e.g. GAME_PATH = "D:/Programme/STFC/default/game/" -> root = "D:/Programme/STFC/")
    if let Some(install_dir) = read_install_dir() {
        let root = install_dir.parent().and_then(|p| p.parent());
        if let Some(root) = root {
            let launcher = root.join(LAUNCHER_EXECUTABLE);
            if launcher.exists() {
                log_debug!("Found launcher relative to game path: {}", launcher.display());
                return Some(launcher);
            }
            log_debug!("Launcher not at derived path: {}", launcher.display());
        }
    }

    // 2. Standard path: %LOCALAPPDATA%\Star Trek Fleet Command\launcher.exe
    if let Some(local_app_data) = dirs::data_local_dir() {
        let standard = local_app_data.join(LAUNCHER_DIR).join(LAUNCHER_EXECUTABLE);
        if standard.exists() {
            log_debug!("Found launcher at standard path: {}", standard.display());
            return Some(standard);
        }
        log_debug!("Launcher not at standard path: {}", standard.display());
    }

    // 3. Registry fallback: read InstallLocation from the uninstall key
    if let Some(path) = find_launcher_via_registry() {
        return Some(path);
    }

    log_warn!("Could not locate the Scopely launcher");
    None
}

/// Query the Windows registry for the launcher's install location.
///
/// Uses `reg query` to avoid an external crate dependency.
fn find_launcher_via_registry() -> Option<PathBuf> {
    let output = super::silent_command("reg")
        .args(["query", UNINSTALL_REG_KEY, "/v", "InstallLocation"])
        .output()
        .map_err(|e| log_debug!("reg query failed: {e}"))
        .ok()?;

    if !output.status.success() {
        log_debug!("reg query returned non-zero status");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "    InstallLocation    REG_SZ    C:\path\to\launcher"
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("InstallLocation") {
            // Skip the "REG_SZ" type token and surrounding whitespace
            if let Some(value) = rest.trim().strip_prefix("REG_SZ") {
                let install_dir = PathBuf::from(value.trim());
                let launcher = install_dir.join(LAUNCHER_EXECUTABLE);
                if launcher.exists() {
                    log_debug!("Found launcher via registry: {}", launcher.display());
                    return Some(launcher);
                }
                log_debug!("Registry path found but launcher missing: {}", launcher.display());
            }
        }
    }

    None
}

/// Locate the STFC installation by reading the Scopely launcher settings INI.
///
/// Returns `None` (with debug/warn logging) if the settings file is missing,
/// the game path key is absent, or the executable does not exist on disk.
pub fn detect() -> Option<(PathBuf, PathBuf)> {
    let install_dir = read_install_dir()?;
    let executable = install_dir.join(EXECUTABLE_NAME);

    if !executable.exists() {
        log_warn!(
            "Install directory found but executable missing: {}",
            executable.display()
        );
        return None;
    }

    Some((install_dir, executable))
}
