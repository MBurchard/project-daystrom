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
mod throttle;

/// Tauri bundle identifier, read from `tauri.conf.json` at compile time.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const TAURI_IDENTIFIER: &str = env!("TAURI_IDENTIFIER");

// ---- Entrypoint -----------------------------------------------------------

/// Library constructor, executed automatically when the dylib/DLL is loaded.
///
/// Initialises the logger, then hooks `il2cpp_init` as the entry point for all further hooks.
/// The actual game hooks are installed later, inside the `il2cpp_init` callback, once the
/// IL2CPP runtime is ready.
#[cfg(not(test))]
#[ctor]
fn init() {
    logging::init();
    debug!(target: "Mod", "Project Daystrom Mod starting...");

    match hooks::il2cpp_init::install() {
        Ok(()) => debug!(target: "Mod", "il2cpp_init hook installed, waiting for game init..."),
        Err(e) => error!(target: "Mod", "Failed to hook il2cpp_init: {e}"),
    }
}
