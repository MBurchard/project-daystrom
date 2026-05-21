//! Skip the first interstitial popup after game start.
//!
//! Hooks `InterstitialViewController.AboutToShow()` to close the first popup immediately via `CloseWhenReady()`.
//! Subsequent popups are passed through to the original handler.
//! When the setting is off, all popups behave normally.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::debug;

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `InterstitialViewController.AboutToShow()`.
static ORIGINAL_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `InterstitialViewController.CloseWhenReady()`.
static CLOSE_WHEN_READY_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the first interstitial has already been seen this session.
static FIRST_SEEN: AtomicBool = AtomicBool::new(false);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("Interstitial");

// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Hook -----------------------------------------------------------------

/// Hook for `InterstitialViewController.AboutToShow()`.
///
/// On the first call, if `skip_first_popup` is enabled, calls `CloseWhenReady()` instead of the original to dismiss the
/// ad popup immediately. All subsequent calls pass through normally.
extern "C" fn hook_about_to_show(this: *mut Il2CppObject) {
    if HOOK_INFO.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| try_close_first_popup(this)));
        match result {
            Ok(true) => return,
            Ok(false) => {}
            Err(_) => HOOK_INFO.record_error(),
        }
    }

    let orig_ptr = ORIGINAL_FN.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

fn try_close_first_popup(this: *mut Il2CppObject) -> bool {
    if crate::settings::skip_first_popup() && !FIRST_SEEN.swap(true, Relaxed) {
        let close_ptr = CLOSE_WHEN_READY_FN.load(Relaxed);
        if !close_ptr.is_null() {
            debug!(target: "Interstitial", "Closing first popup");
            invoke::void(close_ptr, this, "InterstitialViewController.CloseWhenReady");
            return true;
        }
        // Defensive: if CloseWhenReady wasn't resolved, fall through to original.
    }
    false
}

// ---- Installation ---------------------------------------------------------

/// Install the interstitial popup hook.
///
/// Hooks `InterstitialViewController.AboutToShow` and resolves `CloseWhenReady` for guarded invocation.
pub fn install(api: &Il2CppApi) {
    let Some(class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Interstitial", "InterstitialViewController")
    else {
        log::warn!(target: "Interstitial", "InterstitialViewController not found");
        return;
    };

    // Resolve CloseWhenReady (0 params) for guarded invocation.
    if !resolver::resolve_method_into(api, class, "CloseWhenReady", 0, &CLOSE_WHEN_READY_FN) {
        return;
    }

    // Hook AboutToShow (0 params).
    tracker::install_resolved_hook(
        api,
        class,
        "AboutToShow",
        0,
        "Interstitial",
        hook_about_to_show as *const (),
        |orig| ORIGINAL_FN.store(orig as *mut (), Relaxed),
    );
}
