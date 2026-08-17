use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::commands;
use crate::game;
use crate::use_log;

use_log!("Monitor");

/// Interval between process checks.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Interval between profile directory scans after the fast phase.
const PROFILE_SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// Faster profile scan interval during the first minutes after launch.
const PROFILE_SCAN_FAST: Duration = Duration::from_secs(5);

/// Duration of the fast scanning phase after app start.
const PROFILE_SCAN_FAST_PHASE: Duration = Duration::from_secs(180);

/// Interval for re-checking the Scopely update API while Daystrom is running.
const API_RECHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Maximum time to wait for a running mod to restore its Daystrom launch identity.
const GAME_ORIGIN_RECONNECT_GRACE: Duration = Duration::from_secs(10);

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
    /// Finish activation of a bundled mod restored while the game was running.
    FinishModRestore,
    /// Re-query the Scopely update API and push the result.
    RecheckUpdateApi,
}

/// Current relationship between a running game process and Daystrom.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GameOrigin {
    /// No game process is running.
    None,
    /// A running game may still restore its Daystrom identity through the WebSocket.
    Pending,
    /// The running game has confirmed that Daystrom launched it.
    Daystrom,
    /// The running game did not provide a Daystrom launch identity in time.
    External,
}

/// Encapsulates the monitor's mutable state between ticks.
///
/// All decision logic lives in [`tick`](MonitorState::tick), making it testable without a Tauri
/// runtime. The caller only needs to feed in the current process status.
struct MonitorState {
    prev_game: bool,
    prev_launcher: bool,
    last_api_check: Instant,
    last_seen_remote_version: Option<u32>,
    game_origin_reconnect_deadline: Option<Instant>,
    daystrom_origin_confirmed: bool,
}

impl MonitorState {
    /// Create a new monitor state with both processes initially absent.
    ///
    /// The initially checked remote version forms the notification baseline, so an update already
    /// known during startup is not reported as newly discovered later.
    fn new(initial_remote_version: Option<u32>) -> Self {
        Self {
            prev_game: false,
            prev_launcher: false,
            last_api_check: Instant::now(),
            last_seen_remote_version: initial_remote_version,
            game_origin_reconnect_deadline: None,
            daystrom_origin_confirmed: false,
        }
    }

    /// Start origin recovery when Daystrom finds a game already running during startup.
    fn begin_game_origin_recovery(&mut self, game_running: bool, now: Instant) {
        if game_running {
            self.game_origin_reconnect_deadline = Some(now + GAME_ORIGIN_RECONNECT_GRACE);
        }
    }

    /// Classify the running game without reporting it as external during a reconnect window.
    fn classify_game_origin(&mut self, game_running: bool, started_by_daystrom: bool, now: Instant) -> GameOrigin {
        if !game_running {
            self.game_origin_reconnect_deadline = None;
            self.daystrom_origin_confirmed = false;
            return GameOrigin::None;
        }

        if started_by_daystrom {
            self.game_origin_reconnect_deadline = None;
            self.daystrom_origin_confirmed = true;
            return GameOrigin::Daystrom;
        }

        if self.daystrom_origin_confirmed && self.game_origin_reconnect_deadline.is_none() {
            self.game_origin_reconnect_deadline = Some(now + GAME_ORIGIN_RECONNECT_GRACE);
        }

        if self.game_origin_reconnect_deadline.is_some_and(|deadline| now < deadline) {
            return GameOrigin::Pending;
        }

        self.game_origin_reconnect_deadline = None;
        self.daystrom_origin_confirmed = false;
        GameOrigin::External
    }

    /// Evaluate one monitoring cycle and return the actions to execute.
    ///
    /// Compares the current process status against the previous tick, determines which special
    /// actions are needed, and updates internal state. Pure logic: no I/O, no side effects
    /// beyond `self`.
    fn tick(&mut self, game: bool, launcher: bool, installed: bool) -> Vec<MonitorAction> {
        let api_recheck_due = self.last_api_check.elapsed() >= API_RECHECK_INTERVAL;
        let actions = evaluate(self.prev_game, self.prev_launcher, game, launcher, installed, api_recheck_due);

        if actions.iter().any(|a| matches!(a, MonitorAction::RecheckUpdateApi)) {
            self.last_api_check = Instant::now();
        }

        self.prev_game = game;
        self.prev_launcher = launcher;
        actions
    }

    /// Record a successful update check and decide whether it warrants a player notification.
    ///
    /// A version is considered only once. This prevents repeated notifications for the same update
    /// and deliberately consumes discoveries made while the game is not running.
    fn should_notify_update(&mut self, remote_version: u32, update_available: bool, game_running: bool) -> bool {
        let newly_discovered = self.last_seen_remote_version != Some(remote_version);
        self.last_seen_remote_version = Some(remote_version);
        newly_discovered && update_available && game_running
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
    installed: bool,
    api_recheck_due: bool,
) -> Vec<MonitorAction> {
    let mut actions = Vec::new();

    // Game just exited: clear the origin flag and refresh full status
    if prev_game && !game {
        actions.push(MonitorAction::ClearGameStarted);
        actions.push(MonitorAction::RefreshGameStatus);
        actions.push(MonitorAction::FinishModRestore);
    }

    // Launcher just exited: clear the origin flag and refresh full status
    if prev_launcher && !launcher {
        actions.push(MonitorAction::ClearLauncherStarted);
        actions.push(MonitorAction::RefreshGameStatus);
    }

    // Periodic API recheck for every installed game, independent of the launcher process.
    if installed && api_recheck_due {
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
    let initial_game_running = status.game_running;
    crate::game_state::update(&app, |s| *s = status);

    // Profiles are local startup data and must not wait for the remote update check.
    let profiles = crate::profile_state::scan_profiles();
    crate::profile_state::update(&app, |s| {
        s.profiles = profiles;
        s.game_origin_pending = initial_game_running && !crate::process_origin::is_game_started();
    });

    if installed {
        commands::update_check_into_store(&app);
    }

    let mut state = MonitorState::new(crate::game_state::get().remote_version);
    state.begin_game_origin_recovery(initial_game_running, Instant::now());
    let mut last_profile_scan = Instant::now();
    let start_time = Instant::now();

    loop {
        // Periodic profile directory scan (fast in the first 3 minutes, then every 60s)
        let scan_interval = if start_time.elapsed() < PROFILE_SCAN_FAST_PHASE {
            PROFILE_SCAN_FAST
        } else {
            PROFILE_SCAN_INTERVAL
        };
        if last_profile_scan.elapsed() >= scan_interval {
            let profiles = crate::profile_state::scan_profiles();
            crate::profile_state::update(&app, |s| s.profiles = profiles);
            last_profile_scan = Instant::now();
        }
        // Reap zombie child processes before checking process status.
        // Without this, pgrep would find zombies and report them as running.
        crate::process_origin::reap_children();

        let game = game::is_game_running();
        let launcher = game::is_launcher_running();
        let installed = crate::game_state::get().installed;

        // Process tick actions first: origin flags must be cleared before the store emits,
        // so that listeners (e.g. tray quit item) see the correct should_block_quit() state.
        for action in state.tick(game, launcher, installed) {
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
                        // Preserve fields aren't covered by get_game_status
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
                MonitorAction::FinishModRestore => {
                    crate::daystrom_update::resume_pending_mod_restore(&app);
                }
                MonitorAction::RecheckUpdateApi => {
                    log_debug!("Periodic update check");
                    commands::update_check_into_store(&app);
                    let status = crate::game_state::get();
                    if let Some(remote_version) = status.remote_version
                        && state.should_notify_update(remote_version, status.update_available, game)
                    {
                        log_info!("New STFC update detected while the game is running: {remote_version}");
                        crate::notifications::show_game_update(&app, remote_version);
                    }
                }
            }
        }

        // Update process status in the store (emits to frontend only on change)
        crate::game_state::update(&app, |s| {
            s.game_running = game;
            s.launcher_running = launcher;
        });

        // Track which of our launched profiles are still running.
        // Reconcile stale stems after in-game renames: if a running stem no longer matches any profile file, find the
        // profile with the same server ID and update the mapping so the frontend keeps the correct running state.
        let running = crate::process_origin::running_profiles();
        let profiles = crate::profile_state::get().profiles;
        for stem in &running {
            if !profiles.iter().any(|p| p.stem == *stem)
                && let Some(server_str) = stem.split('_').next()
                && let Ok(server_id) = server_str.parse::<i32>()
                && let Some(p) = profiles.iter().find(|p| p.server == server_id)
            {
                crate::process_origin::update_stem(stem, &p.stem);
            }
        }
        let running = crate::process_origin::running_profiles();
        let origin = state.classify_game_origin(game, crate::process_origin::is_game_started(), Instant::now());
        crate::profile_state::update(&app, |s| {
            s.running_profiles = running;
            s.external_game_running = origin == GameOrigin::External;
            s.game_origin_pending = origin == GameOrigin::Pending;
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
        assert!(evaluate(false, false, false, false, true, false).is_empty());
        assert!(evaluate(true, true, true, true, true, false).is_empty());
    }

    #[test]
    fn game_starts_no_special_actions() {
        assert!(evaluate(false, false, true, false, true, false).is_empty());
    }

    #[test]
    fn game_exits_clears_flag_and_refreshes() {
        assert_eq!(
            evaluate(true, false, false, false, true, false),
            vec![MonitorAction::ClearGameStarted, MonitorAction::RefreshGameStatus, MonitorAction::FinishModRestore,]
        );
    }

    #[test]
    fn launcher_starts_no_special_actions() {
        assert!(evaluate(false, false, false, true, true, false).is_empty());
    }

    #[test]
    fn launcher_exits_clears_flag_and_refreshes() {
        assert_eq!(
            evaluate(false, true, false, false, true, false),
            vec![MonitorAction::ClearLauncherStarted, MonitorAction::RefreshGameStatus,]
        );
    }

    #[test]
    fn both_exit_simultaneously() {
        assert_eq!(
            evaluate(true, true, false, false, true, false),
            vec![
                MonitorAction::ClearGameStarted,
                MonitorAction::RefreshGameStatus,
                MonitorAction::FinishModRestore,
                MonitorAction::ClearLauncherStarted,
                MonitorAction::RefreshGameStatus,
            ]
        );
    }

    #[test]
    fn launcher_running_and_api_recheck_due() {
        assert_eq!(
            evaluate(false, true, false, true, true, true),
            vec![MonitorAction::RecheckUpdateApi,]
        );
    }

    #[test]
    fn launcher_running_but_recheck_not_due() {
        assert!(evaluate(false, true, false, true, true, false).is_empty());
    }

    #[test]
    fn api_recheck_due_without_launcher() {
        assert_eq!(
            evaluate(false, false, false, false, true, true),
            vec![MonitorAction::RecheckUpdateApi,]
        );
        assert_eq!(
            evaluate(false, false, true, false, true, true),
            vec![MonitorAction::RecheckUpdateApi,]
        );
    }

    #[test]
    fn launcher_just_started_with_recheck_due() {
        assert_eq!(
            evaluate(false, false, false, true, true, true),
            vec![MonitorAction::RecheckUpdateApi,]
        );
    }

    #[test]
    fn api_recheck_not_triggered_without_installed_game() {
        assert!(evaluate(false, false, false, false, false, true).is_empty());
    }

    // -- MonitorState (stateful tick sequences) --

    #[test]
    fn new_state_starts_with_both_absent() {
        let state = MonitorState::new(None);
        assert!(!state.prev_game);
        assert!(!state.prev_launcher);
    }

    #[test]
    fn tick_updates_previous_state() {
        let mut state = MonitorState::new(None);

        state.tick(true, false, true);
        assert!(state.prev_game);
        assert!(!state.prev_launcher);

        state.tick(true, true, true);
        assert!(state.prev_game);
        assert!(state.prev_launcher);
    }

    #[test]
    fn multi_tick_game_lifecycle() {
        let mut state = MonitorState::new(None);

        // Tick 1: game starts (no special actions, store handles process update)
        let actions = state.tick(true, false, true);
        assert!(actions.is_empty());

        // Tick 2: game still running, no change
        let actions = state.tick(true, false, true);
        assert!(actions.is_empty());

        // Tick 3: game exits
        let actions = state.tick(false, false, true);
        assert_eq!(
            actions,
            vec![MonitorAction::ClearGameStarted, MonitorAction::RefreshGameStatus, MonitorAction::FinishModRestore,]
        );

        // Tick 4: still off, no change
        let actions = state.tick(false, false, true);
        assert!(actions.is_empty());
    }

    #[test]
    fn multi_tick_launcher_lifecycle() {
        let mut state = MonitorState::new(None);

        let actions = state.tick(false, true, true);
        assert!(actions.is_empty());

        let actions = state.tick(false, true, true);
        assert!(actions.is_empty());

        let actions = state.tick(false, false, true);
        assert_eq!(
            actions,
            vec![MonitorAction::ClearLauncherStarted, MonitorAction::RefreshGameStatus,]
        );
    }

    #[test]
    fn multi_tick_overlapping_processes() {
        let mut state = MonitorState::new(None);

        // Launcher starts first
        let actions = state.tick(false, true, true);
        assert!(actions.is_empty());

        // Game joins while launcher still running
        let actions = state.tick(true, true, true);
        assert!(actions.is_empty());

        // Launcher exits, game still running
        let actions = state.tick(true, false, true);
        assert_eq!(
            actions,
            vec![MonitorAction::ClearLauncherStarted, MonitorAction::RefreshGameStatus,]
        );

        // Game exits
        let actions = state.tick(false, false, true);
        assert_eq!(
            actions,
            vec![MonitorAction::ClearGameStarted, MonitorAction::RefreshGameStatus, MonitorAction::FinishModRestore,]
        );
    }

    #[test]
    fn api_recheck_timer_resets_after_recheck() {
        let mut state = MonitorState::new(None);

        // Force the timer to be "expired" (checked_sub avoids underflow on short-uptime Windows)
        let expired = API_RECHECK_INTERVAL + Duration::from_secs(1);
        let Some(past) = Instant::now().checked_sub(expired) else {
            // System uptime too short to represent the interval; skip this test
            return;
        };
        state.last_api_check = past;

        // Installed game + timer expired: triggers recheck.
        let actions = state.tick(false, true, true);
        assert!(actions.contains(&MonitorAction::RecheckUpdateApi));

        // tick() reset the timer, so the next tick should NOT recheck
        let actions = state.tick(false, true, true);
        assert!(!actions.contains(&MonitorAction::RecheckUpdateApi));
    }

    #[test]
    fn api_recheck_triggered_without_launcher() {
        let mut state = MonitorState::new(None);
        let expired = API_RECHECK_INTERVAL + Duration::from_secs(1);
        state.last_api_check = Instant::now().checked_sub(expired).unwrap_or(Instant::now());

        // Timer expired without launcher: recheck still runs.
        let actions = state.tick(true, false, true);
        assert!(actions.contains(&MonitorAction::RecheckUpdateApi));
    }

    #[test]
    fn startup_game_waits_for_origin_recovery_before_becoming_external() {
        let now = Instant::now();
        let mut state = MonitorState::new(None);
        state.begin_game_origin_recovery(true, now);

        assert_eq!(state.classify_game_origin(true, false, now), GameOrigin::Pending);
        assert_eq!(
            state.classify_game_origin(true, false, now + GAME_ORIGIN_RECONNECT_GRACE),
            GameOrigin::External
        );
    }

    #[test]
    fn reconnect_hello_resolves_pending_origin_immediately() {
        let now = Instant::now();
        let mut state = MonitorState::new(None);
        state.begin_game_origin_recovery(true, now);

        assert_eq!(state.classify_game_origin(true, true, now), GameOrigin::Daystrom);
    }

    #[test]
    fn confirmed_daystrom_game_gets_a_new_grace_period_after_disconnect() {
        let now = Instant::now();
        let mut state = MonitorState::new(None);

        assert_eq!(state.classify_game_origin(true, true, now), GameOrigin::Daystrom);
        assert_eq!(state.classify_game_origin(true, false, now), GameOrigin::Pending);
        assert_eq!(
            state.classify_game_origin(true, true, now + Duration::from_secs(1)),
            GameOrigin::Daystrom
        );
    }

    #[test]
    fn game_started_after_daystrom_without_identity_is_external_immediately() {
        let now = Instant::now();
        let mut state = MonitorState::new(None);

        assert_eq!(state.classify_game_origin(true, false, now), GameOrigin::External);
    }

    #[test]
    fn startup_update_is_notification_baseline() {
        let mut state = MonitorState::new(Some(200));

        assert!(!state.should_notify_update(200, true, true));
    }

    #[test]
    fn newly_discovered_update_notifies_once_while_game_runs() {
        let mut state = MonitorState::new(Some(200));

        assert!(state.should_notify_update(201, true, true));
        assert!(!state.should_notify_update(201, true, true));
    }

    #[test]
    fn update_discovered_without_running_game_is_consumed_silently() {
        let mut state = MonitorState::new(Some(200));

        assert!(!state.should_notify_update(201, true, false));
        assert!(!state.should_notify_update(201, true, true));
    }

    #[test]
    fn newly_seen_version_without_available_update_does_not_notify() {
        let mut state = MonitorState::new(Some(200));

        assert!(!state.should_notify_update(201, false, true));
    }
}
