#[cfg(target_os = "windows")]
use std::io;
#[cfg(target_os = "windows")]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use sha2::{Digest, Sha256};
use tauri::Manager;

use crate::use_log;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Cached full path to the Scopely launcher executable (Windows only).
#[cfg(target_os = "windows")]
static LAUNCHER_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Cached full path to the STFC game executable (Windows only).
#[cfg(target_os = "windows")]
static GAME_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Create a `Command` that won't spawn a visible console window on Windows.
///
/// On non-Windows platforms this is equivalent to `Command::new(program)`.
pub(crate) fn silent_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// INI key (with `=` suffix) that holds the game installation directory.
const GAME_PATH_KEY: &str = "152033..GAME_PATH=";

/// Extract the GAME_PATH value from the launcher INI file.
///
/// Hand-rolled because rust-ini chokes on the binary REGION_INFO blob that the Scopely launcher writes.
fn read_game_path(content: &str) -> Option<&str> {
    for line in content.lines() {
        if let Some(value) = line.strip_prefix(GAME_PATH_KEY) {
            return Some(value);
        }
    }
    None
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
pub mod entitlements;

#[cfg(not(target_os = "macos"))]
pub mod entitlements {
    use std::path::Path;

    /// Result of checking the game executable's code-signing entitlements.
    pub struct EntitlementStatus {
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
        EntitlementStatus { missing: vec![] }
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

    #[cfg(target_os = "windows")]
    let base = windows::detect();

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base: Option<(PathBuf, PathBuf)> = {
        log::warn!("Game detection not implemented for this platform");
        None
    };

    let (install_dir, executable) = base?;
    let installed_version = version::read_installed(&install_dir);
    Some(GameInfo { install_dir, executable, installed_version })
}

/// Check whether a process matching `pattern` is currently running.
///
/// On Windows, filters `tasklist` by image name and checks stdout.
/// On macOS/Linux, uses `pgrep -f` for full command-line matching.
fn is_process_active(pattern: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        silent_command("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {pattern}"), "/NH"])
            .output()
            .map(|out| {
                out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains(pattern)
            })
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
}

/// Two-stage process check: quick `tasklist` filter, then PowerShell path verification.
///
/// Returns `true` only if a process with the given image name is running AND its executable path matches
/// `expected_path` (case-insensitive, since Windows paths are case-insensitive).
#[cfg(target_os = "windows")]
fn is_verified_process_running(image_name: &str, expected_path: &Path) -> bool {
    if !is_process_active(image_name) {
        return false;
    }
    verify_process_path(image_name, expected_path)
}

/// Verify that a running process actually lives at the expected filesystem path.
///
/// Uses `Get-Process` to retrieve the executable path of all processes matching the given image name
/// (without `.exe` suffix) and compares each line against the expected path.
#[cfg(target_os = "windows")]
fn verify_process_path(image_name: &str, expected_path: &Path) -> bool {
    let process_name = image_name.trim_end_matches(".exe");
    let ps_command = format!(
        "Get-Process -Name '{}' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Path",
        process_name
    );
    silent_command("powershell")
        .args(["-NoProfile", "-Command", &ps_command])
        .output()
        .map(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let expected = expected_path.to_string_lossy();
            stdout.lines().any(|line| line.trim().eq_ignore_ascii_case(expected.as_ref()))
        })
        .unwrap_or(false)
}

/// Check whether the Scopely launcher is currently running.
///
/// On Windows, uses a two-stage check (image name + full path verification) to avoid false positives from unrelated
/// processes named `launcher.exe`.
/// The launcher can modify game files (updates), so game actions should be blocked while it runs.
pub fn is_launcher_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = LAUNCHER_PATH.get_or_init(|| windows::find_launcher());
        let Some(path) = path.as_ref() else { return false };
        return is_verified_process_running("launcher.exe", path);
    }
    #[cfg(not(target_os = "windows"))]
    is_process_active("Star Trek Fleet Command.app/Contents/MacOS/launcher")
}

/// Locate the bundled mod library in the app's resource directory.
/// Returns `None` if the resource directory is unavailable or the library does not exist.
pub fn find_mod_library(app: &tauri::AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;

    #[cfg(target_os = "macos")]
    let library = resource_dir.join("mod/libstfc-community-patch.dylib");
    #[cfg(target_os = "windows")]
    let library = resource_dir.join("mod/stfc-community-patch.dll");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let library = resource_dir.join("mod/libstfc-community-patch.so");

    if library.exists() {
        Some(library)
    } else {
        None
    }
}

/// Compute the SHA-256 digest of a file by streaming it in 8 KB chunks.
///
/// Returns the 32-byte hash or an I/O error if the file cannot be read.
#[cfg(target_os = "windows")]
pub fn file_sha256(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// Result of checking the mod deployment in the game directory.
#[cfg(target_os = "windows")]
pub enum ModDeploymentState {
    /// No version.dll found in the game directory.
    NotDeployed,
    /// version.dll exists, but its hash does not match the bundled library.
    Outdated,
    /// version.dll exists and matches the bundled library.
    UpToDate,
}

/// Check whether the bundled mod library is deployed and up to date in the game directory.
///
/// Compares the SHA-256 hash of the bundled library against `install_dir/version.dll`.
/// Returns a `ModDeploymentState` indicating whether the DLL is missing, outdated, or current.
#[cfg(target_os = "windows")]
pub fn check_mod_deployment(install_dir: &Path, mod_library: &Path) -> ModDeploymentState {
    let deployed = install_dir.join("version.dll");
    if !deployed.exists() {
        log_info!("No version.dll found in {}", install_dir.display());
        return ModDeploymentState::NotDeployed;
    }
    let Ok(hash_bundled) = file_sha256(mod_library) else {
        log_warn!("Could not hash bundled mod library {}", mod_library.display());
        return ModDeploymentState::NotDeployed;
    };
    let Ok(hash_deployed) = file_sha256(&deployed) else {
        log_warn!("Could not hash deployed version.dll in {}", install_dir.display());
        return ModDeploymentState::NotDeployed;
    };
    if hash_bundled == hash_deployed {
        log_info!("version.dll is up to date");
        ModDeploymentState::UpToDate
    } else {
        log_info!("version.dll is outdated (hash mismatch)");
        ModDeploymentState::Outdated
    }
}

/// Deploy the bundled mod library as `version.dll` into the game directory.
///
/// Copies the file and returns an error string on failure.
#[cfg(target_os = "windows")]
pub fn deploy_mod(install_dir: &Path, mod_library: &Path) -> Result<(), String> {
    let target = install_dir.join("version.dll");
    std::fs::copy(mod_library, &target).map_err(|e| {
        log_error!("Failed to deploy mod to {}: {e}", target.display());
        format!("Failed to deploy mod: {e}")
    })?;
    log_info!("Deployed mod to {}", target.display());
    Ok(())
}

/// Remove the deployed mod and its runtime artifacts from the game directory.
///
/// Deletes `version.dll`, `community_patch.log`, and `community_patch_runtime.vars`.
/// The settings file (`community_patch_settings.toml`) is intentionally kept.
/// Returns an error if `version.dll` does not exist or cannot be deleted.
#[cfg(target_os = "windows")]
pub fn remove_mod(install_dir: &Path) -> Result<(), String> {
    let dll = install_dir.join("version.dll");
    std::fs::remove_file(&dll).map_err(|e| {
        log_error!("Failed to remove mod from {}: {e}", dll.display());
        format!("Failed to remove mod: {e}")
    })?;
    log_info!("Removed {}", dll.display());

    // Clean up runtime artefacts (best-effort may not exist if mod was never run)
    for name in ["community_patch.log", "community_patch_runtime.vars"] {
        let path = install_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => log_info!("Removed {}", path.display()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => log_warn!("Could not remove {}: {e}", path.display()),
        }
    }

    Ok(())
}

/// Check whether the STFC game process is currently running.
///
/// On Windows, uses a two-stage check (image name + full path verification) to avoid false positives from unrelated
/// processes named `prime.exe`.
pub fn is_game_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = GAME_PATH.get_or_init(|| detect().map(|info| info.executable));
        let Some(path) = path.as_ref() else { return false };
        return is_verified_process_running("prime.exe", path);
    }
    #[cfg(not(target_os = "windows"))]
    is_process_active("Star Trek Fleet Command.app/Contents/MacOS/Star Trek Fleet Command")
}

/// Check whether a process matching the given executable path is currently running.
///
/// On Windows, uses a two-stage check (image name + full path verification).
/// On macOS/Linux, uses `pgrep -f` for full command-line matching.
pub fn is_running(executable: &Path) -> bool {
    let name = executable.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.is_empty() {
        return false;
    }
    #[cfg(target_os = "windows")]
    return is_verified_process_running(name, executable);
    #[cfg(not(target_os = "windows"))]
    is_process_active(name)
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_game_path_normal() {
        let ini = "[General]\n152033..GAME_PATH=C:/Games/STFC/\n";
        assert_eq!(read_game_path(ini), Some("C:/Games/STFC/"));
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
152033..GAME_PATH=D:/Games/STFC/
152033..GAME_TEMP_PATH=C:/Temp/stfc/
LANGUAGE=de";
        assert_eq!(read_game_path(ini), Some("D:/Games/STFC/"));
    }

    #[test]
    fn read_game_path_survives_binary_blob() {
        let ini = "\
[General]
152033..GAME_PATH=C:/Games/STFC/
REGION_INFO=\"@Variant(\\0\\0\\0\\b\\0\\0)\"
LANGUAGE=de";
        assert_eq!(read_game_path(ini), Some("C:/Games/STFC/"));
    }
}
