//! In-memory tracking of whether the game or launcher was started by Daystrom.
//!
//! Provides atomic flags that survive across monitor ticks. The flags are synced into the
//! [`GameStatus`](crate::commands::GameStatus) store by the commands and monitor, where the
//! `should_block_quit` derived field replaces the former predicate function.
//!
//! Additionally tracks launched game profiles by PID so that each profile button can
//! independently show whether its game instance is running.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether the game was launched via Daystrom's "Launch Game" button.
static GAME_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Whether the Scopely launcher was opened via Daystrom's "Update" button.
static LAUNCHER_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Map of PID → profile stem for game instances launched by Daystrom.
static LAUNCHED_PROFILES: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

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
/// Called from [`crate::monitor`] when no game processes launched by us are running.
pub fn clear_game_started() {
    GAME_STARTED_BY_US.store(false, Ordering::SeqCst);
}

/// Clear the launcher-started flag.
///
/// Called from [`crate::monitor`] when the launcher process exits.
pub fn clear_launcher_started() {
    LAUNCHER_STARTED_BY_US.store(false, Ordering::SeqCst);
}

/// Whether the game was started by Daystrom (atomic read).
pub fn is_game_started() -> bool {
    GAME_STARTED_BY_US.load(Ordering::SeqCst)
}

/// Register a launched game instance with its profile.
///
/// Called from [`crate::commands::launch_game`] after spawning the process.
pub fn register_launch(pid: u32, profile_stem: String) {
    let mut guard = LAUNCHED_PROFILES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(pid, profile_stem);
}

/// Return the profile stems of all game instances that are still running.
///
/// Checks each tracked PID and removes dead ones. Called by the monitor
/// every 2 seconds.
pub fn running_profiles() -> Vec<String> {
    let mut guard = LAUNCHED_PROFILES.lock().unwrap();
    let Some(map) = guard.as_mut() else {
        return Vec::new();
    };
    map.retain(|&pid, _| is_pid_alive(pid));
    map.values().cloned().collect()
}

/// Check whether a process with the given PID is still alive.
#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|out| {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.contains(&pid.to_string())
        })
        .unwrap_or(false)
}

/// Check whether a process with the given PID is still alive.
#[cfg(target_os = "macos")]
fn is_pid_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Check whether a process with the given PID is still alive.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn is_pid_alive(_pid: u32) -> bool {
    false
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that touch shared atomic statics.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Reset both flags to their default state.
    fn reset_flags() {
        GAME_STARTED_BY_US.store(false, Ordering::SeqCst);
        LAUNCHER_STARTED_BY_US.store(false, Ordering::SeqCst);
        *LAUNCHED_PROFILES.lock().unwrap() = None;
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
    fn register_and_query_profiles() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        register_launch(99999, "106_Nabor".to_string());
        // PID 99999 is almost certainly not alive
        let running = running_profiles();
        assert!(running.is_empty());
    }
}
