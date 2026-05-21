//! Central game status store with automatic change-based event emission.
//!
//! Acts like a reactive store (similar to Pinia in the frontend): any part of the backend can
//! update individual fields, and changes are automatically emitted to the frontend as a
//! `game-status` event. The store deduplicates: if an update doesn't change anything, no event
//! is emitted.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Emitter;

use crate::commands::GameStatus;
use crate::use_log;

use_log!("GameState");

/// The global game status, updated by the monitor and action commands.
static STATE: Mutex<GameStatus> = Mutex::new(GameStatus {
    installed: false,
    game_version: None,
    mod_available: false,
    mod_installable: false,
    mod_deployed: false,
    mod_outdated: false,
    mod_removable: false,
    game_running: false,
    launcher_running: false,
    remote_version: None,
    update_check_failed: false,
    game_started_by_us: false,
    launcher_started_by_us: false,
    update_available: false,
    can_launch: false,
    can_install_mod: false,
    can_remove_mod: false,
    can_launch_updater: false,
    should_block_quit: false,
    version_check_class: String::new(),
});

/// Whether the store has been populated at least once by the monitor's initial detection.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Return a snapshot of the current game status.
pub fn get() -> GameStatus {
    STATE.lock().unwrap().clone()
}

/// Whether the store has been populated with real data (not just defaults).
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}

/// Update the game status and emit a `game-status` event to the frontend if anything changed.
///
/// The updater closure receives a mutable reference to the current status. After the closure
/// returns, the store compares old and new state and emits only when something actually changed.
///
/// The mutex is released before emitting so that event listeners (e.g. tray menu sync) can
/// safely call back into the store or dispatch to the main thread without deadlocking.
pub fn update(app: &tauri::AppHandle, updater: impl FnOnce(&mut GameStatus)) {
    let changed = {
        let mut status = STATE.lock().unwrap();
        let old = status.clone();
        updater(&mut status);
        crate::commands::recompute_derived(&mut status);
        INITIALIZED.store(true, Ordering::SeqCst);
        if *status != old { Some(status.clone()) } else { None }
    };

    if let Some(payload) = changed {
        log_debug!("Status changed, emitting to frontend");
        let _ = app.emit("game-status", payload);
    }
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Reset the store to its default state. Must only be called from serialized tests.
    fn reset() {
        *STATE.lock().unwrap() = GameStatus::default();
        INITIALIZED.store(false, Ordering::SeqCst);
    }

    /// Serialize tests that touch the global store.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn get_returns_default_initially() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        let status = get();
        assert!(!status.installed);
        assert!(!status.game_running);
        assert!(!is_initialized());
    }

    #[test]
    fn is_initialized_false_by_default() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        assert!(!is_initialized());
    }
}
