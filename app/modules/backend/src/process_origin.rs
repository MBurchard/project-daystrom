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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::profile_protocol::{INITIAL_PROFILE_STEM, NEW_ACCOUNT_PROFILE_STEM};

/// Maximum time for a newly launched game to report its first completed UI frame.
const GAME_UI_STARTUP_GRACE: Duration = Duration::from_secs(45);

/// Whether the game was launched via Daystrom's "Launch Game" button.
static GAME_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Whether the Scopely launcher was opened via Daystrom's "Update" button.
static LAUNCHER_STARTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Monotonic identity that distinguishes tracking entries across PID reuse.
static NEXT_TRACKED_GAME_ID: AtomicU64 = AtomicU64::new(1);

/// One game process associated with a Daystrom profile.
struct TrackedGame {
    /// Stable identity of this tracking entry, independent of its operating-system PID.
    tracking_id: u64,
    /// Owned handle when this backend instance spawned the process.
    child: Option<Child>,
    /// Profile stem used by the corresponding launch button.
    profile_stem: String,
    /// Validated mod WebSocket connection currently owned by this game process.
    connection_owner: Option<u64>,
    /// Whether this process has completed at least one validated mod handshake.
    mod_confirmed: bool,
    /// Whether the connected mod can observe the first completed game UI frame.
    ui_ready_supported: Option<bool>,
    /// Whether this process has completed at least one game UI frame.
    ui_ready: bool,
    /// Deadline until which a process without a completed UI frame is presented as starting.
    startup_deadline: Option<Instant>,
    /// Whether a termination job currently owns this entry's child handle.
    termination_pending: bool,
}

/// Process termination work detached from the tracked-game map lock.
struct TerminationJob {
    pid: u32,
    tracking_id: u64,
    child: Option<Child>,
}

/// Timed-out startup condition selected for explicit user-requested termination.
#[derive(Clone, Copy)]
enum TerminationTarget {
    /// The mod connected and supports UI readiness, but never reported it.
    MissingUiReady,
    /// A Daystrom-owned process never established its first mod connection.
    MissingModHandshake,
}

/// Map of PID to tracked game metadata for instances launched by Daystrom.
///
/// Storing the [`Child`] handle allows us to call [`Child::try_wait`] to reap exited processes,
/// preventing zombies on Unix/macOS. Without reaping, `pgrep` would still find zombie processes
/// and report them as running. Reconnected games have no handle because their original Daystrom
/// parent has already exited; their process ID keeps the reconstructed entry valid across temporary
/// WebSocket disconnects.
static LAUNCHED_PROFILES: Mutex<Option<HashMap<u32, TrackedGame>>> = Mutex::new(None);

/// Revision of the tracked-game map, incremented for every successful mutation.
static TRACKED_GAMES_REVISION: AtomicU64 = AtomicU64::new(0);

/// Consistent view of all profile-related facts derived from [`LAUNCHED_PROFILES`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TrackedGamesSnapshot {
    /// Revision used to reject stale snapshots at the profile-state boundary.
    pub(crate) revision: u64,
    /// Profile stems of every tracked game process.
    pub(crate) running_profiles: Vec<String>,
    /// Profile stems still waiting for their first completed game UI frame.
    pub(crate) starting_profiles: Vec<String>,
    /// Profile stems whose game UI has reported readiness.
    pub(crate) ready_profiles: Vec<String>,
    /// Profile stems that did not complete a game UI frame before their startup deadline.
    pub(crate) failed_profiles: Vec<String>,
    /// Profile stems whose game UI readiness cannot be observed by this mod version.
    pub(crate) unclear_profiles: Vec<String>,
    /// Whether at least one game process is tracked.
    pub(crate) tracked_game: bool,
    /// Whether a process without a mod handshake has exceeded its startup deadline.
    pub(crate) expired_unconfirmed_game: bool,
    /// Whether an expired unconfirmed process is owned by a Daystrom child handle.
    pub(crate) terminable_unconfirmed_start: bool,
    /// Whether a UI-ready game is waiting for its mod to reconnect.
    pub(crate) disconnected_confirmed_game: bool,
}

/// Lock the tracked-game map and recover its valid container after an unrelated panic.
fn lock_tracked_games() -> MutexGuard<'static, Option<HashMap<u32, TrackedGame>>> {
    LAUNCHED_PROFILES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Record one tracked-game map mutation while its lock is held.
fn advance_tracked_games_revision() {
    TRACKED_GAMES_REVISION.fetch_add(1, Ordering::SeqCst);
}

/// Allocate one stable identity for a newly tracked game.
fn next_tracked_game_id() -> u64 {
    NEXT_TRACKED_GAME_ID.fetch_add(1, Ordering::Relaxed)
}

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

/// Register a launched game instance and start its game UI startup deadline.
///
/// Called from [`crate::commands::launch_game`] after spawning the process. If its handshake
/// arrived first, the existing confirmed entry remains authoritative and only receives the child
/// handle.
pub fn register_launch(child: Child, profile_stem: String) {
    let pid = child.id();
    let mut guard = lock_tracked_games();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(game) = map.get_mut(&pid)
        && game.child.is_none()
        && game.connection_owner.is_some()
    {
        game.child = Some(child);
        advance_tracked_games_revision();
        return;
    }
    map.insert(
        pid,
        TrackedGame {
            tracking_id: next_tracked_game_id(),
            child: Some(child),
            profile_stem,
            connection_owner: None,
            mod_confirmed: false,
            ui_ready_supported: None,
            ui_ready: false,
            startup_deadline: Some(Instant::now() + GAME_UI_STARTUP_GRACE),
            termination_pending: false,
        },
    );
    advance_tracked_games_revision();
}

/// Confirm a Daystrom-launched game through its WebSocket handshake.
///
/// The handshake confirms the mod connection. Existing tracking remains authoritative unless its
/// profile stem is an initial-launch or new-account placeholder, in which case the handshake
/// resolves it to the real profile stem. UI readiness is confirmed separately.
pub fn register_reconnected_launch(pid: u32, profile_stem: String, connection_id: u64) {
    let mut guard = lock_tracked_games();
    let map = guard.get_or_insert_with(HashMap::new);
    let changed = match map.get_mut(&pid) {
        Some(game) => {
            let mut changed = false;
            if matches!(game.profile_stem.as_str(), INITIAL_PROFILE_STEM | NEW_ACCOUNT_PROFILE_STEM)
                && game.profile_stem != profile_stem
            {
                game.profile_stem = profile_stem;
                changed = true;
            }
            if game.connection_owner != Some(connection_id) {
                game.connection_owner = Some(connection_id);
                changed = true;
            }
            if !game.mod_confirmed {
                game.mod_confirmed = true;
                changed = true;
            }
            changed
        }
        None => {
            map.insert(
                pid,
                TrackedGame {
                    tracking_id: next_tracked_game_id(),
                    child: None,
                    profile_stem,
                    connection_owner: Some(connection_id),
                    mod_confirmed: true,
                    ui_ready_supported: None,
                    ui_ready: false,
                    startup_deadline: Some(Instant::now() + GAME_UI_STARTUP_GRACE),
                    termination_pending: false,
                },
            );
            true
        }
    };
    if changed {
        advance_tracked_games_revision();
    }
    mark_game_started();
}

/// Confirm that a tracked game completed its first `ScreenManager` UI frame.
pub fn register_ui_ready(pid: u32, connection_id: u64) -> bool {
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else { return false };
    let Some(game) = map.get_mut(&pid) else { return false };
    if game.connection_owner != Some(connection_id) {
        return false;
    }
    if !game.ui_ready || game.startup_deadline.is_some() {
        game.ui_ready_supported = Some(true);
        game.ui_ready = true;
        game.startup_deadline = None;
        advance_tracked_games_revision();
    }
    true
}

/// Record whether the current mod connection can observe game UI readiness.
pub fn register_ui_ready_capability(pid: u32, connection_id: u64, supported: bool) -> bool {
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else { return false };
    let Some(game) = map.get_mut(&pid) else { return false };
    if game.connection_owner != Some(connection_id) {
        return false;
    }
    if game.ui_ready_supported != Some(supported) {
        game.ui_ready_supported = Some(supported);
        advance_tracked_games_revision();
    }
    true
}

/// Terminate every tracked game whose UI startup deadline has expired.
pub fn terminate_failed_starts() -> bool {
    terminate_failed_starts_at(Instant::now())
}

/// Terminate failed starts against an explicit clock reading.
fn terminate_failed_starts_at(now: Instant) -> bool {
    terminate_starts_with(now, TerminationTarget::MissingUiReady, crate::game::terminate_process_id)
}

/// Terminate Daystrom-owned starts that never established their first mod connection.
pub fn terminate_unconfirmed_starts() -> bool {
    terminate_unconfirmed_starts_at(Instant::now())
}

/// Terminate unconfirmed starts against an explicit clock reading.
fn terminate_unconfirmed_starts_at(now: Instant) -> bool {
    terminate_starts_with(now, TerminationTarget::MissingModHandshake, crate::game::terminate_process_id)
}

/// Claim matching starts, terminate them without the map lock, and reconcile the results.
fn terminate_starts_with(
    now: Instant,
    target: TerminationTarget,
    mut terminate_process: impl FnMut(u32) -> bool,
) -> bool {
    let (jobs, already_pending) = {
        let mut guard = lock_tracked_games();
        let Some(map) = guard.as_mut() else { return true };
        let mut jobs = Vec::new();
        let mut already_pending = false;
        for (pid, game) in map.iter_mut() {
            let target_matches = match target {
                TerminationTarget::MissingUiReady => game.mod_confirmed && game.ui_ready_supported == Some(true),
                TerminationTarget::MissingModHandshake => !game.mod_confirmed,
            };
            if game.ui_ready || !target_matches || !game.startup_deadline.is_some_and(|deadline| now >= deadline) {
                continue;
            }
            if game.termination_pending {
                already_pending = true;
                continue;
            }
            if matches!(target, TerminationTarget::MissingModHandshake) && game.child.is_none() {
                continue;
            }
            game.termination_pending = true;
            jobs.push(TerminationJob {
                pid: *pid,
                tracking_id: game.tracking_id,
                child: game.child.take(),
            });
        }
        (jobs, already_pending)
    };

    let results = jobs
        .into_iter()
        .map(|mut job| {
            let terminated = job.child.as_mut().map_or_else(
                || terminate_process(job.pid),
                |child| match child.try_wait() {
                    Ok(Some(_)) => true,
                    _ => child.kill().is_ok() && child.wait().is_ok(),
                },
            );
            (job, terminated)
        })
        .collect::<Vec<_>>();

    let mut all_terminated = !already_pending;
    let mut removed = false;
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else {
        return results.iter().all(|(_, terminated)| *terminated);
    };
    for (job, terminated) in results {
        all_terminated &= terminated;
        let same_entry = map.get(&job.pid).is_some_and(|game| game.tracking_id == job.tracking_id);
        if !same_entry {
            continue;
        }
        if terminated {
            map.remove(&job.pid);
            removed = true;
        } else if let Some(game) = map.get_mut(&job.pid) {
            if game.child.is_none() {
                game.child = job.child;
            }
            game.termination_pending = false;
        }
    }
    if removed {
        advance_tracked_games_revision();
    }
    all_terminated
}

/// Release a mod connection when its WebSocket closes.
///
/// A reconstructed entry remains tracked while its process is alive so a temporary disconnect is
/// distinguishable from an external game. Returns whether an exited reconstructed entry was removed.
pub fn unregister_reconnected_launch(pid: u32, connection_id: u64) -> bool {
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else { return false };
    let Some(game) = map.get_mut(&pid) else { return false };
    if game.connection_owner != Some(connection_id) {
        return false;
    }
    game.connection_owner = None;
    if game.child.is_none() && !crate::game::is_process_id_running(pid) {
        map.remove(&pid);
        advance_tracked_games_revision();
        return true;
    }
    advance_tracked_games_revision();
    false
}

/// Return whether any Daystrom-launched game is currently tracked.
pub fn has_tracked_game() -> bool {
    tracked_games_snapshot().tracked_game
}

/// Return whether a UI-ready game is waiting for its mod to reconnect.
#[cfg(test)]
pub fn has_disconnected_confirmed_game() -> bool {
    tracked_games_snapshot().disconnected_confirmed_game
}

/// Reap zombie child processes spawned by Daystrom.
///
/// Calls [`Child::try_wait`] on each tracked process. Exited processes are removed from the
/// map, which also reaps them from the kernel's process table (preventing zombies on macOS).
/// Must be called before `pgrep`-based detection to avoid false positives.
pub fn reap_children() {
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else { return };
    let previous_len = map.len();
    map.retain(|pid, game| {
        game.child.as_mut().map_or_else(
            || game.connection_owner.is_some() || crate::game::is_process_id_running(*pid),
            |child| matches!(child.try_wait(), Ok(None)),
        )
    });
    if map.len() != previous_len {
        advance_tracked_games_revision();
    }
}

/// Update the stored stem for a running profile after an in-game rename.
///
/// Called by the monitor when a running stem no longer matches any profile file, but a profile with the same
/// server ID exists under a new name.
pub fn update_stem(old_stem: &str, new_stem: &str) {
    let mut guard = lock_tracked_games();
    let Some(map) = guard.as_mut() else { return };
    for game in map.values_mut() {
        if game.profile_stem == old_stem {
            if old_stem != new_stem {
                game.profile_stem = new_stem.to_string();
                advance_tracked_games_revision();
            }
            break;
        }
    }
}

/// Return the profile stems of all game instances that are still running.
///
/// Reads from the tracked process map without modifying it. Call [`reap_children`] first to
/// ensure exited processes have been removed.
#[cfg(test)]
pub fn running_profiles() -> Vec<String> {
    tracked_games_snapshot().running_profiles
}

/// Return profile stems that are still waiting for their first completed game UI frame.
#[cfg(test)]
pub fn starting_profiles() -> Vec<String> {
    tracked_games_snapshot().starting_profiles
}

/// Return a consistent snapshot of all tracked game processes.
pub(crate) fn tracked_games_snapshot() -> TrackedGamesSnapshot {
    tracked_games_snapshot_at(Instant::now())
}

/// Return a consistent snapshot at an explicit clock reading.
pub(crate) fn tracked_games_snapshot_at(now: Instant) -> TrackedGamesSnapshot {
    let guard = lock_tracked_games();
    let revision = TRACKED_GAMES_REVISION.load(Ordering::SeqCst);
    let Some(map) = guard.as_ref() else {
        return TrackedGamesSnapshot {
            revision,
            ..TrackedGamesSnapshot::default()
        };
    };

    let running_profiles = map.values().map(|game| game.profile_stem.clone()).collect();
    let starting_profiles = map
        .values()
        .filter(|game| {
            !game.ui_ready
                && game.ui_ready_supported != Some(false)
                && game.startup_deadline.is_some_and(|deadline| now < deadline)
        })
        .map(|game| game.profile_stem.clone())
        .collect();
    let ready_profiles = map
        .values()
        .filter(|game| game.ui_ready)
        .map(|game| game.profile_stem.clone())
        .collect();
    let failed_profiles = map
        .values()
        .filter(|game| {
            !game.ui_ready
                && game.mod_confirmed
                && game.ui_ready_supported == Some(true)
                && game.startup_deadline.is_some_and(|deadline| now >= deadline)
        })
        .map(|game| game.profile_stem.clone())
        .collect();
    let unclear_profiles = map
        .values()
        .filter(|game| {
            !game.ui_ready
                && game.mod_confirmed
                && game.ui_ready_supported != Some(true)
                && game.connection_owner.is_some()
                && (game.ui_ready_supported == Some(false)
                    || game.startup_deadline.is_some_and(|deadline| now >= deadline))
        })
        .map(|game| game.profile_stem.clone())
        .collect();

    TrackedGamesSnapshot {
        revision,
        running_profiles,
        starting_profiles,
        ready_profiles,
        failed_profiles,
        unclear_profiles,
        tracked_game: !map.is_empty(),
        expired_unconfirmed_game: map
            .values()
            .any(|game| !game.mod_confirmed && game.startup_deadline.is_some_and(|deadline| now >= deadline)),
        terminable_unconfirmed_start: map.values().any(|game| {
            !game.mod_confirmed
                && (game.child.is_some() || game.termination_pending)
                && game.startup_deadline.is_some_and(|deadline| now >= deadline)
        }),
        disconnected_confirmed_game: map.values().any(|game| {
            game.mod_confirmed
                && game.connection_owner.is_none()
                && (game.ui_ready || game.ui_ready_supported != Some(true))
        }),
    }
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that touch shared atomic statics.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Synthetic PID that process detection rejects on every supported platform.
    const STOPPED_TEST_PID: u32 = 0;

    /// Acquire the shared test lock without cascading a previous test panic.
    fn lock_tests() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reset both flags to their default state.
    fn reset_flags() {
        GAME_STARTED_BY_US.store(false, Ordering::SeqCst);
        LAUNCHER_STARTED_BY_US.store(false, Ordering::SeqCst);
        let mut tracked = lock_tracked_games();
        if let Some(map) = tracked.as_mut() {
            for game in map.values_mut() {
                if let Some(child) = game.child.as_mut() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        *tracked = None;
        TRACKED_GAMES_REVISION.store(0, Ordering::SeqCst);
    }

    #[test]
    fn mark_and_clear_game_flag() {
        let _lock = lock_tests();
        reset_flags();

        assert!(!GAME_STARTED_BY_US.load(Ordering::SeqCst));
        mark_game_started();
        assert!(GAME_STARTED_BY_US.load(Ordering::SeqCst));
        clear_game_started();
        assert!(!GAME_STARTED_BY_US.load(Ordering::SeqCst));
    }

    #[test]
    fn mark_and_clear_launcher_flag() {
        let _lock = lock_tests();
        reset_flags();

        assert!(!LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
        mark_launcher_started();
        assert!(LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
        clear_launcher_started();
        assert!(!LAUNCHER_STARTED_BY_US.load(Ordering::SeqCst));
    }

    #[test]
    fn flags_are_independent() {
        let _lock = lock_tests();
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
    fn reconnect_restores_and_unregisters_profile_tracking() {
        let _lock = lock_tests();
        reset_flags();

        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);

        assert!(is_game_started());
        assert!(has_tracked_game());
        assert_eq!(running_profiles(), vec!["106_Nabor"]);
        assert!(unregister_reconnected_launch(STOPPED_TEST_PID, 1));
        assert!(!has_tracked_game());
    }

    #[test]
    fn repeated_identical_handshake_keeps_process_revision() {
        let _lock = lock_tests();
        reset_flags();

        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        let revision = tracked_games_snapshot().revision;

        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);

        assert_eq!(tracked_games_snapshot().revision, revision);
    }

    #[test]
    fn live_disconnected_game_remains_tracked_for_reconnect_detection() {
        let _lock = lock_tests();
        reset_flags();
        let pid = std::process::id();

        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready(pid, 1));
        assert!(starting_profiles().is_empty());
        assert!(!has_disconnected_confirmed_game());

        assert!(!unregister_reconnected_launch(pid, 1));
        assert!(starting_profiles().is_empty());
        assert!(has_disconnected_confirmed_game());
        reap_children();
        assert!(has_tracked_game());
    }

    #[test]
    fn disconnecting_one_reconnected_game_preserves_other_tracking() {
        let _lock = lock_tests();
        reset_flags();
        let live_pid = std::process::id();

        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        register_reconnected_launch(live_pid, "107_Spock".to_string(), 2);

        let mut running = running_profiles();
        running.sort();
        assert_eq!(running, vec!["106_Nabor", "107_Spock"]);

        assert!(unregister_reconnected_launch(STOPPED_TEST_PID, 1));
        assert!(has_tracked_game());
        assert!(is_game_started());
        assert_eq!(running_profiles(), vec!["107_Spock"]);

        assert!(!unregister_reconnected_launch(live_pid, 2));
        assert!(has_tracked_game());
    }

    #[test]
    fn disconnected_confirmed_game_is_detected_beside_connected_instance() {
        let _lock = lock_tests();
        reset_flags();
        let disconnected_pid = std::process::id();

        register_reconnected_launch(disconnected_pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready(disconnected_pid, 1));
        register_reconnected_launch(STOPPED_TEST_PID, "107_Spock".to_string(), 2);

        assert!(!unregister_reconnected_launch(disconnected_pid, 1));
        assert_eq!(running_profiles().len(), 2);
        assert!(has_disconnected_confirmed_game());
    }

    #[test]
    fn unconfirmed_game_reports_a_missing_mod_instead_of_a_failed_start() {
        let _lock = lock_tests();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_Nabor".to_string());
        register_reconnected_launch(STOPPED_TEST_PID, "107_Spock".to_string(), 2);
        assert!(register_ui_ready(STOPPED_TEST_PID, 2));
        let after_startup_grace = Instant::now() + GAME_UI_STARTUP_GRACE;

        let snapshot = tracked_games_snapshot_at(after_startup_grace);
        assert_eq!(snapshot.running_profiles.len(), 2);
        assert!(snapshot.failed_profiles.is_empty());
        assert!(snapshot.unclear_profiles.is_empty());
        assert!(snapshot.expired_unconfirmed_game);
        assert!(snapshot.terminable_unconfirmed_start);
        assert!(!snapshot.disconnected_confirmed_game);

        reset_flags();
    }

    #[test]
    fn repeated_reconnect_preserves_reconciled_profile_stem() {
        let _lock = lock_tests();
        reset_flags();

        register_reconnected_launch(STOPPED_TEST_PID, "106_OldName".to_string(), 1);
        update_stem("106_OldName", "106_NewName");
        register_reconnected_launch(STOPPED_TEST_PID, "106_OldName".to_string(), 2);

        assert_eq!(running_profiles(), vec!["106_NewName"]);
        assert!(!unregister_reconnected_launch(STOPPED_TEST_PID, 1));
        assert!(has_tracked_game());
        assert!(unregister_reconnected_launch(STOPPED_TEST_PID, 2));
    }

    #[test]
    fn first_handshake_confirms_spawned_launch_and_resolves_placeholder() {
        let _lock = lock_tests();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        let pid = child.id();
        register_launch(child, INITIAL_PROFILE_STEM.to_string());
        let now = Instant::now();

        let starting = tracked_games_snapshot_at(now);
        assert_eq!(starting.starting_profiles, vec![INITIAL_PROFILE_STEM]);
        let expired = tracked_games_snapshot_at(now + GAME_UI_STARTUP_GRACE);
        assert!(expired.starting_profiles.is_empty());
        assert!(expired.failed_profiles.is_empty());
        assert!(expired.unclear_profiles.is_empty());
        assert!(expired.expired_unconfirmed_game);
        assert!(!expired.disconnected_confirmed_game);

        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);

        assert_eq!(starting_profiles(), vec!["106_Nabor"]);
        assert_eq!(running_profiles(), vec!["106_Nabor"]);
        assert!(register_ui_ready(pid, 1));
        assert!(starting_profiles().is_empty());
        assert_eq!(tracked_games_snapshot().ready_profiles, vec!["106_Nabor"]);
        reset_flags();
    }

    #[test]
    fn handshake_before_spawn_registration_is_preserved() {
        let _lock = lock_tests();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        let pid = child.id();
        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready(pid, 1));
        register_launch(child, "106_Nabor".to_string());

        assert!(starting_profiles().is_empty());
        reset_flags();
    }

    #[test]
    fn ui_ready_requires_the_current_connection_owner() {
        let _lock = lock_tests();
        reset_flags();

        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 2);

        assert!(!register_ui_ready(STOPPED_TEST_PID, 1));
        assert_eq!(starting_profiles(), vec!["106_Nabor"]);
        assert!(register_ui_ready(STOPPED_TEST_PID, 2));
        assert!(starting_profiles().is_empty());
    }

    #[test]
    fn connected_game_without_ui_ready_becomes_a_failed_start() {
        let _lock = lock_tests();
        reset_flags();
        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(STOPPED_TEST_PID, 1, true));

        let failed = tracked_games_snapshot_at(Instant::now() + GAME_UI_STARTUP_GRACE);
        assert!(failed.starting_profiles.is_empty());
        assert_eq!(failed.failed_profiles, vec!["106_Nabor"]);
        assert!(!failed.disconnected_confirmed_game);
    }

    #[test]
    fn disconnected_game_without_ui_ready_remains_a_startup_failure() {
        let _lock = lock_tests();
        reset_flags();
        let pid = std::process::id();
        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(pid, 1, true));
        let now = Instant::now();

        assert!(!unregister_reconnected_launch(pid, 1));
        let starting = tracked_games_snapshot_at(now);
        assert_eq!(starting.starting_profiles, vec!["106_Nabor"]);
        assert!(!starting.disconnected_confirmed_game);

        let failed = tracked_games_snapshot_at(now + GAME_UI_STARTUP_GRACE);
        assert_eq!(failed.failed_profiles, vec!["106_Nabor"]);
        assert!(!failed.disconnected_confirmed_game);
        reset_flags();
    }

    #[cfg(unix)]
    #[test]
    fn terminating_failed_start_reaps_its_owned_process() {
        let _lock = lock_tests();
        reset_flags();
        let child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        register_launch(child, "106_Nabor".to_string());
        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(pid, 1, true));

        assert!(terminate_failed_starts_at(Instant::now() + GAME_UI_STARTUP_GRACE));
        assert!(running_profiles().is_empty());
        assert!(!crate::game::is_process_id_running(pid));
    }

    #[test]
    fn terminating_unconfirmed_start_reaps_its_owned_process() {
        let _lock = lock_tests();
        reset_flags();
        #[cfg(unix)]
        let child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"])
            .spawn()
            .unwrap();
        let pid = child.id();
        register_launch(child, "106_Nabor".to_string());

        assert!(terminate_unconfirmed_starts_at(Instant::now() + GAME_UI_STARTUP_GRACE));
        assert!(running_profiles().is_empty());
        assert!(!crate::game::is_process_id_running(pid));
    }

    #[test]
    fn unconfirmed_termination_leaves_confirmed_ui_failure_tracked() {
        let _lock = lock_tests();
        reset_flags();
        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(STOPPED_TEST_PID, 1, true));
        let after_startup_grace = Instant::now() + GAME_UI_STARTUP_GRACE;

        assert!(terminate_unconfirmed_starts_at(after_startup_grace));
        assert_eq!(
            tracked_games_snapshot_at(after_startup_grace).failed_profiles,
            vec!["106_Nabor"]
        );
        reset_flags();
    }

    #[cfg(unix)]
    #[test]
    fn terminating_reconstructed_failed_start_waits_for_process_exit() {
        let _lock = lock_tests();
        reset_flags();
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(pid, 1, true));

        assert!(terminate_failed_starts_at(Instant::now() + GAME_UI_STARTUP_GRACE));
        let _ = child.wait();
        assert!(running_profiles().is_empty());
        assert!(!crate::game::is_process_id_running(pid));
    }

    #[test]
    fn failed_termination_keeps_the_failed_start_tracked() {
        let _lock = lock_tests();
        reset_flags();
        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(STOPPED_TEST_PID, 1, true));
        let after_startup_grace = Instant::now() + GAME_UI_STARTUP_GRACE;

        assert!(!terminate_failed_starts_at(after_startup_grace));
        assert_eq!(
            tracked_games_snapshot_at(after_startup_grace).failed_profiles,
            vec!["106_Nabor"]
        );
        reset_flags();
    }

    #[test]
    fn termination_wait_does_not_hold_the_tracking_lock() {
        let _lock = lock_tests();
        reset_flags();
        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(STOPPED_TEST_PID, 1, true));
        let after_startup_grace = Instant::now() + GAME_UI_STARTUP_GRACE;
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (continue_tx, continue_rx) = std::sync::mpsc::channel();

        let termination = std::thread::spawn(move || {
            terminate_starts_with(after_startup_grace, TerminationTarget::MissingUiReady, |_| {
                started_tx.send(()).unwrap();
                continue_rx.recv().unwrap();
                false
            })
        });
        started_rx.recv().unwrap();

        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::channel();
        let snapshot = std::thread::spawn(move || {
            snapshot_tx.send(tracked_games_snapshot_at(after_startup_grace)).unwrap();
        });
        let snapshot_remained_available = snapshot_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        continue_tx.send(()).unwrap();

        assert!(!termination.join().unwrap());
        snapshot.join().unwrap();
        assert!(snapshot_remained_available);
        assert_eq!(
            tracked_games_snapshot_at(after_startup_grace).failed_profiles,
            vec!["106_Nabor"]
        );
        reset_flags();
    }

    #[test]
    fn unsupported_ui_readiness_reports_an_unclear_status() {
        let _lock = lock_tests();
        reset_flags();
        register_reconnected_launch(STOPPED_TEST_PID, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(STOPPED_TEST_PID, 1, false));

        let unclear = tracked_games_snapshot();
        assert!(unclear.starting_profiles.is_empty());
        assert!(unclear.failed_profiles.is_empty());
        assert_eq!(unclear.unclear_profiles, vec!["106_Nabor"]);
        assert!(!unclear.expired_unconfirmed_game);
        reset_flags();
    }

    #[test]
    fn disconnected_mod_with_unsupported_ui_readiness_reports_a_missing_connection() {
        let _lock = lock_tests();
        reset_flags();
        let pid = std::process::id();
        register_reconnected_launch(pid, "106_Nabor".to_string(), 1);
        assert!(register_ui_ready_capability(pid, 1, false));
        assert!(!unregister_reconnected_launch(pid, 1));

        let disconnected = tracked_games_snapshot_at(Instant::now() + GAME_UI_STARTUP_GRACE);
        assert!(disconnected.unclear_profiles.is_empty());
        assert!(disconnected.disconnected_confirmed_game);
        reset_flags();
    }

    #[test]
    fn reap_removes_exited_process() {
        let _lock = lock_tests();
        reset_flags();

        // Spawn a process that exits immediately
        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_Nabor".to_string());

        assert_eq!(starting_profiles(), vec!["106_Nabor"]);

        // Give it a moment to exit
        std::thread::sleep(Duration::from_millis(50));

        reap_children();
        assert!(running_profiles().is_empty());
        reset_flags();
    }

    #[test]
    fn update_stem_renames_matching_profile() {
        let _lock = lock_tests();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_OldName".to_string());

        update_stem("106_OldName", "106_NewName");

        let running = running_profiles();
        assert_eq!(running, vec!["106_NewName"]);
        reset_flags();
    }

    #[test]
    fn update_stem_noop_when_not_found() {
        let _lock = lock_tests();
        reset_flags();

        #[cfg(unix)]
        let child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let child = std::process::Command::new("cmd").args(["/C", "exit", "0"]).spawn().unwrap();
        register_launch(child, "106_Nabor".to_string());

        update_stem("411_Unknown", "411_NewName");

        let running = running_profiles();
        assert_eq!(running, vec!["106_Nabor"]);
        reset_flags();
    }
}
