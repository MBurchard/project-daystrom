use std::collections::HashSet;
use std::sync::Mutex;

use log::{debug, info};

pub(crate) mod tracker;

pub(crate) mod chat_frame;
mod hotkeys;
pub mod il2cpp_init;
pub(crate) mod job_queue;
mod player_prefs;
mod spacebar;
pub(crate) mod ui_scale;
mod user_profile;

// ---- Trace mode (dev tool) ------------------------------------------------

/// Set to `true` to activate trace/observation mode.
/// When active, all hooks pass through to the original, no store interaction, no TOML.
const TRACE_ENABLED: bool = false;

/// Whether the mod is in trace/observation mode (no store, pure passthrough).
pub fn is_trace_only() -> bool {
    TRACE_ENABLED
}

/// Dedup state: each op:key combination is logged only once per session.
static DEDUP: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Check if an op+key combination should be logged (dedup).
///
/// Returns `false` for combinations already seen. High-frequency keys
/// (frame polling) are handled generically: logged once, then silent.
pub fn should_log(op: &str, key: &str) -> bool {
    let mut guard = DEDUP.lock().unwrap_or_else(|e| e.into_inner());
    let seen = guard.get_or_insert_with(HashSet::new);
    seen.insert(format!("{op}:{key}"))
}

/// Log a PlayerPrefs operation in trace mode (deduped).
pub fn trace_log(op: &str, key: &str, detail: &str) {
    if !should_log(op, key) {
        return;
    }
    info!(target: "Trace", "{op} \"{key}\" {detail}");
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
    chat_frame::install(api);
    job_queue::install(api);

    debug!(target: "HookEngine", "Hook installation complete");
}
