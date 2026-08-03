//! Suppress intrusive automatic popups after game start.
//!
//! Hooks `InterstitialViewController.AboutToShow()` to close the first popup immediately via `CloseWhenReady()`.
//! It also blocks automatic PLC/chained-offer popups and PLC purchase interstitials at their trigger points while
//! preserving manually opened shop offers and non-commercial interstitials. When the setting is off, all popups
//! behave normally.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::debug;

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::compatibility;
use crate::il2cpp::compatibility_manifest as manifest;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `InterstitialViewController.AboutToShow()`.
static ORIGINAL_ABOUT_TO_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `PlcOfferManager.CanTriggerPopup()`.
static ORIGINAL_CAN_TRIGGER_PLC_POPUP: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `InterstitialManager.QueueOrActionPlcInterstitial(string, bool)`.
static ORIGINAL_QUEUE_PLC_INTERSTITIAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `InterstitialViewController.CloseWhenReady()`.
static CLOSE_WHEN_READY_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the first interstitial has already been seen this session.
static FIRST_SEEN: AtomicBool = AtomicBool::new(false);

/// Whether suppression of an automatic PLC/chained-offer popup has already been logged.
static PLC_OFFER_SUPPRESSION_LOGGED: AtomicBool = AtomicBool::new(false);

/// Whether suppression of a PLC purchase interstitial has already been logged.
static PLC_INTERSTITIAL_SUPPRESSION_LOGGED: AtomicBool = AtomicBool::new(false);

/// Per-hook error tracking and deactivation states.
static FIRST_POPUP_HOOK_INFO: HookInfo = HookInfo::new("Interstitial");
static PLC_OFFER_HOOK_INFO: HookInfo = HookInfo::new("PlcOfferPopup");
static PLC_INTERSTITIAL_HOOK_INFO: HookInfo = HookInfo::new("PlcInterstitial");

// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);
type PopupEligibilityFn = unsafe extern "C" fn(*mut Il2CppObject) -> bool;
type PlcInterstitialFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppString, bool);

// ---- Hook -----------------------------------------------------------------

/// Hook for `InterstitialViewController.AboutToShow()`.
///
/// On the first call, if `skip_first_popup` is enabled, calls `CloseWhenReady()` instead of the original to dismiss the
/// ad popup immediately. All subsequent calls pass through normally.
extern "C" fn hook_about_to_show(this: *mut Il2CppObject) {
    if FIRST_POPUP_HOOK_INFO.run_or(false, || try_close_first_popup(this)) {
        return;
    }

    let orig_ptr = ORIGINAL_ABOUT_TO_SHOW.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

/// Hook for `PlcOfferManager.CanTriggerPopup()`.
///
/// Returning `false` prevents the manager's automatic station-section PLC and chained-offer popups. Manually opened
/// offers use the explicit `TriggerPlcOfferPopup` path and are therefore unaffected.
extern "C" fn hook_can_trigger_plc_popup(this: *mut Il2CppObject) -> bool {
    if PLC_OFFER_HOOK_INFO.run_or(false, crate::settings::skip_first_popup) {
        if !PLC_OFFER_SUPPRESSION_LOGGED.swap(true, Relaxed) {
            debug!(target: "Interstitial", "Suppressing automatic PLC and chained-offer popups");
        }
        return false;
    }

    let orig_ptr = ORIGINAL_CAN_TRIGGER_PLC_POPUP.load(Relaxed);
    if orig_ptr.is_null() {
        return false;
    }
    let original: PopupEligibilityFn = unsafe { std::mem::transmute(orig_ptr) };
    unsafe { original(this) }
}

/// Hook for `InterstitialManager.QueueOrActionPlcInterstitial(string, bool)`.
///
/// PLC interstitials are purchase advertisements delivered through the generic interstitial system. Suppressing this
/// dedicated queue/action path leaves ordinary event and informational interstitials untouched.
extern "C" fn hook_queue_plc_interstitial(
    this: *mut Il2CppObject,
    interstitial_id: *mut Il2CppString,
    show_immediate: bool,
) {
    if PLC_INTERSTITIAL_HOOK_INFO.run_or(false, crate::settings::skip_first_popup) {
        if !PLC_INTERSTITIAL_SUPPRESSION_LOGGED.swap(true, Relaxed) {
            debug!(target: "Interstitial", "Suppressing PLC purchase interstitials");
        }
        return;
    }

    let orig_ptr = ORIGINAL_QUEUE_PLC_INTERSTITIAL.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: PlcInterstitialFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this, interstitial_id, show_immediate) };
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

/// Install the interstitial and automatic purchase-popup hooks.
///
/// The commercial-popup hooks are optional and install independently of the existing first-interstitial hook.
pub fn install(api: &Il2CppApi) {
    if !compatibility::is_enabled(manifest::INTERSTITIAL) {
        return;
    }

    install_first_popup_hook(api);
    install_purchase_popup_hooks(api);
}

/// Install the original first-interstitial suppression hook.
fn install_first_popup_hook(api: &Il2CppApi) {
    let Some(class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Interstitial", "InterstitialViewController")
    else {
        log::warn!(target: "Interstitial", "InterstitialViewController not found");
        return;
    };

    // Resolve CloseWhenReady (0 params) for guarded invocation.
    if CLOSE_WHEN_READY_FN.load(Relaxed).is_null()
        && !resolver::resolve_method_into(api, class, "CloseWhenReady", 0, &CLOSE_WHEN_READY_FN)
    {
        return;
    }

    // Hook AboutToShow (0 params).
    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "AboutToShow",
        0,
        "Interstitial",
        hook_about_to_show as *const (),
        &ORIGINAL_ABOUT_TO_SHOW,
    );
}

/// Install the optional automatic PLC/chained-offer and PLC-interstitial suppression hooks.
fn install_purchase_popup_hooks(api: &Il2CppApi) {
    if let Some(class) = resolver::try_resolve_class(api, "Assembly-CSharp", "Digit.Prime.Shop", "PlcOfferManager") {
        tracker::try_install_resolved_hook_if_missing(
            api,
            class,
            "CanTriggerPopup",
            0,
            "PlcOfferPopup",
            hook_can_trigger_plc_popup as *const (),
            &ORIGINAL_CAN_TRIGGER_PLC_POPUP,
        );
    }

    if let Some(class) =
        resolver::try_resolve_class(api, "Assembly-CSharp", "Digit.Prime.Interstitial", "InterstitialManager")
    {
        tracker::try_install_resolved_hook_if_missing(
            api,
            class,
            "QueueOrActionPlcInterstitial",
            2,
            "PlcInterstitial",
            hook_queue_plc_interstitial as *const (),
            &ORIGINAL_QUEUE_PLC_INTERSTITIAL,
        );
    }
}
