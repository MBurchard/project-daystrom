//! In-memory tracking of whether the game or launcher was started by Daystrom.
//!
//! Provides atomic flags that survive across monitor ticks. The flags are synced into the
//! [`GameStatus`](crate::commands::GameStatus) stored by the commands and monitor, where the
//! `should_block_quit` derived field replaces the former predicate function.
//!
//! Additionally, tracks launched game profiles by PID so that each profile button can
//! independently show whether its game instance is running.

use std::collections::HashMap;
use std::process::Child;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the game was launched via Daystrom's "Launch Game" button.
static GAME_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Whether the Scopely launcher was opened via Daystrom's "Update" button.
static LAUNCHER_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Map of PID → (child handle, profile stem) for game instances launched by Daystrom.
///
/// Storing the [`Child`] handle allows us to call [`Child::try_wait`] to reap exited processes,
/// preventing zombies on Unix/macOS. Without reaping, `pgrep` would still find zombie processes
/// and report them as running.
static LAUNCHED_PROFILES: Mutex<Option<HashMap<u32, (Child, String)>>> = Mutex::new(None);

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

/// Register a launched game instance with its child handle and profile.
///
/// Called from [`crate::commands::launch_game`] after spawning the process.
pub fn register_launch(child: Child, profile_stem: String) {
    let pid = child.id();
    let mut guard = LAUNCHED_PROFILES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(pid, (child, profile_stem));
}

/// Reap zombie child processes spawned by Daystrom.
///
/// Calls [`Child::try_wait`] on each tracked process. Exited processes are removed from the
/// map, which also reaps them from the kernel's process table (preventing zombies on macOS).
/// Must be called before `pgrep`-based detection to avoid false positives.
pub fn reap_children() {
    let mut guard = LAUNCHED_PROFILES.lock().unwrap();
    let Some(map) = guard.as_mut() else { return };
    map.retain(|_, (child, _)| matches!(child.try_wait(), Ok(None)));
}

/// Update the stored stem for a running profile after an in-game rename.
///
/// Called by the monitor when a running stem no longer matches any profile file, but a profile with the same
/// server ID exists under a new name.
pub fn update_stem(old_stem: &str, new_stem: &str) {
    let mut guard = LAUNCHED_PROFILES.lock().unwrap();
    let Some(map) = guard.as_mut() else { return };
    for (_, stem) in map.values_mut() {
        if *stem == old_stem {
            *stem = new_stem.to_string();
            break;
        }
    }
}

/// Return the profile stems of all game instances that are still running.
///
/// Reads from the tracked process map without modifying it. Call [`reap_children`] first to
/// ensure exited processes have been removed.
pub fn running_profiles() -> Vec<String> {
    let guard = LAUNCHED_PROFILES.lock().unwrap();
    let Some(map) = guard.as_ref() else {
        return Vec::new();
    };
    map.values().map(|(_, stem)| stem.clone()).collect()
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
    fn reap_removes_exited_process() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        // Spawn a process that exits immediately
        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_Nabor".to_string());

        // Give it a moment to exit
        std::thread::sleep(std::time::Duration::from_millis(50));

        reap_children();
        assert!(running_profiles().is_empty());
    }

    #[test]
    fn update_stem_renames_matching_profile() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_OldName".to_string());

        update_stem("106_OldName", "106_NewName");

        let running = running_profiles();
        assert_eq!(running, vec!["106_NewName"]);
    }

    #[test]
    fn update_stem_noop_when_not_found() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_Nabor".to_string());

        update_stem("411_Unknown", "411_NewName");

        let running = running_profiles();
        assert_eq!(running, vec!["106_Nabor"]);
    }
}
