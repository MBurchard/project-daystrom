//! Intercept **all** macOS quit paths (Cmd+Q, App menu, Dock "Quit", SIGTERM).
//!
//! Tauri 2 / tao does not fire `RunEvent::ExitRequested` for `[NSApplication terminate:]`, so the existing
//! quit-blocking logic is bypassed.
//! We add `applicationShouldTerminate:` to tao's `TaoAppDelegateParent` class via `class_addMethod`.
//! The method does not exist on that class (the default comes from the `NSResponder` superclass), so this is safe and
//! does not require swizzling.

use std::ffi::c_char;
use std::sync::OnceLock;

use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplicationTerminateReply;

use crate::game;

crate::use_log!("QuitGuard");

/// Global handle so the ObjC callback can reach Tauri.
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Store the app handle for later use by the quit guard callback.
pub(crate) fn set_app_handle(handle: tauri::AppHandle) {
    APP_HANDLE.set(handle).expect("APP_HANDLE already set");
}

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
/// Returns `NSTerminateCancel` when the game or launcher is running (and shows the existing warning dialog),
/// `NSTerminateNow` otherwise.
unsafe extern "C-unwind" fn should_terminate(
    _this: *const AnyObject,
    _cmd: Sel,
    _sender: *const AnyObject,
) -> usize {
    if game::is_launcher_running() || game::is_game_running() {
        log_info!("Quit blocked (game or launcher running)");
        if let Some(handle) = APP_HANDLE.get() {
            use tauri::Manager;
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
                crate::warn_quit_blocked(&window);
            }
        }
        NSApplicationTerminateReply::TerminateCancel.0
    } else {
        log_debug!("Quit permitted");
        NSApplicationTerminateReply::TerminateNow.0
    }
}
