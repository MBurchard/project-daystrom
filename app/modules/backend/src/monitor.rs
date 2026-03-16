use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands;
use crate::game;
use crate::use_log;

use_log!("Monitor");

/// Interval between process checks.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Interval for re-checking the Scopely update API while the launcher is open.
const API_RECHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Flag indicating whether a monitor thread is currently active.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Start the permanent background process monitor.
///
/// Spawns a thread that polls game and launcher process status every 2 seconds and pushes state
/// changes to the frontend via the game state store. Runs for the entire lifetime of the
/// application. Safe to call multiple times; further calls are no-ops.
pub fn start(app: tauri::AppHandle) {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        log_debug!("Monitor already active");
        return;
    }

    log_debug!("Starting process monitor");
    thread::spawn(move || {
        run_loop(app);
    });
}

// ---- Monitor State --------------------------------------------------------------

/// Actions the monitor loop can produce on each tick.
///
/// Returned by [`MonitorState::tick`] as a pure description of what should happen, without
/// performing any side effects. The caller (`run_loop`) is responsible for executing them.
///
/// Process status updates are NOT included here: the caller always writes the current process
/// state into the game state store, which emits to the frontend only when something changed.
#[derive(Clone, Debug, PartialEq)]
enum MonitorAction {
    /// Clear the "game started by us" origin flag.
    ClearGameStarted,
    /// Clear the "launcher started by us" origin flag.
    ClearLauncherStarted,
    /// Run a full game detection and update the store.
    RefreshGameStatus,
    /// Re-query the Scopely update API and push the result.
    RecheckUpdateApi,
}

/// Encapsulates the monitor's mutable state between ticks.
///
/// All decision logic lives in [`tick`](MonitorState::tick), making it testable without a Tauri
/// runtime. The caller only needs to feed in the current process status.
struct MonitorState {
    prev_game: bool,
    prev_launcher: bool,
    last_api_check: Instant,
}

impl MonitorState {
    /// Create a new monitor state with both processes initially absent.
    fn new() -> Self {
        Self {
            prev_game: false,
            prev_launcher: false,
            last_api_check: Instant::now(),
        }
    }

    /// Evaluate one monitoring cycle and return the actions to execute.
    ///
    /// Compares the current process status against the previous tick, determines which special
    /// actions are needed, and updates internal state. Pure logic: no I/O, no side effects
    /// beyond `self`.
    fn tick(&mut self, game: bool, launcher: bool) -> Vec<MonitorAction> {
        let api_recheck_due = self.last_api_check.elapsed() >= API_RECHECK_INTERVAL;
        let actions = evaluate(self.prev_game, self.prev_launcher, game, launcher, api_recheck_due);

        if actions.iter().any(|a| matches!(a, MonitorAction::RecheckUpdateApi)) {
            self.last_api_check = Instant::now();
        }

        self.prev_game = game;
        self.prev_launcher = launcher;
        actions
    }
}

/// Determine which special actions to take based on current and previous state.
///
/// Pure function: no side effects, no I/O. All branching logic of the monitor loop lives here, so
/// it can be tested without a Tauri runtime.
fn evaluate(
    prev_game: bool,
    prev_launcher: bool,
    game: bool,
    launcher: bool,
    api_recheck_due: bool,
) -> Vec<MonitorAction> {
    let mut actions = Vec::new();

    // Game just exited: clear the origin flag and refresh full status
    if prev_game && !game {
        actions.push(MonitorAction::ClearGameStarted);
        actions.push(MonitorAction::RefreshGameStatus);
    }

    // Launcher just exited: clear the origin flag and refresh full status
    if prev_launcher && !launcher {
        actions.push(MonitorAction::ClearLauncherStarted);
        actions.push(MonitorAction::RefreshGameStatus);
    }

    // Periodic API recheck while the launcher is open
    if launcher && api_recheck_due {
        actions.push(MonitorAction::RecheckUpdateApi);
    }

    actions
}

// ---- Main Loop ------------------------------------------------------------------

/// Main monitoring loop.
///
/// Runs initial full game detection, then polls process status every [`POLL_INTERVAL`]
/// seconds. Delegates all decision-making to [`MonitorState::tick`] and only executes the
/// returned actions. Process status is written to the game state store on every tick; the store
/// handles change detection and event emission. Runs indefinitely.
fn run_loop(app: tauri::AppHandle) {
    // Initial full detection populates the store
    let status = commands::get_game_status(app.clone());
    let installed = status.installed;
    crate::game_state::update(&app, |s| *s = status);
    if installed {
        commands::update_check_into_store(&app);
    }

    let mut state = MonitorState::new();

    loop {
        let game = game::is_game_running();
        let launcher = game::is_launcher_running();

        // Process tick actions first: origin flags must be cleared before the store emits,
        // so that listeners (e.g. tray quit item) see the correct should_block_quit() state.
        for action in state.tick(game, launcher) {
            match action {
                MonitorAction::ClearGameStarted => {
                    crate::process_origin::clear_game_started();
                    crate::game_state::update(&app, |s| {
                        s.game_started_by_us = false;
                    });
                    log_debug!("Game process ended");
                }
                MonitorAction::ClearLauncherStarted => {
                    crate::process_origin::clear_launcher_started();
                    crate::game_state::update(&app, |s| {
                        s.launcher_started_by_us = false;
                    });
                    log_debug!("Launcher process ended");
                }
                MonitorAction::RefreshGameStatus => {
                    let fresh = commands::get_game_status(app.clone());
                    crate::game_state::update(&app, |s| {
                        // Preserve fields not covered by get_game_status
                        let remote_version = s.remote_version;
                        let update_check_failed = s.update_check_failed;
                        let game_started_by_us = s.game_started_by_us;
                        let launcher_started_by_us = s.launcher_started_by_us;
                        *s = fresh;
                        s.remote_version = remote_version;
                        s.update_check_failed = update_check_failed;
                        s.game_started_by_us = game_started_by_us;
                        s.launcher_started_by_us = launcher_started_by_us;
                    });
                }
                MonitorAction::RecheckUpdateApi => {
                    log_debug!("Periodic update check");
                    commands::update_check_into_store(&app);
                }
            }
        }

        // Update process status in the store (emits to frontend only on change)
        crate::game_state::update(&app, |s| {
            s.game_running = game;
            s.launcher_running = launcher;
        });

        thread::sleep(POLL_INTERVAL);
    }
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- evaluate (pure logic) --

    #[test]
    fn no_change_produces_no_actions() {
        assert!(evaluate(false, false, false, false, false).is_empty());
        assert!(evaluate(true, true, true, true, false).is_empty());
    }

    #[test]
    fn game_starts_no_special_actions() {
        assert!(evaluate(false, false, true, false, false).is_empty());
    }

    #[test]
    fn game_exits_clears_flag_and_refreshes() {
        assert_eq!(evaluate(true, false, false, false, false), vec![
            MonitorAction::ClearGameStarted,
            MonitorAction::RefreshGameStatus,
        ]);
    }

    #[test]
    fn launcher_starts_no_special_actions() {
        assert!(evaluate(false, false, false, true, false).is_empty());
    }

    #[test]
    fn launcher_exits_clears_flag_and_refreshes() {
        assert_eq!(evaluate(false, true, false, false, false), vec![
            MonitorAction::ClearLauncherStarted,
            MonitorAction::RefreshGameStatus,
        ]);
    }

    #[test]
    fn both_exit_simultaneously() {
        assert_eq!(evaluate(true, true, false, false, false), vec![
            MonitorAction::ClearGameStarted,
            MonitorAction::RefreshGameStatus,
            MonitorAction::ClearLauncherStarted,
            MonitorAction::RefreshGameStatus,
        ]);
    }

    #[test]
    fn launcher_running_and_api_recheck_due() {
        assert_eq!(evaluate(false, true, false, true, true), vec![
            MonitorAction::RecheckUpdateApi,
        ]);
    }

    #[test]
    fn launcher_running_but_recheck_not_due() {
        assert!(evaluate(false, true, false, true, false).is_empty());
    }

    #[test]
    fn launcher_not_running_recheck_ignored() {
        assert!(evaluate(false, false, false, false, true).is_empty());
        assert!(evaluate(false, false, true, false, true).is_empty());
    }

    #[test]
    fn launcher_just_started_with_recheck_due() {
        assert_eq!(evaluate(false, false, false, true, true), vec![
            MonitorAction::RecheckUpdateApi,
        ]);
    }

    // -- MonitorState (stateful tick sequences) --

    #[test]
    fn new_state_starts_with_both_absent() {
        let state = MonitorState::new();
        assert!(!state.prev_game);
        assert!(!state.prev_launcher);
    }

    #[test]
    fn tick_updates_previous_state() {
        let mut state = MonitorState::new();

        state.tick(true, false);
        assert!(state.prev_game);
        assert!(!state.prev_launcher);

        state.tick(true, true);
        assert!(state.prev_game);
        assert!(state.prev_launcher);
    }

    #[test]
    fn multi_tick_game_lifecycle() {
        let mut state = MonitorState::new();

        // Tick 1: game starts (no special actions, store handles process update)
        let actions = state.tick(true, false);
        assert!(actions.is_empty());

        // Tick 2: game still running, no change
        let actions = state.tick(true, false);
        assert!(actions.is_empty());

        // Tick 3: game exits
        let actions = state.tick(false, false);
        assert_eq!(actions, vec![
            MonitorAction::ClearGameStarted,
            MonitorAction::RefreshGameStatus,
        ]);

        // Tick 4: still off, no change
        let actions = state.tick(false, false);
        assert!(actions.is_empty());
    }

    #[test]
    fn multi_tick_launcher_lifecycle() {
        let mut state = MonitorState::new();

        let actions = state.tick(false, true);
        assert!(actions.is_empty());

        let actions = state.tick(false, true);
        assert!(actions.is_empty());

        let actions = state.tick(false, false);
        assert_eq!(actions, vec![
            MonitorAction::ClearLauncherStarted,
            MonitorAction::RefreshGameStatus,
        ]);
    }

    #[test]
    fn multi_tick_overlapping_processes() {
        let mut state = MonitorState::new();

        // Launcher starts first
        let actions = state.tick(false, true);
        assert!(actions.is_empty());

        // Game joins while launcher still running
        let actions = state.tick(true, true);
        assert!(actions.is_empty());

        // Launcher exits, game still running
        let actions = state.tick(true, false);
        assert_eq!(actions, vec![
            MonitorAction::ClearLauncherStarted,
            MonitorAction::RefreshGameStatus,
        ]);

        // Game exits
        let actions = state.tick(false, false);
        assert_eq!(actions, vec![
            MonitorAction::ClearGameStarted,
            MonitorAction::RefreshGameStatus,
        ]);
    }

    #[test]
    fn api_recheck_timer_resets_after_recheck() {
        let mut state = MonitorState::new();

        // Force the timer to be "expired" (checked_sub avoids underflow on Windows)
        let expired = API_RECHECK_INTERVAL + Duration::from_secs(1);
        state.last_api_check = Instant::now().checked_sub(expired).unwrap_or(Instant::now());

        // Launcher running + timer expired: triggers recheck
        let actions = state.tick(false, true);
        assert!(actions.contains(&MonitorAction::RecheckUpdateApi));

        // tick() reset the timer, so the next tick should NOT recheck
        let actions = state.tick(false, true);
        assert!(!actions.contains(&MonitorAction::RecheckUpdateApi));
    }

    #[test]
    fn api_recheck_not_triggered_without_launcher() {
        let mut state = MonitorState::new();
        let expired = API_RECHECK_INTERVAL + Duration::from_secs(1);
        state.last_api_check = Instant::now().checked_sub(expired).unwrap_or(Instant::now());

        // Timer expired but no launcher: no recheck
        let actions = state.tick(true, false);
        assert!(!actions.contains(&MonitorAction::RecheckUpdateApi));
    }
}
