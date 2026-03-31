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
use crate::hooks::tracker;
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

/// Function pointer for `ScreenManager.get_IsInputFocused()` (static).
static IS_INPUT_FOCUSED_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `EventSystem.get_current()` (static, returns instance).
static EVENT_SYSTEM_CURRENT_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `EventSystem.SetSelectedGameObject(GameObject)`.
static SET_SELECTED_GO_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `IsActive() -> bool` (from UIBehaviour, shared by all reward screens).
static IS_ACTIVE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ---- Reward screen tracking -----------------------------------------------

/// Per-subclass tracking slot for `GenericRewardsScreenViewController` descendants.
///
/// Each slot holds the IL2CPP class pointer (for runtime dispatch in the base-class Awake hook), the currently
/// tracked instance, and the resolved `OnCollectClicked` function pointer.
struct RewardTarget {
    class: AtomicPtr<()>,
    instance: AtomicPtr<()>,
    on_collect: AtomicPtr<()>,
}

impl RewardTarget {
    const fn new() -> Self {
        Self {
            class: AtomicPtr::new(std::ptr::null_mut()),
            instance: AtomicPtr::new(std::ptr::null_mut()),
            on_collect: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

/// Reward screen tracking slots.
///
/// - Slot 0: `AnimatedRewardsScreenViewController` (has own Awake/OnDestroy overrides)
/// - Slot 1: `ShipScrappingRewardsScreenViewController` (shares base Awake)
/// - Slot 2: `FirstTimeSpenderScreenViewController` (shares base Awake)
/// - Slot 3: `RewardPreviewMultipleListViewController` (shares base Awake)
static REWARD_TARGETS: [RewardTarget; 4] = [
    RewardTarget::new(),
    RewardTarget::new(),
    RewardTarget::new(),
    RewardTarget::new(),
];

/// Original trampoline for `AnimatedRewardsScreenViewController.Awake()`.
static ORIG_ANIMATED_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `AnimatedRewardsScreenViewController.OnDestroy()`.
static ORIG_ANIMATED_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `GenericRewardsScreenViewController.Awake()` (shared by slots 1-3).
static ORIG_BASE_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for the inherited `OnDestroy()` (shared by slots 1-3).
static ORIG_BASE_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ShortcutsManager.InitializeActions()`.
///
/// Stored but never called: suppressing the original prevents Scopely's keyboard shortcuts from being
/// registered, avoiding conflicts with our own hotkey handling.
#[allow(dead_code)]
static ORIG_INITIALIZE_ACTIONS: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking.
static HOOK_INFO: HookInfo = HookInfo::new("Hotkeys");


// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);
type GsharedLifecycleFn = unsafe extern "C" fn(*mut Il2CppObject, *const ());
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

// ---- Reward Awake/OnDestroy hooks -----------------------------------------

/// Awake hook for `AnimatedRewardsScreenViewController` (slot 0, own override).
extern "C" fn hook_animated_awake(this: *mut Il2CppObject) {
    REWARD_TARGETS[0].instance.store(this as *mut (), Relaxed);
    debug!(target: "Hotkeys", "Reward instance tracked (AnimatedRewards)");
    let orig = ORIG_ANIMATED_AWAKE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// OnDestroy hook for `AnimatedRewardsScreenViewController` (slot 0).
extern "C" fn hook_animated_destroy(this: *mut Il2CppObject) {
    let _ = REWARD_TARGETS[0].instance.compare_exchange(
        this as *mut (), std::ptr::null_mut(), Relaxed, Relaxed,
    );
    let orig = ORIG_ANIMATED_DESTROY.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// Awake hook for `GenericRewardsScreenViewController` (shared by slots 1-3).
///
/// Reads the IL2CPP class pointer from the instance at runtime (offset 0) and matches it against the stored
/// class pointers to determine which tracking slot to use.
extern "C" fn hook_base_reward_awake(this: *mut Il2CppObject) {
    let class = unsafe { tracker::read_ptr(this as *const (), 0) };
    for (i, target) in REWARD_TARGETS[1..].iter().enumerate() {
        if target.class.load(Relaxed) == class {
            target.instance.store(this as *mut (), Relaxed);
            debug!(target: "Hotkeys", "Reward instance tracked (base slot {})", i + 1);
            break;
        }
    }
    let orig = ORIG_BASE_AWAKE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// OnDestroy hook for the inherited `ViewController<T>.OnDestroy()` (shared by slots 1-3).
///
/// This is a generic shared (`_gshared`) method with a hidden `MethodInfo*` second parameter.
/// We must forward it to the trampoline, otherwise the original crashes reading generic context.
extern "C" fn hook_base_reward_destroy(this: *mut Il2CppObject, method: *const ()) {
    for target in &REWARD_TARGETS[1..] {
        let _ = target.instance.compare_exchange(
            this as *mut (), std::ptr::null_mut(), Relaxed, Relaxed,
        );
    }
    let orig = ORIG_BASE_DESTROY.load(Relaxed);
    if !orig.is_null() {
        let f: GsharedLifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this, method) };
    }
}

// ---- Reward collection ----------------------------------------------------

/// Find the first active reward screen and collect via `OnCollectClicked()`.
///
/// Iterates over all four tracking slots (one per `GenericRewardsScreenViewController` subclass).
/// The first slot with a non-null, active instance wins.
/// This handles AnimatedRewards, ShipScrapping, FirstTimeSpender, and RewardPreviewMultipleList screens.
fn collect_reward_screen() {
    let is_active_ptr = IS_ACTIVE_FN.load(Relaxed);

    for target in &REWARD_TARGETS {
        let instance = target.instance.load(Relaxed);
        if instance.is_null() {
            continue;
        }
        let instance = instance as *mut Il2CppObject;

        // Check IsActive (from UIBehaviour); skip if not resolved.
        if !is_active_ptr.is_null() {
            let is_active: IsActiveFn = unsafe { std::mem::transmute(is_active_ptr) };
            if !unsafe { is_active(instance) } {
                continue;
            }
        }

        let on_collect_ptr = target.on_collect.load(Relaxed);
        if on_collect_ptr.is_null() {
            continue;
        }

        debug!(target: "Hotkeys", "ESC: collecting reward screen");
        let on_collect: OnCollectFn = unsafe { std::mem::transmute(on_collect_ptr) };
        unsafe { on_collect(instance) };
        return;
    }
}

// ---- Installation ---------------------------------------------------------

/// Install all hotkey-related hooks.
///
/// Hooks Input.GetKeyDownInt for key detection and consumption, tracks all GenericRewardsScreenViewController
/// subclasses for ESC collection, installs spacebar hooks, and hooks ScreenManager.Update() for per-frame key checks.
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

/// Track all `GenericRewardsScreenViewController` subclasses for ESC reward collection.
///
/// Slot 0 (`AnimatedRewardsScreenViewController`) has its own Awake/OnDestroy overrides and is hooked separately.
/// Slots 1-3 share the base class Awake/OnDestroy and use a single hook with runtime class dispatch.
fn install_reward_tracking(api: &Il2CppApi) {
    // Subclass definitions: (slot, assembly, namespace, class name, label).
    let subclasses: [(usize, &str, &str, &str); 4] = [
        (0, "Digit.Prime.Missions.UI", "AnimatedRewardsScreenViewController", "AnimatedRewards"),
        (1, "Digit.Prime.Ships", "ShipScrappingRewardsScreenViewController", "ShipScrapping"),
        (2, "Digit.Prime.SharedFeatures", "FirstTimeSpenderScreenViewController", "FirstTimeSpender"),
        (3, "Digit.Prime.SharedFeatures", "RewardPreviewMultipleListViewController", "RewardPreview"),
    ];

    // Resolve each subclass: store its Il2CppClass pointer and OnCollectClicked.
    for &(slot, ns, name, label) in &subclasses {
        let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", ns, name) else {
            warn!(target: "Hotkeys", "{name} not found");
            continue;
        };
        REWARD_TARGETS[slot].class.store(class as *mut (), Relaxed);

        if let Some(ptr) = tracker::resolve_fn(api, class, "OnCollectClicked", 0) {
            REWARD_TARGETS[slot].on_collect.store(ptr as *mut (), Relaxed);
            debug!(target: "Hotkeys", "{label}: OnCollectClicked resolved");
        } else {
            warn!(target: "Hotkeys", "{label}: OnCollectClicked not found");
        }
    }

    // Resolve IsActive from UIBehaviour (shared, only need it once from any resolved class).
    for target in &REWARD_TARGETS {
        let class = target.class.load(Relaxed);
        if !class.is_null() {
            if let Some(ptr) = tracker::resolve_fn(
                api, class as *mut Il2CppClass, "IsActive", 0,
            ) {
                IS_ACTIVE_FN.store(ptr as *mut (), Relaxed);
                debug!(target: "Hotkeys", "IsActive resolved (from UIBehaviour)");
                break;
            }
        }
    }

    // Hook AnimatedRewardsScreenViewController.Awake/OnDestroy (slot 0, own overrides).
    let animated_class = REWARD_TARGETS[0].class.load(Relaxed);
    if !animated_class.is_null() {
        let class = animated_class as *mut Il2CppClass;
        if let Some(ptr) = tracker::resolve_fn(api, class, "Awake", 0) {
            match engine::install_hook("RewardAnimatedAwake", ptr, hook_animated_awake as *const ()) {
                Ok(orig) => {
                    ORIG_ANIMATED_AWAKE.store(orig as *mut (), Relaxed);
                    debug!(target: "Hotkeys", "AnimatedRewards Awake hook installed");
                }
                Err(e) => warn!(target: "Hotkeys", "Failed to hook AnimatedRewards Awake: {e}"),
            }
        }
        if let Some(ptr) = tracker::resolve_fn(api, class, "OnDestroy", 0) {
            match engine::install_hook(
                "RewardAnimatedDestroy", ptr, hook_animated_destroy as *const (),
            ) {
                Ok(orig) => {
                    ORIG_ANIMATED_DESTROY.store(orig as *mut (), Relaxed);
                    debug!(target: "Hotkeys", "AnimatedRewards OnDestroy hook installed");
                }
                Err(e) => warn!(target: "Hotkeys", "Failed to hook AnimatedRewards OnDestroy: {e}"),
            }
        }
    }

    // Hook GenericRewardsScreenViewController.Awake/OnDestroy (shared by slots 1-3).
    let Some(base_class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.SharedFeatures",
        "GenericRewardsScreenViewController",
    ) else {
        warn!(target: "Hotkeys", "GenericRewardsScreenViewController not found");
        return;
    };

    if let Some(ptr) = tracker::resolve_fn(api, base_class, "Awake", 0) {
        match engine::install_hook("RewardBaseAwake", ptr, hook_base_reward_awake as *const ()) {
            Ok(orig) => {
                ORIG_BASE_AWAKE.store(orig as *mut (), Relaxed);
                debug!(target: "Hotkeys", "Base reward Awake hook installed");
            }
            Err(e) => warn!(target: "Hotkeys", "Failed to hook base reward Awake: {e}"),
        }
    }
    if let Some(ptr) = tracker::resolve_fn(api, base_class, "OnDestroy", 0) {
        match engine::install_hook(
            "RewardBaseDestroy", ptr, hook_base_reward_destroy as *const (),
        ) {
            Ok(orig) => {
                ORIG_BASE_DESTROY.store(orig as *mut (), Relaxed);
                debug!(target: "Hotkeys", "Base reward OnDestroy hook installed");
            }
            Err(e) => warn!(target: "Hotkeys", "Failed to hook base reward OnDestroy: {e}"),
        }
    }
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
    use std::sync::Mutex;

    /// Serialize tests that mutate global reward state.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Reset all reward tracking slots to null.
    fn reset_reward_targets() {
        for target in &REWARD_TARGETS {
            target.class.store(std::ptr::null_mut(), Relaxed);
            target.instance.store(std::ptr::null_mut(), Relaxed);
            target.on_collect.store(std::ptr::null_mut(), Relaxed);
        }
        IS_ACTIVE_FN.store(std::ptr::null_mut(), Relaxed);
    }

    #[test]
    fn keycode_escape_is_27() {
        assert_eq!(KEYCODE_ESCAPE, 27);
    }

    #[test]
    fn keycode_space_is_32() {
        assert_eq!(KEYCODE_SPACE, 32);
    }

    #[test]
    fn key_down_returns_false_when_fn_not_resolved() {
        assert!(!key_down(KEYCODE_ESCAPE));
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

    #[test]
    fn collect_is_noop_without_instance() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();
        // Should not panic when all slots are empty.
        collect_reward_screen();
    }

    #[test]
    fn animated_awake_stores_in_slot_0() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let fake_instance = 0xDEAD_0000usize as *mut Il2CppObject;
        REWARD_TARGETS[0].instance.store(std::ptr::null_mut(), Relaxed);
        hook_animated_awake(fake_instance);
        assert_eq!(REWARD_TARGETS[0].instance.load(Relaxed), fake_instance as *mut ());

        reset_reward_targets();
    }

    #[test]
    fn animated_destroy_clears_matching_slot_0() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let fake = 0xDEAD_0001usize as *mut ();
        REWARD_TARGETS[0].instance.store(fake, Relaxed);
        hook_animated_destroy(fake as *mut Il2CppObject);
        assert!(REWARD_TARGETS[0].instance.load(Relaxed).is_null());

        reset_reward_targets();
    }

    #[test]
    fn animated_destroy_ignores_non_matching() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let tracked = 0xDEAD_0002usize as *mut ();
        let other = 0xDEAD_0003usize as *mut ();
        REWARD_TARGETS[0].instance.store(tracked, Relaxed);
        hook_animated_destroy(other as *mut Il2CppObject);
        assert_eq!(REWARD_TARGETS[0].instance.load(Relaxed), tracked);

        reset_reward_targets();
    }

    #[test]
    fn base_awake_dispatches_to_correct_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        // Build a fake IL2CPP object: class pointer at offset 0.
        let fake_class = 0xC1A5_0001usize as *mut ();
        let fake_obj: [*mut (); 1] = [fake_class];
        let fake_ptr = fake_obj.as_ptr() as *mut Il2CppObject;

        // Register the class in slot 2 (index 1 in the 1.. slice = slot 2 overall).
        REWARD_TARGETS[2].class.store(fake_class, Relaxed);

        hook_base_reward_awake(fake_ptr);
        assert_eq!(REWARD_TARGETS[2].instance.load(Relaxed), fake_ptr as *mut ());
        // Other slots untouched.
        assert!(REWARD_TARGETS[0].instance.load(Relaxed).is_null());
        assert!(REWARD_TARGETS[1].instance.load(Relaxed).is_null());
        assert!(REWARD_TARGETS[3].instance.load(Relaxed).is_null());

        reset_reward_targets();
    }

    #[test]
    fn base_awake_ignores_unknown_class() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let unknown_class = 0xC1A5_FFFFusize as *mut ();
        let fake_obj: [*mut (); 1] = [unknown_class];
        let fake_ptr = fake_obj.as_ptr() as *mut Il2CppObject;

        hook_base_reward_awake(fake_ptr);
        for target in &REWARD_TARGETS {
            assert!(target.instance.load(Relaxed).is_null());
        }

        reset_reward_targets();
    }

    #[test]
    fn base_destroy_clears_matching_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let fake = 0xDEAD_0004usize as *mut ();
        REWARD_TARGETS[1].instance.store(fake, Relaxed);
        REWARD_TARGETS[2].instance.store(fake, Relaxed);

        hook_base_reward_destroy(fake as *mut Il2CppObject, std::ptr::null());
        assert!(REWARD_TARGETS[1].instance.load(Relaxed).is_null());
        assert!(REWARD_TARGETS[2].instance.load(Relaxed).is_null());
        // Slot 0 is not touched by base destroy.
        assert!(REWARD_TARGETS[0].instance.load(Relaxed).is_null());

        reset_reward_targets();
    }

    #[test]
    fn collect_skips_slot_without_on_collect() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        // Instance present but no on_collect resolved.
        let fake = 0xDEAD_0005usize as *mut ();
        REWARD_TARGETS[0].instance.store(fake, Relaxed);
        // on_collect is null, should skip without crash.
        collect_reward_screen();

        reset_reward_targets();
    }

    // ---- Behavioral tests -------------------------------------------------

    /// Track which slot's on_collect was called.
    static COLLECT_CALLED_SLOT: AtomicI32 = AtomicI32::new(-1);

    extern "C" fn fake_collect_0(_: *mut Il2CppObject) {
        COLLECT_CALLED_SLOT.store(0, Relaxed);
    }

    extern "C" fn fake_collect_2(_: *mut Il2CppObject) {
        COLLECT_CALLED_SLOT.store(2, Relaxed);
    }

    use std::sync::atomic::AtomicI32;

    #[test]
    fn collect_picks_first_active_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();
        COLLECT_CALLED_SLOT.store(-1, Relaxed);

        // Slot 0 and 2 both have instances and on_collect. Slot 0 should win.
        let inst_0 = 0xDEAD_1000usize as *mut ();
        let inst_2 = 0xDEAD_2000usize as *mut ();
        REWARD_TARGETS[0].instance.store(inst_0, Relaxed);
        REWARD_TARGETS[0].on_collect.store(fake_collect_0 as *mut (), Relaxed);
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2].on_collect.store(fake_collect_2 as *mut (), Relaxed);

        collect_reward_screen();
        assert_eq!(COLLECT_CALLED_SLOT.load(Relaxed), 0);

        reset_reward_targets();
    }

    #[test]
    fn collect_skips_to_next_slot_when_first_empty() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();
        COLLECT_CALLED_SLOT.store(-1, Relaxed);

        // Slot 0 empty, slot 2 populated. Slot 2 should be called.
        let inst_2 = 0xDEAD_3000usize as *mut ();
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2].on_collect.store(fake_collect_2 as *mut (), Relaxed);

        collect_reward_screen();
        assert_eq!(COLLECT_CALLED_SLOT.load(Relaxed), 2);

        reset_reward_targets();
    }

    extern "C" fn fake_is_active_false(_: *mut Il2CppObject) -> bool {
        false
    }

    #[test]
    fn collect_skips_inactive_instance() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();
        COLLECT_CALLED_SLOT.store(-1, Relaxed);

        // Slot 0 has instance + on_collect, but IsActive returns false.
        // Slot 2 has instance + on_collect and no IsActive gate (should be called).
        IS_ACTIVE_FN.store(fake_is_active_false as *mut (), Relaxed);
        let inst_0 = 0xDEAD_4000usize as *mut ();
        let inst_2 = 0xDEAD_5000usize as *mut ();
        REWARD_TARGETS[0].instance.store(inst_0, Relaxed);
        REWARD_TARGETS[0].on_collect.store(fake_collect_0 as *mut (), Relaxed);
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2].on_collect.store(fake_collect_2 as *mut (), Relaxed);

        collect_reward_screen();
        // IsActive is global, returns false for ALL instances. Neither should be called.
        assert_eq!(COLLECT_CALLED_SLOT.load(Relaxed), -1);

        reset_reward_targets();
    }
}
