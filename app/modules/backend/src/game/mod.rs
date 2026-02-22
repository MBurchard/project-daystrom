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

use_log!("Game");

/// Location of an STFC installation on the local machine.
pub struct GameInfo {
    /// Root directory of the game installation (the `GAME_PATH` from Xsolla's launcher settings).
    pub install_dir: PathBuf,
    /// Full path to the game's main executable binary.
    pub executable: PathBuf,
}

/// Detect whether STFC is installed on this machine.
/// Returns `None` if the game is not found — errors are logged internally and never block startup.
pub fn detect() -> Option<GameInfo> {
    #[cfg(target_os = "macos")]
    {
        macos::detect()
    }

    #[cfg(not(target_os = "macos"))]
    {
        log::warn!("Game detection not implemented for this platform");
        None
    }
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

/// Check whether a process matching the given executable path is currently running.
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
