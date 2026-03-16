//! macOS-specific hooks for quit interception and minimize-to-tray behaviour.
//!
//! **Quit guard:** Intercepts all macOS quit paths (Cmd+Q, App menu, Dock "Quit", SIGTERM) by adding
//! `applicationShouldTerminate:` to tao's `TaoAppDelegateParent` class. Tauri 2 / tao does not fire
//! `RunEvent::ExitRequested` for `[NSApplication terminate:]`, so the existing quit-blocking logic would
//! be bypassed without this hook.
//!
//! **Minimize guard:** Overrides `miniaturize:` on tao's NSWindow subclass to prevent the native
//! Genie animation and instead hide the window to the system tray. This is more reliable than
//! hooking `windowShouldMiniaturize:` on the delegate, which `performMiniaturize:` may not consult
//! in all configurations.

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
            log_debug!("Quit guard installed (applicationShouldTerminate: added)");
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
    if crate::game_state::get().should_block_quit {
        log_debug!("Quit blocked (Daystrom-started process still running)");
        if let Some(handle) = APP_HANDLE.get() {
            use tauri::Manager;
            if let Some(window) = handle.get_webview_window("main") {
                crate::warn_quit_blocked(&window);
            }
        }
        NSApplicationTerminateReply::TerminateCancel.0
    } else {
        log_debug!("Quit permitted");
        NSApplicationTerminateReply::TerminateNow.0
    }
}

// ---- Minimize Guard -------------------------------------------------------------

/// Override `miniaturize:` on the NSWindow's runtime class to intercept all minimize paths.
///
/// Uses `class_addMethod` to shadow the inherited `NSWindow::miniaturize:` on the concrete
/// subclass (typically tao's `TaoWindow`). This catches traffic-light button clicks
/// (`performMiniaturize:` → `miniaturize:`), programmatic calls, and double-click-titlebar
/// minimize. The override hides the window to the system tray instead of performing the
/// native Genie animation.
pub(crate) fn install_minimize_guard(window: &tauri::WebviewWindow) {
    unsafe {
        let ns_window_raw = match window.ns_window() {
            Ok(ptr) => ptr,
            Err(e) => {
                log_error!("Failed to get NSWindow: {e}; minimize guard NOT installed");
                return;
            }
        };

        let ns_window: *const AnyObject = ns_window_raw.cast();
        let cls = (*ns_window).class();
        let sel = sel!(miniaturize:);

        // miniaturize: signature: (self, _cmd, sender) -> void.
        // Type encoding: v = void, @ = object, : = selector.
        let types: *const c_char = c"v@:@".as_ptr();

        let imp: Imp = std::mem::transmute(
            intercept_miniaturize
                as unsafe extern "C-unwind" fn(*const AnyObject, Sel, *const AnyObject),
        );

        let success = ffi::class_addMethod(
            (cls as *const AnyClass).cast_mut(),
            sel,
            imp,
            types,
        );

        if success.as_bool() {
            log_debug!("Minimize guard installed (miniaturize: overridden on {:?})", cls.name());
        } else {
            log_error!(
                "Failed to override miniaturize: on {:?} (method may already exist on this class)",
                cls.name(),
            );
        }
    }
}

/// ObjC callback: replaces `miniaturize:` to hide to tray instead of performing the Genie
/// animation.
unsafe extern "C-unwind" fn intercept_miniaturize(
    _this: *const AnyObject,
    _cmd: Sel,
    _sender: *const AnyObject,
) {
    log_debug!("Minimize intercepted, hiding to tray");
    if let Some(handle) = APP_HANDLE.get() {
        use tauri::Manager;
        if let Some(window) = handle.get_webview_window("main") {
            crate::minimize_to_tray(&window);
        }
    }
}
