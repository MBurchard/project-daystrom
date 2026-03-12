//! In-memory tracking of whether the game or launcher was started by Daystrom.
//!
//! This module provides the central `should_block_quit()` predicate that all quit paths use to
//! decide whether exiting the app should be prevented. Quit is blocked only when a process that
//! **we** started is still running.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the game was launched via Daystrom's "Launch Game" button.
static GAME_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Whether the Scopely launcher was opened via Daystrom's "Update" button.
static LAUNCHER_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Mark the game as having been started by Daystrom.
///
/// Called from [`crate::commands::launch_game`] after successful spawn.
pub fn mark_game_started() {
    GAME_STARTED_BY_US.store(true, Ordering::SeqCst);
}

/// Mark the launcher as having been started by Daystrom.
///
/// Called from [`crate::commands::launch_updater`] after successful spawn.
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
/// Returns `true` when a process that Daystrom started is still running. The monitor clears the
/// flags when processes exit, so a simple flag check is sufficient. No expensive process lookups
/// on the UI thread.
pub fn should_block_quit() -> bool {
    GAME_STARTED_BY_US.load(Ordering::SeqCst)
        || LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst)
}

/// Whether the game-started flag is set.
pub fn is_game_started() -> bool {
    GAME_STARTED_BY_US.load(Ordering::SeqCst)
}

/// Whether the launcher-started flag is set.
pub fn is_launcher_started() -> bool {
    LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst)
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
    fn should_block_quit_follows_flags() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        assert!(!should_block_quit());

        mark_game_started();
        assert!(should_block_quit(), "game flag alone blocks quit");

        clear_game_started();
        assert!(!should_block_quit());

        mark_launcher_started();
        assert!(should_block_quit(), "launcher flag alone blocks quit");

        clear_launcher_started();
        assert!(!should_block_quit());
    }
}
