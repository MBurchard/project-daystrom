use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

mod commands;
mod game;
mod logging;
#[cfg(target_os = "macos")]
mod macos_quit;
mod monitor;

use commands::{get_game_status, launch_game, launch_updater, prepare_mod, remove_mod};

use_log!("Startup");

/// Show a warning dialog explaining that the app cannot quit while the game or launcher is running.
pub(crate) fn warn_quit_blocked(window: &tauri::WebviewWindow) {
    let launcher = game::is_launcher_running();
    let game = game::is_game_running();
    let message = match (launcher, game) {
        (true, true) => "Cannot quit while the Scopely Launcher and the game are running.\n\
                         Close both first, then quit.",
        (true, false) => "Cannot quit while the Scopely Launcher is running.\n\
                          Close the launcher first, then quit.",
        (_, true) => "Cannot quit while the game is running.\n\
                      Close the game first, then quit.",
        _ => return,
    };
    window.dialog()
        .message(message)
        .title("Quit Blocked")
        .kind(MessageDialogKind::Warning)
        .show(|_| {});
}

/// Bootstrap and run the Tauri application.
///
/// Sets up logging, builds the system tray, and opens DevTools in debug builds.
/// Game detection runs lazily on the first `get_game_status` command from the frontend.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging::build_plugin())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let version = &app.package_info().version;
            log_info!("Project Daystrom {version} initialised");

            #[cfg(target_os = "macos")]
            {
                macos_quit::set_app_handle(app.handle().clone());
                macos_quit::install_quit_guard();
            }

            // ---- System Tray --------------------------------------------------------

            let show_item = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

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
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        log_debug!("[EVENT] Tray menu: Quit clicked");
                        if game::is_launcher_running() || game::is_game_running() {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
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
            get_game_status,
            launch_updater,
            prepare_mod,
            remove_mod,
            launch_game,
        ])
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    log_debug!("[EVENT] CloseRequested on window '{}'", window.label());
                    api.prevent_close();
                    let window = window.clone();
                    window.dialog()
                        .message("Project Daystrom will continue running in the background.\nClick the tray icon to reopen the window.")
                        .title("Minimised to Tray")
                        .kind(MessageDialogKind::Info)
                        .show(move |_| {
                            log_debug!("[EVENT] Hiding window to tray");
                            let _ = window.hide();
                        });
                }
                tauri::WindowEvent::Destroyed => {
                    log_debug!("[EVENT] Window '{}' destroyed", window.label());
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::ExitRequested { api, code, .. } => {
                    if code == Some(0) {
                        log_debug!("[EVENT] ExitRequested (code: {code:?}), shutting down");
                        return;
                    }
                    log_debug!("[EVENT] ExitRequested (code: {code:?})");
                    if game::is_launcher_running() || game::is_game_running() {
                        api.prevent_exit();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                            warn_quit_blocked(&window);
                        }
                    } else {
                        // Keep running in tray when no game/launcher active
                        api.prevent_exit();
                    }
                }
                tauri::RunEvent::Exit => {
                    log_debug!("[EVENT] Exit (app is shutting down)");
                }
                _ => {}
            }
        });
}
