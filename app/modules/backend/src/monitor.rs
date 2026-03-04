use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tauri::Emitter;

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
/// Spawns a thread that polls game and launcher process status every 2 seconds and pushes state changes to the frontend
/// via Tauri events. Runs for the entire lifetime of the application.
/// Safe to call multiple times; subsequent calls are no-ops.
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

/// Main monitoring loop.
///
/// Checks process status every [`POLL_INTERVAL`] seconds. Emits `process-status` events on state changes, `game-status`
/// events after a process exits (full refresh), and `update-check` events for periodic API rechecks while the launcher
/// is open. Runs indefinitely.
fn run_loop(app: tauri::AppHandle) {
    let mut prev_game = false;
    let mut prev_launcher = false;
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
            match commands::check_for_update() {
                Ok(check) => { let _ = app.emit("update-check", check); }
                Err(_) => { let _ = app.emit("update-check-failed", ()); }
            }
            last_api_check = Instant::now();
        }

        prev_game = game;
        prev_launcher = launcher;
    }
}
