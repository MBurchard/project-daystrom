//! Skip the first interstitial popup after game start.
//!
//! Hooks `InterstitialViewController.AboutToShow()` to close the first popup immediately via `CloseWhenReady()`.
//! Subsequent popups are passed through to the original handler.
//! When the setting is off, all popups behave normally.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `InterstitialViewController.AboutToShow()`.
static ORIGINAL_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Direct function pointer for `InterstitialViewController.CloseWhenReady()`.
static CLOSE_WHEN_READY_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the first interstitial has already been seen this session.
static FIRST_SEEN: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Hook -----------------------------------------------------------------

/// Hook for `InterstitialViewController.AboutToShow()`.
///
/// On the first call, if `skip_first_popup` is enabled, calls `CloseWhenReady()` instead of the original to dismiss the
/// ad popup immediately. All subsequent calls pass through normally.
extern "C" fn hook_about_to_show(this: *mut Il2CppObject) {
    if crate::settings::skip_first_popup() && !FIRST_SEEN.swap(true, Relaxed) {
        let close_ptr = CLOSE_WHEN_READY_FN.load(Relaxed);
        if !close_ptr.is_null() {
            debug!(target: "Interstitial", "Closing first popup");
            let close: LifecycleFn = unsafe { std::mem::transmute(close_ptr) };
            unsafe { close(this) };
            return;
        }
        // Defensive: if CloseWhenReady wasn't resolved, fall through to original.
    }

    let orig_ptr = ORIGINAL_FN.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

// ---- Installation ---------------------------------------------------------

/// Install the interstitial popup hook.
///
/// Hooks `InterstitialViewController.AboutToShow` and resolves `CloseWhenReady` for direct
/// invocation.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.Interstitial", "InterstitialViewController",
    ) else {
        log::warn!(target: "Interstitial", "InterstitialViewController not found");
        return;
    };

    // Resolve CloseWhenReady (0 params) for direct call.
    let Some(close_ptr) = tracker::resolve_fn(api, class, "CloseWhenReady", 0) else {
        log::warn!(target: "Interstitial", "CloseWhenReady not found");
        return;
    };
    CLOSE_WHEN_READY_FN.store(close_ptr as *mut (), Relaxed);

    // Hook AboutToShow (0 params).
    let Some(show_ptr) = tracker::resolve_fn(api, class, "AboutToShow", 0) else {
        log::warn!(target: "Interstitial", "AboutToShow not found");
        return;
    };

    match crate::hook::engine::install_hook(
        "Interstitial", show_ptr, hook_about_to_show as *const (),
    ) {
        Ok(orig) => {
            ORIGINAL_FN.store(orig as *mut (), Relaxed);
            debug!(target: "Interstitial", "AboutToShow hook installed");
        }
        Err(e) => log::warn!(target: "Interstitial", "Failed to hook AboutToShow: {e}"),
    }
}
