#[cfg(not(test))]
use ctor::ctor;
#[cfg(not(test))]
use log::info;

#[cfg_attr(test, allow(dead_code))]
mod logging;

/// Tauri bundle identifier, read from `tauri.conf.json` at compile time.
#[cfg_attr(test, allow(dead_code))]
pub(crate) const TAURI_IDENTIFIER: &str = env!("TAURI_IDENTIFIER");

// ---- Entrypoint -----------------------------------------------------------

/// Library constructor, executed automatically when the dylib/DLL is loaded.
///
/// Initializes the logger and writes a "Hallo Welt" line to confirm the successful injection.
#[cfg(not(test))]
#[ctor]
fn init() {
    logging::init();
    info!(target: "Mod", "Hallo Welt");
}
