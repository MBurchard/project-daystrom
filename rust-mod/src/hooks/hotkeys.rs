//! Hotkey hooks for quality-of-life keyboard shortcuts.
//!
//! Hooks `ScreenManager.Update()` (per-frame) to intercept key presses.
//! Currently, handles ESC on reward/collect dialogues that the game ignores.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::{debug, error, warn};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// Unity `KeyCode.Escape` value.
const KEYCODE_ESCAPE: i32 = 27;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `ScreenManager.Update()`.
static ORIGINAL_UPDATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `Input.GetKeyDownInt(KeyCode) -> bool`.
static GET_KEY_DOWN_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Tracked `AnimatedRewardsScreenViewController` instance pointer.
/// Set in Awake, cleared in OnDestroy.
static REWARD_INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original Awake function pointer.
static ORIGINAL_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original OnDestroy function pointer.
static ORIGINAL_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `AnimatedRewardsScreenViewController.OnCollectClicked()`.
static ON_COLLECT_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `IsActive() -> bool` on the reward controller.
static IS_ACTIVE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking.
static HOOK_INFO: HookInfo = HookInfo::new("Hotkeys");

/// Whether the first ESC collect has been logged.
static LOGGED_FIRST_COLLECT: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type UpdateFn = unsafe extern "C" fn(*mut Il2CppObject);
type AwakeFn = unsafe extern "C" fn(*mut Il2CppObject);
type DestroyFn = unsafe extern "C" fn(*mut Il2CppObject);
type GetKeyDownIntFn = unsafe extern "C" fn(i32) -> bool;
type IsActiveFn = unsafe extern "C" fn(*mut Il2CppObject) -> bool;
type OnCollectFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Input helper ---------------------------------------------------------

/// Check whether a key was pressed in this frame.
fn key_down(key: i32) -> bool {
    let ptr = GET_KEY_DOWN_FN.load(Relaxed);
    if ptr.is_null() {
        return false;
    }
    let get_key_down: GetKeyDownIntFn = unsafe { std::mem::transmute(ptr) };
    unsafe { get_key_down(key) }
}

// ---- Instance tracking hooks ----------------------------------------------

/// Hook for `AnimatedRewardsScreenViewController.Awake()`.
///
/// Stores the instance pointer so the Update hook can check it without an expensive FindObjectOfType search.
extern "C" fn hook_awake(this: *mut Il2CppObject) {
    REWARD_INSTANCE.store(this as *mut (), Relaxed);
    let original: AwakeFn = unsafe { std::mem::transmute(ORIGINAL_AWAKE.load(Relaxed)) };
    unsafe { original(this) };
}

/// Hook for `AnimatedRewardsScreenViewController.OnDestroy()`.
///
/// Clears the cached instance pointer.
extern "C" fn hook_destroy(this: *mut Il2CppObject) {
    // Only clear if it's still our tracked instance (avoid race with a new one).
    let _ = REWARD_INSTANCE.compare_exchange(
        this as *mut (),
        std::ptr::null_mut(),
        Relaxed,
        Relaxed,
    );
    let original: DestroyFn = unsafe { std::mem::transmute(ORIGINAL_DESTROY.load(Relaxed)) };
    unsafe { original(this) };
}

// ---- Main update hook -----------------------------------------------------

/// Hook for `ScreenManager.Update()`.
///
/// Runs after the original update. Checks for the ESC key and collects active reward screens.
extern "C" fn hook_update(this: *mut Il2CppObject) {
    let original: UpdateFn = unsafe { std::mem::transmute(ORIGINAL_UPDATE.load(Relaxed)) };
    unsafe { original(this) };

    if !HOOK_INFO.is_active() {
        return;
    }

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if !key_down(KEYCODE_ESCAPE) {
            return;
        }
        collect_reward_screen();
    }));

    if result.is_err() {
        HOOK_INFO.record_error();
    }
}

/// If an `AnimatedRewardsScreenViewController` is tracked and active, collect rewards via `OnCollectClicked()`.
///
/// Unlike the stfc-mod (which calls `GoBackToLastSection` and merely dismisses), we trigger the
/// actual collect action. This handles both `ClaimOnShow` (already claimed, just closes) and
/// `ClaimOnCollect` (triggers the claim callback, then closes).
fn collect_reward_screen() {
    let instance = REWARD_INSTANCE.load(Relaxed) as *mut Il2CppObject;
    if instance.is_null() {
        return;
    }

    // Check IsActive — skip if we couldn't resolve the method.
    let is_active_ptr = IS_ACTIVE_FN.load(Relaxed);
    if !is_active_ptr.is_null() {
        let is_active: IsActiveFn = unsafe { std::mem::transmute(is_active_ptr) };
        if !unsafe { is_active(instance) } {
            return;
        }
    }

    let on_collect_ptr = ON_COLLECT_FN.load(Relaxed);
    if on_collect_ptr.is_null() {
        return;
    }

    if !LOGGED_FIRST_COLLECT.swap(true, Relaxed) {
        debug!(target: "Hotkeys", "ESC: collecting reward screen");
    }

    let on_collect: OnCollectFn = unsafe { std::mem::transmute(on_collect_ptr) };
    unsafe { on_collect(instance) };
}

// ---- Installation ---------------------------------------------------------

/// Install all hotkey-related hooks.
///
/// Resolves Input.GetKeyDownInt for key detection, tracks AnimatedRewardsScreenViewController instances,
/// and hooks ScreenManager.Update() for per-frame key checks.
pub fn install(api: &Il2CppApi) {
    if !install_input(api) {
        return;
    }
    install_reward_tracking(api);
    install_update_hook(api);
}

/// Resolve `Input.GetKeyDownInt(KeyCode)` and store the function pointer.
///
/// Returns `false` if resolution fails (remaining hooks would be useless).
fn install_input(api: &Il2CppApi) -> bool {
    let Some(class) = resolver::resolve_class(
        api, "UnityEngine.InputLegacyModule", "UnityEngine", "Input",
    ) else {
        warn!(target: "Hotkeys", "Input class not found, hotkeys disabled");
        return false;
    };

    let Some(method) = resolver::resolve_method(api, class, "GetKeyDownInt", 1) else {
        warn!(target: "Hotkeys", "Input.GetKeyDownInt not found, hotkeys disabled");
        return false;
    };

    let ptr = unsafe { (*method).method_pointer };
    GET_KEY_DOWN_FN.store(ptr as *mut (), Relaxed);
    debug!(target: "Hotkeys", "Input.GetKeyDownInt resolved");
    true
}

/// Hook `AnimatedRewardsScreenViewController.Awake()` and `OnDestroy()` for
/// instance tracking, and resolve `OnCollectClicked()` + `IsActive()`.
fn install_reward_tracking(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api,
        "Assembly-CSharp",
        "Digit.Prime.Missions.UI",
        "AnimatedRewardsScreenViewController",
    ) else {
        warn!(target: "Hotkeys", "AnimatedRewardsScreenViewController not found");
        return;
    };

    // Resolve OnCollectClicked (required).
    let Some(on_collect) = resolver::resolve_method(api, class, "OnCollectClicked", 0) else {
        warn!(target: "Hotkeys", "OnCollectClicked not found");
        return;
    };
    ON_COLLECT_FN.store(unsafe { (*on_collect).method_pointer } as *mut (), Relaxed);

    // Resolve IsActive (optional, skip if not found).
    if let Some(is_active) = resolver::resolve_method(api, class, "IsActive", 0) {
        IS_ACTIVE_FN.store(unsafe { (*is_active).method_pointer } as *mut (), Relaxed);
    } else {
        warn!(target: "Hotkeys", "IsActive not found, skipping active check");
    }

    // Hook Awake (virtual, Slot 4).
    if let Some(awake) = resolver::resolve_method(api, class, "Awake", 0) {
        let target = unsafe { (*awake).method_pointer };
        match engine::install_hook("RewardAwake", target, hook_awake as *const ()) {
            Ok(original) => {
                ORIGINAL_AWAKE.store(original as *mut (), Relaxed);
                debug!(target: "Hotkeys", "Reward Awake hook installed");
            }
            Err(e) => warn!(target: "Hotkeys", "Failed to hook Awake: {e}"),
        }
    }

    // Hook OnDestroy (virtual, Slot 8).
    if let Some(destroy) = resolver::resolve_method(api, class, "OnDestroy", 0) {
        let target = unsafe { (*destroy).method_pointer };
        match engine::install_hook("RewardDestroy", target, hook_destroy as *const ()) {
            Ok(original) => {
                ORIGINAL_DESTROY.store(original as *mut (), Relaxed);
                debug!(target: "Hotkeys", "Reward OnDestroy hook installed");
            }
            Err(e) => warn!(target: "Hotkeys", "Failed to hook OnDestroy: {e}"),
        }
    }
}

/// Hook `ScreenManager.Update()` for per-frame key checks.
///
/// Falls back to `LateUpdate()` if `Update` is not found (Update may not appear in the IL2CPP dump if
/// it's compiler-generated).
fn install_update_hook(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager",
    ) else {
        return;
    };

    // Try Update first, fall back to LateUpdate.
    let (name, method) =
        if let Some(m) = resolver::resolve_method(api, class, "Update", 0) {
            ("Update", m)
        } else if let Some(m) = resolver::resolve_method(api, class, "LateUpdate", 0) {
            warn!(target: "Hotkeys", "Update not found, falling back to LateUpdate");
            ("LateUpdate", m)
        } else {
            error!(target: "Hotkeys", "Neither Update nor LateUpdate found on ScreenManager");
            return;
        };

    let target = unsafe { (*method).method_pointer };
    match engine::install_hook("Hotkeys", target, hook_update as *const ()) {
        Ok(original) => {
            ORIGINAL_UPDATE.store(original as *mut (), Relaxed);
            debug!(
                target: "Hotkeys",
                "Hotkeys hook installed (ScreenManager.{name})"
            );
        }
        Err(e) => {
            error!(target: "Hotkeys", "Failed to hook ScreenManager.{name}: {e}");
        }
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_escape_is_27() {
        assert_eq!(KEYCODE_ESCAPE, 27);
    }

    #[test]
    fn reward_instance_starts_null() {
        assert!(REWARD_INSTANCE.load(Relaxed).is_null());
    }

    #[test]
    fn key_down_returns_false_when_fn_not_resolved() {
        // GET_KEY_DOWN_FN is null by default, key_down must return false.
        assert!(!key_down(KEYCODE_ESCAPE));
    }

    #[test]
    fn collect_is_noop_without_instance() {
        // Should not panic when no instance is tracked.
        collect_reward_screen();
    }

    #[test]
    fn compare_exchange_clears_matching_instance() {
        let fake = 0x1234usize as *mut ();
        REWARD_INSTANCE.store(fake, Relaxed);

        let _ = REWARD_INSTANCE.compare_exchange(
            fake,
            std::ptr::null_mut(),
            Relaxed,
            Relaxed,
        );
        assert!(REWARD_INSTANCE.load(Relaxed).is_null());
    }

    #[test]
    fn compare_exchange_preserves_different_instance() {
        let fake_a = 0x1234usize as *mut ();
        let fake_b = 0x5678usize as *mut ();
        REWARD_INSTANCE.store(fake_b, Relaxed);

        // Trying to clear fake_a should not affect fake_b.
        let _ = REWARD_INSTANCE.compare_exchange(
            fake_a,
            std::ptr::null_mut(),
            Relaxed,
            Relaxed,
        );
        assert_eq!(REWARD_INSTANCE.load(Relaxed), fake_b);

        // Cleanup.
        REWARD_INSTANCE.store(std::ptr::null_mut(), Relaxed);
    }
}
