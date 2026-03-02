use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tauri::Emitter;

use crate::commands;
use crate::game;
use crate::use_log;

use_log!("Monitor");

/// Interval between process checks.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Interval for re-checking the Scopely update API while the launcher is open.
const API_RECHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Flag indicating whether a monitor thread is currently active.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Start background process monitoring.
///
/// Spawns a thread that polls game and launcher process status every 5 seconds
/// and pushes state changes to the frontend via Tauri events. Stops automatically
/// when no watched processes are running.
///
/// `initial_game` / `initial_launcher` reflect the expected state right after launch
/// so the first poll does not emit a spurious change event.
pub fn start(app: tauri::AppHandle, initial_game: bool, initial_launcher: bool) {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        log_debug!("Monitor already active");
        return;
    }

    log_debug!("Starting process monitor");
    thread::spawn(move || {
        run_loop(app, initial_game, initial_launcher);
        ACTIVE.store(false, Ordering::Release);
        log_debug!("Process monitor stopped");
    });
}

/// Main monitoring loop.
///
/// Checks process status every [`POLL_INTERVAL`] seconds. Emits `process-status` events
/// on state changes, `game-status` events after a process exits (full refresh), and
/// `update-check` events for periodic API rechecks while the launcher is open.
fn run_loop(app: tauri::AppHandle, initial_game: bool, initial_launcher: bool) {
    let mut prev_game = initial_game;
    let mut prev_launcher = initial_launcher;
    let mut last_api_check = Instant::now();

    loop {
        thread::sleep(POLL_INTERVAL);

        let game = game::is_game_running();
        let launcher = game::is_launcher_running();

        // Emit process-status only when something changed
        if game != prev_game || launcher != prev_launcher {
            let _ = app.emit("process-status", commands::ProcessStatus {
                game_running: game,
                launcher_running: launcher,
            });
        }

        // Game just exited: push full status refresh
        if prev_game && !game {
            log_debug!("Game process ended, refreshing status");
            let status = commands::get_game_status(app.clone());
            let _ = app.emit("game-status", status);
        }

        // Launcher just exited: push full status refresh
        if prev_launcher && !launcher {
            log_debug!("Launcher process ended, refreshing status");
            let status = commands::get_game_status(app.clone());
            let _ = app.emit("game-status", status);
        }

        // Periodic API recheck while the launcher is open
        if launcher && last_api_check.elapsed() >= API_RECHECK_INTERVAL {
            log_debug!("Periodic update check");
            if let Ok(check) = commands::check_for_update() {
                let _ = app.emit("update-check", check);
            }
            last_api_check = Instant::now();
        }

        prev_game = game;
        prev_launcher = launcher;

        // Nothing running: monitoring no longer needed
        if !game && !launcher {
            break;
        }
    }
}
