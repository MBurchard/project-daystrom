//! Native desktop notifications that need interaction callbacks.

use std::thread;

#[cfg(not(target_os = "macos"))]
use notify_rust::Timeout;
#[cfg(target_os = "macos")]
use notify_rust::error::{ApplicationError, MacOsError};
use notify_rust::{Notification, NotificationResponse};
use tauri::Manager;

use crate::use_log;

use_log!("Notifications");

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

/// Configure platform identity fields required for native desktop notifications.
fn configure_platform_identity(_notification: &mut Notification, _app: &tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    if let Ok(executable) = tauri::utils::platform::current_exe()
        && let Some(directory) = executable.parent()
    {
        use std::path::MAIN_SEPARATOR;

        let directory = directory.display().to_string();
        let debug_suffix = format!("{MAIN_SEPARATOR}target{MAIN_SEPARATOR}debug");
        let release_suffix = format!("{MAIN_SEPARATOR}target{MAIN_SEPARATOR}release");
        if !directory.ends_with(&debug_suffix) && !directory.ends_with(&release_suffix) {
            _notification.app_id(&_app.config().identifier);
        }
    }

    #[cfg(target_os = "macos")]
    match notify_rust::set_application(if tauri::is_dev() { "com.apple.Terminal" } else { &_app.config().identifier }) {
        Ok(()) | Err(MacOsError::Application(ApplicationError::AlreadySet(_))) => {}
        Err(error) => log_warn!("Failed to configure macOS notification identity: {error}"),
    }
}

/// Configure notification expiry where the active platform backend supports it.
fn configure_timeout(_notification: &mut Notification) {
    #[cfg(not(target_os = "macos"))]
    _notification.timeout(Timeout::Milliseconds(15_000));
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
            let mut notification = Notification::new();
            notification.summary("STFC update available").body(&format!(
                "Version {version} is available. Close the game and open Daystrom to start the update."
            ));
            configure_timeout(&mut notification);
            configure_platform_identity(&mut notification, &app);

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
