//! In-memory tracking of whether the game or launcher was started by Daystrom.
//!
//! This module provides the central `should_block_quit()` predicate that all quit paths use to
//! decide whether exiting the app should be prevented. Quit is blocked only when a process that
//! **we** started is still running.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::game;

/// Whether the game was launched via Daystrom's "Launch Game" button.
static GAME_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Whether the Scopely launcher was opened via Daystrom's "Update" button.
static LAUNCHER_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Mark the game as having been started by Daystrom.
///
/// Called from [`crate::commands::launch_game`] after a successful spawn.
pub fn mark_game_started() {
    GAME_STARTED_BY_US.store(true, Ordering::SeqCst);
}

/// Mark the launcher as having been started by Daystrom.
///
/// Called from [`crate::commands::launch_updater`] after a successful spawn.
pub fn mark_launcher_started() {
    LAUNCHER_STARTED_BY_US.store(true, Ordering::SeqCst);
}

/// Clear the game-started flag.
///
/// Called from [`crate::monitor`] when the game process exits.
pub fn clear_game_started() {
    GAME_STARTED_BY_US.store(false, Ordering::SeqCst);
}

/// Clear the launcher-started flag.
///
/// Called from [`crate::monitor`] when the launcher process exits.
pub fn clear_launcher_started() {
    LAUNCHER_STARTED_BY_US.store(false, Ordering::SeqCst);
}

/// Whether quitting the app should be blocked.
///
/// Returns `true` when a process that Daystrom started is still running. Externally started
/// processes do not block quit.
pub fn should_block_quit() -> bool {
    (game::is_game_running() && GAME_STARTED_BY_US.load(Ordering::SeqCst))
        || (game::is_launcher_running() && LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst))
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serialize tests that touch shared atomic statics.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Reset both flags to their default state.
    fn reset_flags() {
        GAME_STARTED_BY_US.store(false, Ordering::SeqCst);
        LAUNCHER_STARTED_BY_US.store(false, Ordering::SeqCst);
    }

    #[test]
    fn mark_and_clear_game_flag() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        assert!(!GAME_STARTED_BY_US.load(Ordering::SeqCst));
        mark_game_started();
        assert!(GAME_STARTED_BY_US.load(Ordering::SeqCst));
        clear_game_started();
        assert!(!GAME_STARTED_BY_US.load(Ordering::SeqCst));
    }

    #[test]
    fn mark_and_clear_launcher_flag() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        assert!(!LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
        mark_launcher_started();
        assert!(LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
        clear_launcher_started();
        assert!(!LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
    }

    #[test]
    fn flags_are_independent() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        mark_game_started();
        assert!(GAME_STARTED_BY_US.load(Ordering::SeqCst));
        assert!(!LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));

        mark_launcher_started();
        clear_game_started();
        assert!(!GAME_STARTED_BY_US.load(Ordering::SeqCst));
        assert!(LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
    }

    #[test]
    fn should_block_quit_false_when_no_processes_running() {
        // In the test environment, no game or launcher processes are running.
        // Even with flags set, should_block_quit returns false because the
        // process check (is_game_running/is_launcher_running) is the first condition.
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        assert!(!should_block_quit());

        mark_game_started();
        mark_launcher_started();
        assert!(!should_block_quit(), "flags alone do not block quit without running processes");
    }
}
