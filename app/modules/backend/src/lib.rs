use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

mod commands;
mod daystrom_update;
mod game;
mod game_state;
mod logging;
#[cfg(target_os = "macos")]
mod macos_hooks;
mod monitor;
mod notifications;
mod process_origin;
mod profile_state;
mod settings;
mod state_update;
mod websocket;

use commands::{get_cached_game_status, launch_game, launch_updater, prepare_mod, remove_mod};
use daystrom_update::{
    check_for_daystrom_update, dismiss_daystrom_update, get_cached_daystrom_update_status, install_daystrom_update,
};
use profile_state::get_cached_profile_state;
use settings::{get_game_settings, set_game_settings};

use_log!("Startup");

/// Set when set_position() is called; cleared by the first Moved event which then shows the window.
static SHOW_AFTER_REPOSITION: AtomicBool = AtomicBool::new(false);

/// Set after the backend has asked the frontend to flush its logging appenders.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set after the frontend has completed its asynchronous shutdown work.
static SHUTDOWN_READY: AtomicBool = AtomicBool::new(false);

/// Set while the selected shutdown action exits, installs, or recovers from an error.
static SHUTDOWN_FINISHING: AtomicBool = AtomicBool::new(false);

/// Return whether the frontend has completed its coordinated shutdown work.
#[cfg(target_os = "macos")]
pub(crate) fn shutdown_ready() -> bool {
    SHUTDOWN_READY.load(Relaxed)
}

/// Ask the frontend to flush logging before exiting the application.
///
/// The main window is hidden immediately while its webview remains alive for the asynchronous flush.
/// A short timeout keeps the native application terminable when the webview is unavailable or the
/// frontend listener fails to respond.
pub(crate) fn request_shutdown(app: &tauri::AppHandle) {
    if SHUTDOWN_READY.load(Relaxed) || SHUTDOWN_FINISHING.load(Relaxed) {
        return;
    }
    if SHUTDOWN_REQUESTED.swap(true, Relaxed) {
        return;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    log_debug!("[EVENT] Requesting coordinated frontend shutdown");
    if let Err(error) = app.emit("shutdown-requested", ()) {
        log_warn!("Failed to request frontend shutdown: {error}");
        finish_shutdown(app);
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if SHUTDOWN_REQUESTED.load(Relaxed) && !SHUTDOWN_FINISHING.load(Relaxed) {
            log_warn!("Frontend shutdown timed out; continuing application shutdown");
            finish_shutdown(&app);
        }
    });
}

/// Finish the active shutdown action after frontend logging has flushed or timed out.
fn finish_shutdown(app: &tauri::AppHandle) {
    if SHUTDOWN_FINISHING.swap(true, Relaxed) {
        return;
    }
    match daystrom_update::install_pending_update(app) {
        daystrom_update::PendingInstallResult::Installed => {
            SHUTDOWN_READY.store(true, Relaxed);
            log_info!("Restarting Daystrom after successful update installation");
            app.restart();
        }
        daystrom_update::PendingInstallResult::Failed => {
            SHUTDOWN_REQUESTED.store(false, Relaxed);
            SHUTDOWN_FINISHING.store(false, Relaxed);
            show_main_window(app);
        }
        daystrom_update::PendingInstallResult::None => {
            SHUTDOWN_READY.store(true, Relaxed);
            app.exit(0);
        }
    }
}

/// Return whether an exit request represents a confirmed shutdown or updater restart.
fn should_allow_exit(code: Option<i32>) -> bool {
    code == Some(0) || code == Some(tauri::RESTART_EXIT_CODE)
}

/// Complete a coordinated shutdown after frontend appenders have flushed.
#[tauri::command]
fn complete_shutdown(app: tauri::AppHandle) {
    if !SHUTDOWN_REQUESTED.load(Relaxed) {
        log_debug!("[EVENT] Ignoring stale frontend shutdown completion");
        return;
    }
    log_debug!("[EVENT] Frontend shutdown completed");
    finish_shutdown(&app);
}

/// Make the window visible and open DevTools in debug builds.
fn show_window(window: &tauri::WebviewWindow) {
    let _ = window.show();
    #[cfg(debug_assertions)]
    if std::env::var("DAYSTROM_DEVTOOLS").as_deref() != Ok("0") {
        window.open_devtools();
    }
}

/// Show, restore, and focus the main Daystrom window.
pub(crate) fn show_main_window(app: &tauri::AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    true
}

/// Select the appropriate warning message based on which Daystrom-started processes are running.
///
/// Returns `None` when neither game nor launcher is running (quit should not be blocked).
fn quit_blocked_message(launcher_running: bool, game_running: bool) -> Option<&'static str> {
    match (launcher_running, game_running) {
        (true, true) => Some(
            "The launcher and the game are still running.\n\
             Daystrom has been minimised to the tray instead.",
        ),
        (true, false) => Some(
            "The launcher is still running.\n\
             Daystrom has been minimised to the tray instead.",
        ),
        (false, true) => Some(
            "The game is still running.\n\
             Daystrom has been minimised to the tray instead.",
        ),
        (false, false) => None,
    }
}

/// Show a warning dialogue and ensure the window stays in the tray afterwards.
///
/// Called from all quit paths (Close button, Tray menu, Cmd+Q, Dock) when
/// [`process_origin::should_block_quit`] returns `true`. Skips the dialogue when the window is
/// already hidden (the user already knows the app is in the tray).
pub(crate) fn warn_quit_blocked(window: &tauri::WebviewWindow) {
    if !window.is_visible().unwrap_or(false) {
        log_debug!("Quit blocked silently (window already hidden)");
        return;
    }
    if hint_level(settings::minimize_hint_count()) == HintLevel::Silent {
        log_debug!("Quit blocked silently (hint level: silent)");
        let _ = window.hide();
        return;
    }
    let status = game_state::get();
    let Some(message) = quit_blocked_message(status.launcher_started_by_us, status.game_started_by_us) else {
        return;
    };
    window
        .dialog()
        .message(message)
        .title("Still Running")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
    let _ = window.hide();
    settings::increment_minimize_hint();
}

/// How intrusively the minimize-to-tray action should notify the user.
///
/// Derived from the number of times the user has already seen a hint.
#[derive(Clone, Copy, Debug, PartialEq)]
enum HintLevel {
    /// First time: show a blocking native dialogue before hiding.
    Dialog,
    /// 2nd to 5th time: show a system notification after hiding.
    Notification,
    /// After that: hide silently.
    Silent,
}

/// Determine the hint level based on how many times the user has already been notified.
///
/// Pure function, no I/O.
fn hint_level(minimize_hint_count: u32) -> HintLevel {
    match minimize_hint_count {
        0 => HintLevel::Dialog,
        1..=4 => HintLevel::Notification,
        _ => HintLevel::Silent,
    }
}

/// Hide a window to the system tray with progressively less intrusive hints.
///
/// Uses [`hint_level`] to decide whether to show a dialogue, notification, or nothing. The hint
/// counter is incremented after each call, so subsequent minimizes become less intrusive.
pub(crate) fn minimize_to_tray(window: &tauri::WebviewWindow) {
    let count = settings::minimize_hint_count();

    match hint_level(count) {
        HintLevel::Dialog => {
            let w = window.clone();
            window
                .dialog()
                .message(
                    "Project Daystrom will continue running in the background.\n\
                          Click the tray icon to reopen the window.",
                )
                .title("Minimised to Tray")
                .kind(MessageDialogKind::Info)
                .show(move |_| {
                    log_debug!("[EVENT] Hiding window to tray (after dialog)");
                    let _ = w.hide();
                });
        }
        HintLevel::Notification => {
            log_debug!("[EVENT] Hiding window to tray (count={count})");
            notifications::show_minimize_hint(window);
        }
        HintLevel::Silent => {
            log_debug!("[EVENT] Hiding window to tray (count={count})");
            let _ = window.hide();
        }
    }

    settings::increment_minimize_hint();
}

/// Bootstrap and run the Tauri application.
///
/// Sets up logging, builds the system tray, starts the background monitor, and opens DevTools in
/// debug builds.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging::build_plugin())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let version = &app.package_info().version;
            log_info!("Project Daystrom {version} initialised");

            settings::load();

            // Restore the saved window position and size (stored as logical pixels).
            // The window starts invisible (tauri.conf.json) to prevent a flash on the primary screen.
            // On macOS, set_position dispatches async on the main queue, so we defer show() until the Moved event fires
            // (handled in on_window_event below).
            // On Windows, set_position() is synchronous and the event loop is not yet running during setup(), so we
            // show directly.
            if let Some(window) = app.get_webview_window("main") {
                let mut needs_reposition = false;
                if let Some(ws) = settings::get_window_settings() {
                    if let (Some(x), Some(y)) = (ws.x, ws.y) {
                        let _ = window.set_position(tauri::LogicalPosition::new(x as f64, y as f64));
                        needs_reposition = true;
                    }
                    if let (Some(w), Some(h)) = (ws.width, ws.height) {
                        let _ = window.set_size(tauri::LogicalSize::new(w as f64, h as f64));
                    }
                    if ws.maximized.unwrap_or(false) {
                        let _ = window.maximize();
                    }
                }

                if needs_reposition && cfg!(target_os = "macos") {
                    SHOW_AFTER_REPOSITION.store(true, Relaxed);
                } else {
                    show_window(&window);
                }
            }

            #[cfg(target_os = "macos")]
            {
                macos_hooks::set_app_handle(app.handle().clone());
                macos_hooks::install_quit_guard();
                if let Some(window) = app.get_webview_window("main") {
                    macos_hooks::install_minimize_guard(&window);
                }
            }

            monitor::start(app.handle().clone());
            daystrom_update::start(app.handle().clone());
            websocket::start(app.handle().clone());

            // ---- System Tray --------------------------------------------------------

            let show_item = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            // Sync quit item with game-status changes from the store.
            let quit_ref = quit_item.clone();
            app.listen("game-status", move |_event| {
                let enabled = !game_state::get().should_block_quit;
                let _ = quit_ref.set_enabled(enabled);
            });

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Project Daystrom")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        log_debug!("[EVENT] Tray menu: Show Window clicked");
                        show_main_window(app);
                    }
                    "quit" => {
                        log_debug!("[EVENT] Tray menu: Quit clicked");
                        if game_state::get().should_block_quit {
                            if let Some(window) = app.get_webview_window("main") {
                                warn_quit_blocked(&window);
                            }
                        } else {
                            request_shutdown(app);
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        log_debug!("[EVENT] Tray icon left-clicked");
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_cached_game_status,
            get_cached_daystrom_update_status,
            get_game_settings,
            set_game_settings,
            get_cached_profile_state,
            launch_updater,
            prepare_mod,
            remove_mod,
            launch_game,
            check_for_daystrom_update,
            dismiss_daystrom_update,
            install_daystrom_update,
            complete_shutdown,
        ])
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    log_debug!("[EVENT] CloseRequested on window '{}'", window.label());
                    if game_state::get().should_block_quit {
                        api.prevent_close();
                        if let Some(wv) = window.app_handle().get_webview_window(window.label()) {
                            warn_quit_blocked(&wv);
                        }
                    } else {
                        api.prevent_close();
                        request_shutdown(window.app_handle());
                    }
                }
                tauri::WindowEvent::Moved(..) | tauri::WindowEvent::Resized(..) => {
                    // Show the window after set_position has been applied (one-shot).
                    // Only react to Moved, not Resized, to avoid showing before the position is applied.
                    if matches!(event, tauri::WindowEvent::Moved(..))
                        && SHOW_AFTER_REPOSITION.swap(false, Relaxed)
                        && let Some(wv) = window.app_handle().get_webview_window(window.label())
                    {
                        show_window(&wv);
                    }
                    // Windows fallback: detect minimizing via Resized (macOS uses native hook).
                    #[cfg(not(target_os = "macos"))]
                    if window.is_minimized().unwrap_or(false) {
                        if let Some(wv) = window.app_handle().get_webview_window(window.label()) {
                            minimize_to_tray(&wv);
                        }
                        return;
                    }
                    // Persist window geometry as logical pixels (debounced via settings::save).
                    if !window.is_minimized().unwrap_or(false)
                        && let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size())
                    {
                        let scale = window.scale_factor().unwrap_or(1.0);
                        settings::save_window_state(
                            (pos.x as f64 / scale).round() as i32,
                            (pos.y as f64 / scale).round() as i32,
                            (size.width as f64 / scale).round() as u32,
                            (size.height as f64 / scale).round() as u32,
                            window.is_maximized().unwrap_or(false),
                        );
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    log_debug!("[EVENT] Window '{}' destroyed", window.label());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                if should_allow_exit(code) {
                    log_debug!("[EVENT] ExitRequested (code: {code:?}), shutting down");
                    return;
                }
                log_debug!("[EVENT] ExitRequested (code: {code:?}), staying in tray");
                api.prevent_exit();
            }
            tauri::RunEvent::Exit => {
                log_debug!("[EVENT] Exit (app is shutting down)");
                settings::flush_saves();
            }
            _ => {}
        });
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_shutdown_and_updater_restart_may_exit() {
        assert!(should_allow_exit(Some(0)));
        assert!(should_allow_exit(Some(tauri::RESTART_EXIT_CODE)));
        assert!(!should_allow_exit(None));
        assert!(!should_allow_exit(Some(1)));
    }

    // -- quit_blocked_message --

    #[test]
    fn quit_blocked_both_running() {
        let msg = quit_blocked_message(true, true);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("launcher and the game"));
    }

    #[test]
    fn quit_blocked_launcher_only() {
        let msg = quit_blocked_message(true, false);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("launcher is still running"));
    }

    #[test]
    fn quit_blocked_game_only() {
        let msg = quit_blocked_message(false, true);
        assert!(msg.is_some());
        assert!(msg.unwrap().contains("game is still running"));
    }

    #[test]
    fn quit_not_blocked_neither_running() {
        assert!(quit_blocked_message(false, false).is_none());
    }

    // -- hint_level --

    #[test]
    fn hint_level_first_time_is_dialog() {
        assert_eq!(hint_level(0), HintLevel::Dialog);
    }

    #[test]
    fn hint_level_second_to_fifth_is_notification() {
        for count in 1..=4 {
            assert_eq!(hint_level(count), HintLevel::Notification);
        }
    }

    #[test]
    fn hint_level_after_fifth_is_silent() {
        assert_eq!(hint_level(5), HintLevel::Silent);
        assert_eq!(hint_level(100), HintLevel::Silent);
        assert_eq!(hint_level(u32::MAX), HintLevel::Silent);
    }
}
