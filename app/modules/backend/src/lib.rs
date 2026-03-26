use tauri::{Listener, Manager};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

mod commands;
mod game;
mod game_state;
mod logging;
#[cfg(target_os = "macos")]
mod macos_hooks;
mod monitor;
mod process_origin;
mod profile_state;
mod settings;
mod websocket;

use commands::{get_cached_game_status, launch_game, launch_updater, prepare_mod, remove_mod};
use profile_state::get_cached_profile_state;
use settings::{get_game_settings, set_game_settings};

use_log!("Startup");

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
    let Some(message) = quit_blocked_message(
        status.launcher_started_by_us,
        status.game_started_by_us,
    ) else {
        return;
    };
    window.dialog()
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
            window.dialog()
                .message("Project Daystrom will continue running in the background.\n\
                          Click the tray icon to reopen the window.")
                .title("Minimised to Tray")
                .kind(MessageDialogKind::Info)
                .show(move |_| {
                    log_debug!("[EVENT] Hiding window to tray (after dialog)");
                    let _ = w.hide();
                });
        }
        HintLevel::Notification => {
            log_debug!("[EVENT] Hiding window to tray (count={count})");
            let _ = window.hide();
            use tauri_plugin_notification::NotificationExt;
            let _ = window.app_handle().notification()
                .builder()
                .title("Minimised to Tray")
                .body("Project Daystrom is still running. Click the tray icon to reopen.")
                .show();
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
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let version = &app.package_info().version;
            log_info!("Project Daystrom {version} initialised");

            settings::load();

            #[cfg(target_os = "macos")]
            {
                macos_hooks::set_app_handle(app.handle().clone());
                macos_hooks::install_quit_guard();
                if let Some(window) = app.get_webview_window("main") {
                    macos_hooks::install_minimize_guard(&window);
                }
            }

            monitor::start(app.handle().clone());
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
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        log_debug!("[EVENT] Tray menu: Quit clicked");
                        if game_state::get().should_block_quit {
                            if let Some(window) = app.get_webview_window("main") {
                                warn_quit_blocked(&window);
                            }
                        } else {
                            app.exit(0);
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
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // ---- DevTools (debug only) ----------------------------------------------

            #[cfg(debug_assertions)]
            if std::env::var("DAYSTROM_DEVTOOLS").as_deref() != Ok("0") {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
                log_debug!("DevTools opened");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_cached_game_status,
            get_game_settings,
            set_game_settings,
            get_cached_profile_state,
            launch_updater,
            prepare_mod,
            remove_mod,
            launch_game,
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
                        window.app_handle().exit(0);
                    }
                }
                // Windows fallback: no native hook available, detect minimizing via Resized event.
                // On macOS the ObjC minimize guard handles this before the event fires.
                #[cfg(not(target_os = "macos"))]
                tauri::WindowEvent::Resized { .. } => {
                    if window.is_minimized().unwrap_or(false) {
                        if let Some(wv) = window.app_handle().get_webview_window(window.label()) {
                            minimize_to_tray(&wv);
                        }
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
        .run(|_app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, code, .. } => {
                    if code == Some(0) {
                        log_debug!("[EVENT] ExitRequested (code: {code:?}), shutting down");
                        return;
                    }
                    log_debug!("[EVENT] ExitRequested (code: {code:?}), staying in tray");
                    api.prevent_exit();
                }
                tauri::RunEvent::Exit => {
                    log_debug!("[EVENT] Exit (app is shutting down)");
                    websocket::cleanup();
                }
                _ => {}
            }
        });
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
