use log::debug;

mod hotkeys;
pub mod il2cpp_init;
mod player_prefs;
pub(crate) mod ui_scale;
mod user_profile;

// ---- Trace mode (dev tool) ------------------------------------------------

/// Hardcoded trace pattern. Set to `Some("substring")` to activate observation mode.
/// When active, all hooks pass through to the original, no store interaction, no TOML.
/// Only keys containing the pattern are logged. Set to `None` for normal operation.
const TRACE_PATTERN: Option<&str> = None;

/// Whether a key matches the active trace pattern.
pub fn is_trace_match(key: &str) -> bool {
    TRACE_PATTERN.is_some_and(|p| key.contains(p))
}

/// Whether the mod is in trace/observation mode (no store, pure passthrough).
pub fn is_trace_only() -> bool {
    TRACE_PATTERN.is_some()
}

// ---- Hook installation ----------------------------------------------------

/// Install all game hooks after IL2CPP has been initialized.
///
/// Called from the `il2cpp_init` hook callback. Each hook logs its own success or failure, a failed hook never prevents
/// other hooks from being installed.
pub fn install_all_hooks() {
    let Some(api) = il2cpp_init::IL2CPP_API.get() else {
        return;
    };

    user_profile::install(api);
    player_prefs::install(api);
    ui_scale::install(api);
    hotkeys::install(api);

    debug!(target: "HookEngine", "Hook installation complete");
}
