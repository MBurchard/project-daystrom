use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::Manager;

use crate::use_log;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod entitlements;

#[cfg(not(target_os = "macos"))]
pub mod entitlements {
    use std::path::Path;

    /// Result of checking the game executable's code-signing entitlements.
    pub struct EntitlementStatus {
        /// Entitlement keys that are present and set to `true`.
        pub granted: Vec<&'static str>,
        /// Entitlement keys that are absent or not `true`.
        pub missing: Vec<&'static str>,
    }

    impl EntitlementStatus {
        /// Returns `true` when all four required entitlements are granted.
        pub fn all_granted(&self) -> bool {
            self.missing.is_empty()
        }
    }

    /// Stub — entitlements are a macOS concept; always returns empty on other platforms.
    pub fn check(_executable: &Path) -> EntitlementStatus {
        EntitlementStatus { granted: vec![], missing: vec![] }
    }

    /// Stub — entitlement patching is only available on macOS.
    pub fn patch(_executable: &Path) -> Result<(), String> {
        Err("Entitlement patching is only supported on macOS".to_string())
    }
}
pub mod launcher;
pub mod version;

use_log!("Game");

/// Location of an STFC installation on the local machine.
pub struct GameInfo {
    /// Root directory of the game installation (the `GAME_PATH` from the Scopely launcher settings).
    pub install_dir: PathBuf,
    /// Full path to the game's main executable binary.
    pub executable: PathBuf,
    /// Installed game version from the `.version` file, if available.
    pub installed_version: Option<u32>,
}

/// Detect whether STFC is installed on this machine.
///
/// Returns `None` if the game is not found — errors are logged internally and never block startup.
/// When found, also reads the installed version from the `.version` file.
pub fn detect() -> Option<GameInfo> {
    #[cfg(target_os = "macos")]
    let base = macos::detect();

    #[cfg(not(target_os = "macos"))]
    let base: Option<(PathBuf, PathBuf)> = {
        log::warn!("Game detection not implemented for this platform");
        None
    };

    let (install_dir, executable) = base?;
    let installed_version = version::read_installed(&install_dir);
    Some(GameInfo { install_dir, executable, installed_version })
}

/// Check whether the Scopely launcher is currently running.
///
/// The launcher can modify game files (updates), so game actions should be blocked while it runs.
// TODO(windows): Detect the launcher process on Windows.
pub fn is_launcher_running() -> bool {
    Command::new("pgrep")
        .args(["-f", "Star Trek Fleet Command.app/Contents/MacOS/launcher"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Locate the bundled mod library in the app's resource directory.
/// Returns `None` if the resource directory is unavailable or the dylib does not exist.
pub fn find_mod_library(app: &tauri::AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;
    let dylib = resource_dir.join("mod/libstfc-community-patch.dylib");
    if dylib.exists() {
        Some(dylib)
    } else {
        None
    }
}

/// Check whether the STFC game process is currently running.
///
/// Uses a hardcoded process name so it can be called without filesystem I/O.
// TODO(windows): Adapt the process name for Windows.
pub fn is_game_running() -> bool {
    Command::new("pgrep")
        .args(["-f", "Star Trek Fleet Command.app/Contents/MacOS/Star Trek Fleet Command"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Check whether a process matching the given executable path is currently running.
///
/// Uses `pgrep -f` to search for the executable name.
pub fn is_running(executable: &Path) -> bool {
    let name = executable
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if name.is_empty() {
        return false;
    }
    Command::new("pgrep")
        .args(["-f", name])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
