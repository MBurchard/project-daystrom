//! macOS-specific hooks for quit interception and minimize-to-tray behaviour.
//!
//! **Quit guard:** Intercepts all macOS quit paths (Cmd+Q, App menu, Dock "Quit", SIGTERM) by adding
//! `applicationShouldTerminate:` to tao's `TaoAppDelegateParent` class. Tauri 2 / tao does not fire
//! `RunEvent::ExitRequested` for `[NSApplication terminate:]`, so the existing quit-blocking logic would
//! be bypassed without this hook.
//!
//! **Minimize guard:** Intercepts `windowShouldMiniaturize:` on the window's delegate class to prevent
//! the native Genie animation and instead hide the window to the system tray.

use std::ffi::c_char;
use std::sync::OnceLock;

use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplicationTerminateReply;

use crate::process_origin;

crate::use_log!("MacHooks");

/// Global handle so ObjC callbacks can reach Tauri.
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Store the app handle for later use by the ObjC callbacks.
///
/// Must be called before [`install_quit_guard`] and [`install_minimize_guard`].
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

        let success = ffi::class_addMethod(
            (cls as *const AnyClass).cast_mut(),
            sel,
            imp,
            types,
        );

        if success.as_bool() {
            log_info!("Quit guard installed (applicationShouldTerminate: added)");
        } else {
            log_error!("Failed to add applicationShouldTerminate: to TaoAppDelegateParent");
        }
    }
}

/// ObjC callback: decides whether the application is allowed to terminate.
///
/// Returns `NSTerminateCancel` when a Daystrom-started process is still running (and shows a warning),
/// `NSTerminateNow` otherwise.
unsafe extern "C-unwind" fn should_terminate(
    _this: *const AnyObject,
    _cmd: Sel,
    _sender: *const AnyObject,
) -> usize {
    if process_origin::should_block_quit() {
        log_info!("Quit blocked (Daystrom-started process still running)");
        if let Some(handle) = APP_HANDLE.get() {
            use tauri::Manager;
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                crate::warn_quit_blocked(&window);
                let _ = window.hide();
            }
        }
        NSApplicationTerminateReply::TerminateCancel.0
    } else {
        log_debug!("Quit permitted");
        NSApplicationTerminateReply::TerminateNow.0
    }
}

// ---- Minimize Guard -------------------------------------------------------------

/// Install `windowShouldMiniaturize:` on the window's delegate class.
///
/// Prevents the native Genie minimize animation. Instead, the callback hides the window to the
/// system tray via [`crate::minimize_to_tray`].
pub(crate) fn install_minimize_guard(window: &tauri::WebviewWindow) {
    unsafe {
        use objc2::msg_send;

        let ns_window_raw = match window.ns_window() {
            Ok(ptr) => ptr,
            Err(e) => {
                log_error!("Failed to get NSWindow: {e}; minimize guard NOT installed");
                return;
            }
        };

        // Get the delegate object from the NSWindow
        let ns_window: *const AnyObject = ns_window_raw.cast();
        let delegate: *const AnyObject = msg_send![ns_window, delegate];

        if delegate.is_null() {
            log_error!("NSWindow delegate is null; minimize guard NOT installed");
            return;
        }

        let cls = (*delegate).class();
        let sel = sel!(windowShouldMiniaturize:);

        // If the method already exists on this exact class, skip.
        if cls.instance_method(sel).is_some() {
            log_warn!("windowShouldMiniaturize: already exists on delegate; skipping");
            return;
        }

        // windowShouldMiniaturize: signature: (self, _cmd, NSWindow*) -> BOOL.
        // Type encoding: B = BOOL (C99 _Bool on 64-bit), @ = object, : = selector.
        let types: *const c_char = c"B@:@".as_ptr();

        let imp: Imp = std::mem::transmute(
            window_should_miniaturize
                as unsafe extern "C-unwind" fn(*const AnyObject, Sel, *const AnyObject) -> Bool,
        );

        let success = ffi::class_addMethod(
            (cls as *const AnyClass).cast_mut(),
            sel,
            imp,
            types,
        );

        if success.as_bool() {
            log_info!("Minimize guard installed (windowShouldMiniaturize: added)");
        } else {
            log_error!("Failed to add windowShouldMiniaturize: to window delegate");
        }
    }
}

/// ObjC callback: intercepts the minimize action to hide to tray instead.
///
/// Always returns `NO` to prevent the native Genie animation, then triggers the tray-hide logic.
unsafe extern "C-unwind" fn window_should_miniaturize(
    _this: *const AnyObject,
    _cmd: Sel,
    _window: *const AnyObject,
) -> Bool {
    log_debug!("Minimize intercepted, hiding to tray");
    if let Some(handle) = APP_HANDLE.get() {
        use tauri::Manager;
        if let Some(window) = handle.get_webview_window("main") {
            crate::minimize_to_tray(&window);
        }
    }
    Bool::NO
}
