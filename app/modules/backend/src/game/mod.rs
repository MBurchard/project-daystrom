use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub mod entitlements;

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
