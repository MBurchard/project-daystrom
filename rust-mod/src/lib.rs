#[cfg(not(test))]
use ctor::ctor;
#[cfg(not(test))]
use log::{debug, error};

#[cfg_attr(test, allow(dead_code, unused_imports))]
mod hook;
#[cfg_attr(test, allow(dead_code, unused_imports))]
mod hooks;
#[cfg_attr(test, allow(dead_code, unused_imports))]
mod il2cpp;
#[cfg_attr(test, allow(dead_code))]
mod logging;
mod profile_store;
// Windows DLL proxy: forwards version.dll API calls to the real system DLL.
// Must be compiled in so the linker sees the exported symbols from version.def.
#[cfg(target_os = "windows")]
mod proxy;
mod throttle;

/// Tauri bundle identifier, read from `tauri.conf.json` at compile time.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const TAURI_IDENTIFIER: &str = env!("TAURI_IDENTIFIER");

// ---- Entrypoint -----------------------------------------------------------

/// Library constructor, executed automatically when the dylib/DLL is loaded.
///
/// On Windows, it only activates when loaded by `prime.exe` (the game). Other processes that
/// happen to load version.dll from this directory get the proxy forwarding but no hooks.
///
/// Initializes the logger, then hooks `il2cpp_init` as the entry point for all further hooks.
/// The actual game hooks are installed later, inside the `il2cpp_init` callback, once the
/// IL2CPP runtime is ready.
#[cfg(not(test))]
#[ctor]
fn init() {
    if !is_game_process() {
        return;
    }

    // Without DAYSTROM_PROFILE the mod is transparent (no hooks, no logging).
    // The DLL proxy still forwards version.dll calls to the system DLL.
    if std::env::var(logging::PROFILE_ENV).unwrap_or_default().is_empty() {
        return;
    }

    logging::init();
    debug!(target: "Mod", "Project Daystrom Mod starting...");

    match hooks::il2cpp_init::install() {
        Ok(()) => debug!(target: "Mod", "il2cpp_init hook installed, waiting for game init..."),
        Err(e) => error!(target: "Mod", "Failed to hook il2cpp_init: {e}"),
    }
}

/// Check whether the current process is the STFC game executable.
///
/// On macOS this always returns `true` because injection happens via DYLD which is
/// already targeted at the game process. On Windows the DLL sits in the game directory
/// and could be loaded by any process that calls version.dll functions.
#[cfg(not(test))]
fn is_game_process() -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = std::env::current_exe().unwrap_or_default();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        name.starts_with("prime")
    }

    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}
