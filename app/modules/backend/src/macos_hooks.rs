//! macOS-specific hook for quit interception.
//!
//! **Quit guard:** Intercepts all macOS quit paths (Cmd+Q, App menu, Dock "Quit", SIGTERM) by adding
//! `applicationShouldTerminate:` to tao's `TaoAppDelegateParent` class. Tauri 2 / tao does not fire
//! `RunEvent::ExitRequested` for `[NSApplication terminate:]`, so the existing quit-blocking and
//! coordinated-shutdown logic would be bypassed without this hook.

use std::ffi::c_char;
use std::sync::OnceLock;

use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplicationTerminateReply;

crate::use_log!("MacHooks");

/// Global handle so ObjC callbacks can reach Tauri.
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Store the app handle for later use by the ObjC callbacks.
///
/// Must be called before [`install_quit_guard`].
pub(crate) fn set_app_handle(handle: tauri::AppHandle) {
    APP_HANDLE.set(handle).expect("APP_HANDLE already set");
}

// ---- Quit Guard -----------------------------------------------------------------

/// Install `applicationShouldTerminate:` on tao's delegate class.
///
/// Must be called once during app setup, after `set_app_handle`.
pub(crate) fn install_quit_guard() {
    unsafe {
        let cls = AnyClass::get(c"TaoAppDelegateParent");
        let Some(cls) = cls else {
            log_error!("Class TaoAppDelegateParent not found; quit guard NOT installed");
            return;
        };

        let sel = sel!(applicationShouldTerminate:);

        // If the method already exists on this exact class (not inherited), skip.
        if cls.instance_method(sel).is_some() {
            log_warn!("applicationShouldTerminate: already exists on TaoAppDelegateParent; skipping");
            return;
        }

        // applicationShouldTerminate: signature: (self, _cmd, NSApplication*) -> NSUInteger.
        // Type encoding: Q = unsigned long (NSUInteger, 64-bit), @ = object, : = selector.
        let types: *const c_char = c"Q@:@".as_ptr();

        let imp: Imp = std::mem::transmute(
            should_terminate as unsafe extern "C-unwind" fn(*const AnyObject, Sel, *const AnyObject) -> usize,
        );

        let success = ffi::class_addMethod((cls as *const AnyClass).cast_mut(), sel, imp, types);

        if success.as_bool() {
            log_debug!("Quit guard installed (applicationShouldTerminate: added)");
        } else {
            log_error!("Failed to add applicationShouldTerminate: to TaoAppDelegateParent");
        }
    }
}

/// ObjC callback: decides whether the application is allowed to terminate.
///
/// Returns `NSTerminateCancel` while a Daystrom-started process is still running or while the frontend
/// flushes its logging appenders. Once shutdown is ready, the Tauri exit request terminates the app.
unsafe extern "C-unwind" fn should_terminate(_this: *const AnyObject, _cmd: Sel, _sender: *const AnyObject) -> usize {
    if crate::shutdown_ready() {
        log_debug!("Coordinated shutdown completed; terminating");
        return NSApplicationTerminateReply::TerminateNow.0;
    }

    if crate::game_state::get().should_block_quit {
        log_debug!("Quit blocked (Daystrom-started process still running)");
        if let Some(handle) = APP_HANDLE.get() {
            use tauri::Manager;
            if let Some(window) = handle.get_webview_window("main") {
                crate::warn_quit_blocked(&window);
            }
        }
        return NSApplicationTerminateReply::TerminateCancel.0;
    }

    log_debug!("Quit permitted; requesting coordinated shutdown");
    let Some(handle) = APP_HANDLE.get() else {
        log_warn!("App handle unavailable during coordinated shutdown; terminating immediately");
        return NSApplicationTerminateReply::TerminateNow.0;
    };
    crate::request_shutdown(handle);
    NSApplicationTerminateReply::TerminateCancel.0
}
