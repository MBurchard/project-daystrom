//! Native desktop notifications for application hints and interactive update alerts.

use std::thread;

#[cfg(target_os = "macos")]
use std::sync::Mutex;

#[cfg(not(target_os = "macos"))]
use notify_rust::Timeout;
use notify_rust::{Notification, NotificationResponse};
use tauri::Manager;

use crate::use_log;

use_log!("Notifications");

/// Cached macOS notification authorization for the current application process.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum NotificationPermission {
    /// Permission has not been decided or the previous check failed temporarily.
    Unknown,
    /// The user granted permission.
    Authorized,
    /// The user denied permission.
    Denied,
}

/// Current macOS notification authorization state.
#[cfg(target_os = "macos")]
static NOTIFICATION_PERMISSION: Mutex<NotificationPermission> = Mutex::new(NotificationPermission::Unknown);

/// Return whether a native notification response represents a body click.
fn should_focus_window(response: &NotificationResponse) -> bool {
    matches!(response, NotificationResponse::Default)
}

/// Show and focus the main Daystrom window without affecting the running game.
fn focus_main_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        log_warn!("Main window unavailable after update notification click");
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Configure the application identity required for installed Windows notifications.
#[cfg(target_os = "windows")]
fn configure_windows_identity(notification: &mut Notification, app: &tauri::AppHandle) {
    if let Ok(executable) = tauri::utils::platform::current_exe()
        && let Some(directory) = executable.parent()
    {
        use std::path::MAIN_SEPARATOR;

        let directory = directory.display().to_string();
        let debug_suffix = format!("{MAIN_SEPARATOR}target{MAIN_SEPARATOR}debug");
        let release_suffix = format!("{MAIN_SEPARATOR}target{MAIN_SEPARATOR}release");
        if !directory.ends_with(&debug_suffix) && !directory.ends_with(&release_suffix) {
            notification.app_id(&app.config().identifier);
        }
    }
}

/// Configure notification expiry where the active platform backend supports it.
fn configure_timeout(_notification: &mut Notification) {
    #[cfg(not(target_os = "macos"))]
    _notification.timeout(Timeout::Milliseconds(15_000));
}

/// Ensure the current platform permits Daystrom to display native notifications.
#[cfg(target_os = "macos")]
fn notification_permission_granted() -> bool {
    let mut permission = NOTIFICATION_PERMISSION.lock().unwrap();
    match *permission {
        NotificationPermission::Authorized => return true,
        NotificationPermission::Denied => return false,
        NotificationPermission::Unknown => {}
    }

    match notify_rust::request_auth_blocking() {
        Ok(true) => {
            *permission = NotificationPermission::Authorized;
            true
        }
        Ok(false) => {
            *permission = NotificationPermission::Denied;
            log_warn!("macOS notification permission was denied");
            false
        }
        Err(error) => {
            log_warn!("Failed to request macOS notification permission: {error}");
            false
        }
    }
}

/// Native desktop notifications require no explicit permission request on this platform.
#[cfg(not(target_os = "macos"))]
fn notification_permission_granted() -> bool {
    true
}

/// Show the non-interactive tray hint, then hide the Daystrom window.
///
/// Keeping the window visible until the worker has requested macOS notification permission avoids
/// presenting the first system authorization dialog for an application that has already vanished.
pub fn show_minimize_hint(window: &tauri::WebviewWindow) {
    let worker_window = window.clone();
    let fallback_window = window.clone();
    let spawn_result = thread::Builder::new()
        .name("daystrom-minimize-notification".into())
        .spawn(move || {
            if notification_permission_granted() {
                let mut notification = Notification::new();
                notification
                    .summary("Minimised to Tray")
                    .body("Project Daystrom is still running. Click the tray icon to reopen.");
                configure_timeout(&mut notification);
                #[cfg(target_os = "windows")]
                configure_windows_identity(&mut notification, worker_window.app_handle());

                if let Err(error) = notification.show() {
                    log_warn!("Failed to show minimize notification: {error}");
                }
            }

            let _ = worker_window.hide();
        });

    if let Err(error) = spawn_result {
        log_warn!("Failed to start minimize notification worker: {error}");
        let _ = fallback_window.hide();
    }
}

/// Notify the player about a newly discovered STFC version.
///
/// Clicking the notification only brings Daystrom to the foreground. It never stops the game or
/// starts the Scopely launcher automatically.
pub fn show_game_update(app: &tauri::AppHandle, version: u32) {
    let app = app.clone();
    let spawn_result = thread::Builder::new()
        .name("daystrom-update-notification".into())
        .spawn(move || {
            if !notification_permission_granted() {
                return;
            }

            let mut notification = Notification::new();
            notification.summary("STFC update available").body(&format!(
                "Version {version} is available. Close the game and open Daystrom to start the update."
            ));
            configure_timeout(&mut notification);
            #[cfg(target_os = "windows")]
            configure_windows_identity(&mut notification, &app);

            let handle = match notification.show() {
                Ok(handle) => handle,
                Err(error) => {
                    log_warn!("Failed to show update notification: {error}");
                    return;
                }
            };

            if let Err(error) = handle.wait_for_response(move |response: &NotificationResponse| {
                if should_focus_window(response) {
                    focus_main_window(&app);
                }
            }) {
                log_warn!("Failed to receive update notification response: {error}");
            }
        });

    if let Err(error) = spawn_result {
        log_warn!("Failed to start update notification worker: {error}");
    }
}

/// Notify the user about a Daystrom release discovered by a periodic background check.
///
/// Clicking the notification only brings Daystrom to the foreground. It never downloads or
/// installs the release without a separate user action.
pub fn show_daystrom_update(app: &tauri::AppHandle, version: &str) {
    let app = app.clone();
    let version = version.to_string();
    let spawn_result = thread::Builder::new()
        .name("daystrom-app-update-notification".into())
        .spawn(move || {
            if !notification_permission_granted() {
                return;
            }

            let mut notification = Notification::new();
            notification
                .summary("Project Daystrom update available")
                .body(&format!("Version {version} is ready. Open Daystrom to review the update."));
            configure_timeout(&mut notification);
            #[cfg(target_os = "windows")]
            configure_windows_identity(&mut notification, &app);

            let handle = match notification.show() {
                Ok(handle) => handle,
                Err(error) => {
                    log_warn!("Failed to show Daystrom update notification: {error}");
                    return;
                }
            };

            if let Err(error) = handle.wait_for_response(move |response: &NotificationResponse| {
                if should_focus_window(response) {
                    focus_main_window(&app);
                }
            }) {
                log_warn!("Failed to receive Daystrom update notification response: {error}");
            }
        });

    if let Err(error) = spawn_result {
        log_warn!("Failed to start Daystrom update notification worker: {error}");
    }
}

#[cfg(test)]
mod tests {
    use notify_rust::CloseReason;

    use super::*;

    #[test]
    fn body_click_focuses_window() {
        assert!(should_focus_window(&NotificationResponse::Default));
    }

    #[test]
    fn dismissed_notification_does_not_focus_window() {
        assert!(!should_focus_window(&NotificationResponse::Closed(CloseReason::Dismissed)));
    }

    #[test]
    fn explicit_notification_action_does_not_focus_window() {
        assert!(!should_focus_window(&NotificationResponse::Action("open-update".to_string())));
    }
}
