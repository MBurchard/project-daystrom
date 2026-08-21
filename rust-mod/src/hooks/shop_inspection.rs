//! Allow shop bundles to be inspected while their redemption cooldown is active.
//!
//! Hooks `BundleDataWidget.OnActionButtonPressedCallback()` and asks the bundle's own view-state renderer to expose
//! its auxiliary view button. Cooldown bundles are routed to the game's auxiliary view action, while every other
//! bundle continues through the original action unchanged.

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

/// `BundleDataWidget.ItemState.ShowViewButton`, verified across both retained game dumps.
const SHOW_VIEW_BUTTON: i32 = 1 << 10;

/// Original trampoline for `BundleDataWidget.OnActionButtonPressedCallback()`.
static ORIGINAL_ACTION: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `BundleDataWidget.SetButtonState()`.
static ORIGINAL_SET_BUTTON_STATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Game method metadata for `BundleDataWidget.AuxViewButtonPressedHandler()`.
static AUXILIARY_VIEW_ACTION: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Runtime offset of `BundleDataWidget._currentState`.
static CURRENT_STATE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Runtime offset of `BundleDataWidget._auxiliaryViewButton`.
static AUXILIARY_VIEW_BUTTON_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Game method metadata for `BundleDataWidget.SetViewButton()`.
static SET_VIEW_BUTTON: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Game method metadata for `GenericButtonWidget.OverrideInteractable(bool)`.
static OVERRIDE_INTERACTABLE: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("ShopInspection");

/// ABI of a no-argument bundle-widget callback trampoline.
type BundleCallbackFn = unsafe extern "C" fn(*mut Il2CppObject);

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

/// Write the bundle state through its runtime-resolved field offset.
fn set_current_state(this: *mut Il2CppObject, state: i32) -> bool {
    let offset = CURRENT_STATE_OFFSET.load(Relaxed);
    if this.is_null() || offset == 0 {
        return false;
    }

    unsafe { *(this.cast::<u8>().add(offset).cast::<i32>()) = state };
    true
}

/// Add the view-button flag without changing any other bundle-state flags.
fn state_with_view_button(state: i32) -> i32 {
    state | SHOW_VIEW_BUTTON
}

/// Invoke the original action callback trampoline when it is available.
fn invoke_original_action(this: *mut Il2CppObject) {
    invoke_original_callback(&ORIGINAL_ACTION, this);
}

/// Return whether two IL2CPP methods share one compiled implementation.
fn methods_share_implementation(first: *const MethodInfo, second: *const MethodInfo) -> bool {
    if first.is_null() || second.is_null() {
        return false;
    }

    unsafe { (*first).method_pointer == (*second).method_pointer }
}

/// Return whether every resolved method has a distinct compiled implementation.
fn method_implementations_are_distinct(methods: &[*const MethodInfo]) -> bool {
    methods.iter().enumerate().all(|(index, first)| {
        !first.is_null()
            && methods[index + 1..]
                .iter()
                .all(|second| !methods_share_implementation(*first, *second))
    })
}

/// Route action-button clicks through the cooldown inspection behaviour.
extern "C" fn hook_action_button_pressed(this: *mut Il2CppObject) {
    let inspected = HOOK_INFO.run_or(false, || {
        let auxiliary = AUXILIARY_VIEW_ACTION.load(Relaxed);
        let state = current_state(this);
        if click_action(state) != ClickAction::Inspect {
            return false;
        }

        invoke::void(auxiliary, this, "BundleDataWidget.AuxViewButtonPressedHandler")
    });

    if !inspected {
        invoke_original_action(this);
    }
}

/// Reveal and enable the auxiliary view button after the game has configured a cooldown bundle.
extern "C" fn hook_set_button_state(this: *mut Il2CppObject) {
    invoke_original_callback(&ORIGINAL_SET_BUTTON_STATE, this);

    HOOK_INFO.run(|| {
        let Some(original_state) = current_state(this) else {
            return;
        };
        if click_action(Some(original_state)) != ClickAction::Inspect {
            return;
        }

        let offset = AUXILIARY_VIEW_BUTTON_OFFSET.load(Relaxed);
        if offset == 0 {
            return;
        }

        let button = unsafe { tracker::read_ptr(this.cast(), offset) }.cast::<Il2CppObject>();
        if button.is_null() {
            return;
        }

        let visible_state = state_with_view_button(original_state);
        if !set_current_state(this, visible_state) {
            return;
        }
        if !invoke::void(SET_VIEW_BUTTON.load(Relaxed), this, "BundleDataWidget.SetViewButton") {
            return;
        }

        invoke::void_bool(
            OVERRIDE_INTERACTABLE.load(Relaxed),
            button,
            true,
            "GenericButtonWidget.OverrideInteractable",
        );
    });
}

/// Invoke a stored no-argument bundle-widget trampoline when it is available.
fn invoke_original_callback(original: &AtomicPtr<()>, this: *mut Il2CppObject) {
    let original = original.load(Relaxed);
    if !original.is_null() {
        let original: BundleCallbackFn = unsafe { std::mem::transmute(original) };
        unsafe { original(this) };
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

    if !resolver::resolve_field_offset_into(api, class, "_currentState", &CURRENT_STATE_OFFSET)
        || !resolver::resolve_field_offset_into(api, class, "_auxiliaryViewButton", &AUXILIARY_VIEW_BUTTON_OFFSET)
    {
        return;
    }

    let Some(action_method) = resolver::resolve_method(api, class, "OnActionButtonPressedCallback", 0) else {
        return;
    };
    let Some(auxiliary_method) = resolver::resolve_method(api, class, "AuxViewButtonPressedHandler", 0) else {
        return;
    };
    let Some(set_button_state_method) = resolver::resolve_method(api, class, "SetButtonState", 0) else {
        return;
    };
    let Some(set_view_button_method) = resolver::resolve_method(api, class, "SetViewButton", 0) else {
        return;
    };
    let Some(button_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "GenericButtonWidget")
    else {
        return;
    };
    let Some(override_interactable_method) = resolver::resolve_method(api, button_class, "OverrideInteractable", 1)
    else {
        return;
    };
    if !method_implementations_are_distinct(&[
        action_method,
        auxiliary_method,
        set_button_state_method,
        set_view_button_method,
        override_interactable_method,
    ]) {
        log::warn!(target: "ShopInspection", "Required methods share a compiled implementation; hooks disabled");
        return;
    }

    AUXILIARY_VIEW_ACTION.store(auxiliary_method.cast_mut(), Relaxed);
    SET_VIEW_BUTTON.store(set_view_button_method.cast_mut(), Relaxed);
    OVERRIDE_INTERACTABLE.store(override_interactable_method.cast_mut(), Relaxed);

    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "SetButtonState",
        0,
        "ShopInspection.CardButton",
        hook_set_button_state as *const (),
        &ORIGINAL_SET_BUTTON_STATE,
    );
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
        assert_eq!(
            click_action(Some(COOLDOWN_TIMER_ON | 1 | SHOW_VIEW_BUTTON)),
            ClickAction::Inspect
        );
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

    #[test]
    fn hook_methods_require_distinct_implementations() {
        let first = MethodInfo {
            method_pointer: first_action as *const (),
        };
        let folded = MethodInfo {
            method_pointer: first_action as *const (),
        };
        let distinct = MethodInfo {
            method_pointer: second_action as *const (),
        };

        assert!(method_implementations_are_distinct(&[&first, &distinct]));
        assert!(!method_implementations_are_distinct(&[&first, &folded]));
        assert!(!method_implementations_are_distinct(&[&first, std::ptr::null()]));
    }
}
