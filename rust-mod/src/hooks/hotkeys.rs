//! Hotkey hooks for quality-of-life keyboard shortcuts.
//!
//! Hooks `ScreenManager.Update()` (per-frame) to intercept key presses.
//! Currently, handles ESC on reward/collect dialogues and delegates SPACE to the `spacebar` module for
//! default-action execution.
//!
//! `Input.GetKeyDownInt` is hooked (not just resolved) so that consumed keys can be suppressed for the rest
//! of the frame. This prevents the game's own shortcut system from also reacting to keys we already handled.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::{debug, error, warn};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::hooks::tracker::{self, instance_tracker};
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// Unity `KeyCode.Escape` value.
const KEYCODE_ESCAPE: i32 = 27;

/// Unity `KeyCode.Space` value.
const KEYCODE_SPACE: i32 = 32;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `ScreenManager.Update()`.
static ORIGINAL_UPDATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original (trampoline) function pointer for `Input.GetKeyDownInt(KeyCode)`.
///
/// Our `key_down()` helper calls the trampoline directly, bypassing the consumption check. Game code goes
/// through the hook and sees consumed keys as not pressed.
static ORIGINAL_GET_KEY_DOWN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Set to `true` after we handle Space in a frame. Our `GetKeyDownInt` hook returns `false` for Space
/// while this flag is set, preventing the game from also processing it.
static SPACE_CONSUMED: AtomicBool = AtomicBool::new(false);

// AnimatedRewardsScreenViewController instance tracker.
instance_tracker!(reward);

/// Function pointer for `ScreenManager.get_IsInputFocused()` (static).
static IS_INPUT_FOCUSED_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `EventSystem.get_current()` (static, returns instance).
static EVENT_SYSTEM_CURRENT_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `EventSystem.SetSelectedGameObject(GameObject)`.
static SET_SELECTED_GO_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `AnimatedRewardsScreenViewController.OnCollectClicked()`.
static ON_COLLECT_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `IsActive() -> bool` on the reward controller.
static IS_ACTIVE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ShortcutsManager.InitializeActions()`.
///
/// Stored but never called: suppressing the original prevents Scopely's keyboard shortcuts from being
/// registered, avoiding conflicts with our own hotkey handling.
#[allow(dead_code)]
static ORIG_INITIALIZE_ACTIONS: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking.
static HOOK_INFO: HookInfo = HookInfo::new("Hotkeys");

/// Whether the first ESC collect has been logged.
static LOGGED_FIRST_COLLECT: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type UpdateFn = unsafe extern "C" fn(*mut Il2CppObject);
type GetKeyDownIntFn = unsafe extern "C" fn(i32) -> bool;
type IsInputFocusedFn = unsafe extern "C" fn() -> bool;
type GetCurrentEventSystemFn = unsafe extern "C" fn() -> *mut Il2CppObject;
type SetSelectedGoFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject);
type IsActiveFn = unsafe extern "C" fn(*mut Il2CppObject) -> bool;
type OnCollectFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Input helpers --------------------------------------------------------

/// Check whether a key was pressed in this frame.
///
/// Calls the original `GetKeyDownInt` trampoline directly, bypassing the consumption hook. This ensures our
/// own code always sees the real input state.
pub(super) fn key_down(key: i32) -> bool {
    let ptr = ORIGINAL_GET_KEY_DOWN.load(Relaxed);
    if ptr.is_null() {
        return false;
    }
    let get_key_down: GetKeyDownIntFn = unsafe { std::mem::transmute(ptr) };
    unsafe { get_key_down(key) }
}

/// Returns `true` if a text input field is focused (chat, search, etc.).
///
/// Uses `ScreenManager.IsInputFocused` (static property). If the method could not be resolved, returns `false`
/// (optimistic: proceed with action).
pub(super) fn is_input_focused() -> bool {
    let ptr = IS_INPUT_FOCUSED_FN.load(Relaxed);
    if ptr.is_null() {
        return false;
    }
    let is_focused: IsInputFocusedFn = unsafe { std::mem::transmute(ptr) };
    unsafe { is_focused() }
}

/// Deselect the EventSystem's current selection.
///
/// Unity's `StandaloneInputModule` treats Space/Enter as "Submit", clicking whatever UI element is
/// selected. Calling this after we handle Space prevents that side effect.
fn deselect_event_system() {
    let current_fn_ptr = EVENT_SYSTEM_CURRENT_FN.load(Relaxed);
    if current_fn_ptr.is_null() {
        return;
    }
    let get_current: GetCurrentEventSystemFn =
        unsafe { std::mem::transmute(current_fn_ptr) };
    let event_system = unsafe { get_current() };
    if event_system.is_null() {
        return;
    }
    let set_fn_ptr = SET_SELECTED_GO_FN.load(Relaxed);
    if set_fn_ptr.is_null() {
        return;
    }
    let set_selected: SetSelectedGoFn = unsafe { std::mem::transmute(set_fn_ptr) };
    unsafe { set_selected(event_system, std::ptr::null_mut()) };
}

// ---- ShortcutsManager suppression -----------------------------------------

/// Hook for `ShortcutsManager.InitializeActions()`.
///
/// Suppresses the original to prevent Scopely's keyboard shortcuts (Unity Input System actions) from being
/// registered. Without this, the game's own Space binding ("find selected ship") fires alongside our hooks.
extern "C" fn hook_initialize_actions(_this: *mut Il2CppObject) {
    debug!(target: "Hotkeys", "ShortcutsManager.InitializeActions suppressed");
}

// ---- GetKeyDownInt hook ---------------------------------------------------

/// Hook for `Input.GetKeyDownInt(KeyCode)`.
///
/// If a key has been consumed by our hotkey system in this frame, it returns `false` so the game's own shortcut
/// system does not also react to it.
extern "C" fn hook_get_key_down(key: i32) -> bool {
    if key == KEYCODE_SPACE && SPACE_CONSUMED.load(Relaxed) {
        return false;
    }
    let orig_ptr = ORIGINAL_GET_KEY_DOWN.load(Relaxed);
    if orig_ptr.is_null() {
        return false;
    }
    let original: GetKeyDownIntFn = unsafe { std::mem::transmute(orig_ptr) };
    unsafe { original(key) }
}

// ---- Main update hook -----------------------------------------------------

/// Hook for `ScreenManager.Update()`.
///
/// Processes hotkeys BEFORE calling the original update. This way consumed keys are already suppressed when
/// the game's own update logic runs.
extern "C" fn hook_update(this: *mut Il2CppObject) {
    // Reset consumption flags at the start of each frame.
    SPACE_CONSUMED.store(false, Relaxed);

    if HOOK_INFO.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if key_down(KEYCODE_ESCAPE) {
                collect_reward_screen();
            }
            if key_down(KEYCODE_SPACE)
                && !is_input_focused()
                && super::spacebar::check()
            {
                SPACE_CONSUMED.store(true, Relaxed);
                deselect_event_system();
            }
        }));

        if result.is_err() {
            HOOK_INFO.record_error();
        }
    }

    // Original update runs AFTER our key processing.
    let orig_ptr = ORIGINAL_UPDATE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: UpdateFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

/// If an `AnimatedRewardsScreenViewController` is tracked and active, collect rewards via `OnCollectClicked()`.
///
/// Unlike the stfc-mod (which calls `GoBackToLastSection` and merely dismisses), we trigger the actual collect action.
/// This handles both `ClaimOnShow` (already claimed, just closes) and `ClaimOnCollect` (triggers the claim callback,
/// then closes).
fn collect_reward_screen() {
    let instance = reward::get() as *mut Il2CppObject;
    if instance.is_null() {
        return;
    }

    // Check IsActive; skip if we couldn't resolve the method.
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
/// Hooks Input.GetKeyDownInt for key detection and consumption, tracks AnimatedRewardsScreenViewController
/// instances, installs spacebar hooks, and hooks ScreenManager.Update() for per-frame key checks.
pub fn install(api: &Il2CppApi) {
    suppress_scopely_shortcuts(api);
    if !install_input(api) {
        return;
    }
    install_reward_tracking(api);
    super::spacebar::install(api);
    install_update_hook(api);
}

/// Hook `ShortcutsManager.InitializeActions()` to prevent the game's own keyboard shortcuts from being
/// registered.
///
/// Scopely's shortcuts use Unity's new Input System and would conflict with our hotkey hooks (e.g. Space =
/// "find selected ship" instead of engage).
fn suppress_scopely_shortcuts(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.GameInput", "ShortcutsManager",
    ) else {
        warn!(target: "Hotkeys", "ShortcutsManager not found, game shortcuts may conflict");
        return;
    };
    let Some(ptr) = tracker::resolve_fn(api, class, "InitializeActions", 0) else {
        warn!(target: "Hotkeys", "InitializeActions not found, game shortcuts may conflict");
        return;
    };
    match engine::install_hook(
        "InitializeActions", ptr, hook_initialize_actions as *const (),
    ) {
        Ok(orig) => {
            ORIG_INITIALIZE_ACTIONS.store(orig as *mut (), Relaxed);
            debug!(target: "Hotkeys", "Scopely shortcuts suppressed");
        }
        Err(e) => warn!(target: "Hotkeys", "Failed to suppress Scopely shortcuts: {e}"),
    }
}

/// Hook `Input.GetKeyDownInt(KeyCode)` and resolve `ScreenManager.IsInputFocused`.
///
/// GetKeyDownInt is hooked (not just resolved) so consumed keys can be suppressed for the game's own code.
/// Returns `false` if the hook cannot be installed (remaining hooks would be useless).
fn install_input(api: &Il2CppApi) -> bool {
    let Some(input_class) = resolver::resolve_class(
        api, "UnityEngine.InputLegacyModule", "UnityEngine", "Input",
    ) else {
        warn!(target: "Hotkeys", "Input class not found, hotkeys disabled");
        return false;
    };

    let Some(ptr) = tracker::resolve_fn(api, input_class, "GetKeyDownInt", 1) else {
        warn!(target: "Hotkeys", "Input.GetKeyDownInt not found, hotkeys disabled");
        return false;
    };

    match engine::install_hook("GetKeyDownInt", ptr, hook_get_key_down as *const ()) {
        Ok(original) => {
            ORIGINAL_GET_KEY_DOWN.store(original as *mut (), Relaxed);
            debug!(target: "Hotkeys", "Input.GetKeyDownInt hooked");
        }
        Err(e) => {
            warn!(target: "Hotkeys", "Failed to hook GetKeyDownInt, falling back to direct call: {e}");
            ORIGINAL_GET_KEY_DOWN.store(ptr as *mut (), Relaxed);
        }
    }

    // IsInputFocused is optional; spacebar/future keys still work without it.
    if let Some(sm_class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager",
    ) {
        if let Some(ptr) = tracker::resolve_fn(api, sm_class, "get_IsInputFocused", 0) {
            IS_INPUT_FOCUSED_FN.store(ptr as *mut (), Relaxed);
            debug!(target: "Hotkeys", "ScreenManager.IsInputFocused resolved");
        } else {
            warn!(target: "Hotkeys", "get_IsInputFocused not found, input focus guard disabled");
        }
    }

    // EventSystem: deselect UI after handling Space to prevent Unity's Submit action.
    if let Some(es_class) = resolver::resolve_class(
        api, "UnityEngine.UI", "UnityEngine.EventSystems", "EventSystem",
    ) {
        if let Some(ptr) = tracker::resolve_fn(api, es_class, "get_current", 0) {
            EVENT_SYSTEM_CURRENT_FN.store(ptr as *mut (), Relaxed);
        }
        if let Some(ptr) = tracker::resolve_fn(api, es_class, "SetSelectedGameObject", 1) {
            SET_SELECTED_GO_FN.store(ptr as *mut (), Relaxed);
        }
        if !EVENT_SYSTEM_CURRENT_FN.load(Relaxed).is_null()
            && !SET_SELECTED_GO_FN.load(Relaxed).is_null()
        {
            debug!(target: "Hotkeys", "EventSystem deselect resolved");
        } else {
            warn!(target: "Hotkeys", "EventSystem partially resolved, UI deselect may not work");
        }
    }

    true
}

/// Hook `AnimatedRewardsScreenViewController.Awake()` and `OnDestroy()` for instance, tracking, and resolve
/// `OnCollectClicked()` + `IsActive()`.
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
    let Some(ptr) = tracker::resolve_fn(api, class, "OnCollectClicked", 0) else {
        warn!(target: "Hotkeys", "OnCollectClicked not found");
        return;
    };
    ON_COLLECT_FN.store(ptr as *mut (), Relaxed);

    // Resolve IsActive (optional, skip if not found).
    if let Some(ptr) = tracker::resolve_fn(api, class, "IsActive", 0) {
        IS_ACTIVE_FN.store(ptr as *mut (), Relaxed);
    } else {
        warn!(target: "Hotkeys", "IsActive not found, skipping active check");
    }

    // Hook Awake/OnDestroy for instance tracking.
    reward::install(api, class, "Reward");
}

/// Hook `ScreenManager.Update()` for per-frame key checks.
///
/// Falls back to `LateUpdate()` if `Update` is not found (Update may not appear in the IL2CPP dump if it's
/// compiler-generated).
fn install_update_hook(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager",
    ) else {
        return;
    };

    // Try Update first, fall back to LateUpdate.
    let (name, ptr) =
        if let Some(p) = tracker::resolve_fn(api, class, "Update", 0) {
            ("Update", p)
        } else if let Some(p) = tracker::resolve_fn(api, class, "LateUpdate", 0) {
            warn!(target: "Hotkeys", "Update not found, falling back to LateUpdate");
            ("LateUpdate", p)
        } else {
            error!(target: "Hotkeys", "Neither Update nor LateUpdate found on ScreenManager");
            return;
        };

    match engine::install_hook("Hotkeys", ptr, hook_update as *const ()) {
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
    fn keycode_space_is_32() {
        assert_eq!(KEYCODE_SPACE, 32);
    }

    #[test]
    fn reward_instance_starts_null() {
        assert!(reward::get().is_null());
    }

    #[test]
    fn key_down_returns_false_when_fn_not_resolved() {
        assert!(!key_down(KEYCODE_ESCAPE));
    }

    #[test]
    fn collect_is_noop_without_instance() {
        collect_reward_screen();
    }

    #[test]
    fn is_input_focused_false_when_unresolved() {
        assert!(!is_input_focused());
    }

    #[test]
    fn deselect_event_system_noop_when_unresolved() {
        // Should not panic when function pointers are null.
        deselect_event_system();
    }

    #[test]
    fn space_consumed_starts_false() {
        assert!(!SPACE_CONSUMED.load(Relaxed));
    }
}
