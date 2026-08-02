use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{debug, info};

use crate::il2cpp::compatibility;

const TRACE_LOG_TARGET: &str = "Trace";
const LOG_TARGET: &str = "HookEngine";

pub(crate) mod tracker;

mod cargo_view;
pub(crate) mod chat_frame;
mod fleet_bar;
mod fleet_scanner;
pub(crate) mod hotkeys;
pub mod il2cpp_init;
mod interstitial;
pub(crate) mod job_queue;
mod main_action;
pub(crate) mod main_thread;
pub(crate) mod navigation_view;
mod player_prefs;
mod shop_reveal;
pub(crate) mod system_zoom;
mod target_viewer;
pub(crate) mod toast_banner;
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

/// Log interval for PlayerPrefs operations. Each `op:key` combination is logged at most once per interval, then
/// suppressed until the interval elapses.
const LOG_INTERVAL: Duration = Duration::from_secs(60);

/// Dedup state: maps each op:key combination to the last time it was logged.
static DEDUP: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Check if an op+key combination should be logged (time-based dedup).
///
/// Returns `true` on the first call for a given combination and then once every [`LOG_INTERVAL`].
/// High-frequency keys (frame polling) are handled generically, logged once per interval, then silent.
pub fn should_log(op: &str, key: &str) -> bool {
    let mut guard = DEDUP.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    let combo = format!("{op}:{key}");

    match map.get(&combo) {
        Some(last) if now.duration_since(*last) < LOG_INTERVAL => false,
        _ => {
            map.insert(combo, now);
            true
        }
    }
}

/// Log a PlayerPrefs operation in trace mode (deduped).
pub fn trace_log(op: &str, key: &str, detail: &str) {
    if !should_log(op, key) {
        return;
    }
    info!(target: TRACE_LOG_TARGET, "{op} \"{key}\" {detail}");
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

    compatibility::initialize(api);

    user_profile::install(api);
    player_prefs::install(api);
    ui_scale::install(api);
    hotkeys::install(api);
    cargo_view::install(api);
    chat_frame::install(api);
    job_queue::install(api);
    toast_banner::install(api);
    system_zoom::install(api);
    shop_reveal::install(api);
    interstitial::install(api);
    fleet_scanner::install(api);

    debug!(target: LOG_TARGET, "Hook installation complete");
}
