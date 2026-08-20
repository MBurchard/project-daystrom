//! Allow shop bundles to be inspected while their redemption cooldown is active.
//!
//! Hooks `BundleDataWidget.OnActionButtonPressedCallback()`. Cooldown bundles are routed to the game's own auxiliary
//! view action, while every other bundle continues through the original action unchanged.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::Relaxed};

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::compatibility;
use crate::il2cpp::compatibility_manifest as manifest;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// `BundleDataWidget.ItemState.CooldownTimerOn`, verified across both retained game dumps.
const COOLDOWN_TIMER_ON: i32 = 1 << 14;

/// Original trampoline for `BundleDataWidget.OnActionButtonPressedCallback()`.
static ORIGINAL_ACTION: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Game method metadata for `BundleDataWidget.AuxViewButtonPressedHandler()`.
static AUXILIARY_VIEW_ACTION: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Runtime offset of `BundleDataWidget._currentState`.
static CURRENT_STATE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("ShopInspection");

/// ABI of the original action callback trampoline.
type BundleActionFn = unsafe extern "C" fn(*mut Il2CppObject);

/// Action selected for a bundle click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClickAction {
    /// Preserve the game's normal purchase, claim, or showcase behaviour.
    Original,
    /// Open the game's read-only bundle contents view.
    Inspect,
}

/// Select the safe action for the current bundle state.
fn click_action(state: Option<i32>) -> ClickAction {
    match state {
        Some(state) if state & COOLDOWN_TIMER_ON != 0 => ClickAction::Inspect,
        _ => ClickAction::Original,
    }
}

/// Read the current bundle state through its runtime-resolved field offset.
fn current_state(this: *mut Il2CppObject) -> Option<i32> {
    let offset = CURRENT_STATE_OFFSET.load(Relaxed);
    if this.is_null() || offset == 0 {
        return None;
    }

    Some(unsafe { tracker::read_i32(this.cast(), offset) })
}

/// Invoke the original callback trampoline when it is available.
fn invoke_original(this: *mut Il2CppObject) {
    let original = ORIGINAL_ACTION.load(Relaxed);
    if !original.is_null() {
        let original: BundleActionFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this) };
    }
}

/// Return whether two IL2CPP methods share one compiled implementation.
fn methods_share_implementation(first: *const MethodInfo, second: *const MethodInfo) -> bool {
    if first.is_null() || second.is_null() {
        return false;
    }

    unsafe { (*first).method_pointer == (*second).method_pointer }
}

/// Route cooldown bundle clicks to the read-only contents view.
extern "C" fn hook_action_button_pressed(this: *mut Il2CppObject) {
    let inspected = HOOK_INFO.run_or(false, || {
        let auxiliary = AUXILIARY_VIEW_ACTION.load(Relaxed);
        click_action(current_state(this)) == ClickAction::Inspect
            && invoke::void(auxiliary, this, "BundleDataWidget.AuxViewButtonPressedHandler")
    });

    if !inspected {
        invoke_original(this);
    }
}

/// Install the shop bundle inspection hook.
pub fn install(api: &Il2CppApi) {
    if !compatibility::is_enabled(manifest::SHOP_INSPECTION) {
        return;
    }

    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Shop", "BundleDataWidget") else {
        log::warn!(target: "ShopInspection", "BundleDataWidget not found");
        return;
    };

    if !resolver::resolve_field_offset_into(api, class, "_currentState", &CURRENT_STATE_OFFSET) {
        return;
    }

    let Some(action_method) = resolver::resolve_method(api, class, "OnActionButtonPressedCallback", 0) else {
        return;
    };
    let Some(auxiliary_method) = resolver::resolve_method(api, class, "AuxViewButtonPressedHandler", 0) else {
        return;
    };
    if methods_share_implementation(action_method, auxiliary_method) {
        log::warn!(target: "ShopInspection", "Action and inspection methods share one implementation; hook disabled");
        return;
    }
    AUXILIARY_VIEW_ACTION.store(auxiliary_method.cast_mut(), Relaxed);

    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "OnActionButtonPressedCallback",
        0,
        "ShopInspection",
        hook_action_button_pressed as *const (),
        &ORIGINAL_ACTION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_bundle_preserves_original_action() {
        assert_eq!(click_action(Some(0)), ClickAction::Original);
    }

    #[test]
    fn cooldown_bundle_opens_inspection() {
        assert_eq!(click_action(Some(COOLDOWN_TIMER_ON)), ClickAction::Inspect);
    }

    #[test]
    fn cooldown_bit_is_detected_among_other_state_flags() {
        assert_eq!(click_action(Some(COOLDOWN_TIMER_ON | 1 | 1024)), ClickAction::Inspect);
    }

    #[test]
    fn unavailable_state_falls_back_to_original_action() {
        assert_eq!(click_action(None), ClickAction::Original);
    }

    unsafe extern "C" fn first_action(_: *mut Il2CppObject) {}

    unsafe extern "C" fn second_action(_: *mut Il2CppObject) {}

    #[test]
    fn identical_code_folding_is_detected_by_shared_method_pointer() {
        let first = MethodInfo {
            method_pointer: first_action as *const (),
        };
        let folded = MethodInfo {
            method_pointer: first_action as *const (),
        };
        let distinct = MethodInfo {
            method_pointer: second_action as *const (),
        };

        assert!(methods_share_implementation(&first, &folded));
        assert!(!methods_share_implementation(&first, &distinct));
    }
}
