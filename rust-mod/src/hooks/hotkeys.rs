//! Hotkey hooks for quality-of-life keyboard shortcuts.
//!
//! Hooks `ScreenManager.Update()` (per-frame) to intercept key presses.
//! Currently, handles ESC on reward/collect dialogues and delegates the configurable main action key to the
//! `main_action` module for default-action execution.
//!
//! `Input.GetKeyDownInt` is hooked (not just resolved) so that consumed keys can be suppressed for the rest
//! of the frame. This prevents the game's own shortcut system from also reacting to keys we already handled.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
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

/// Whether `Input.GetKeyDownInt` has been hooked successfully.
static GET_KEY_DOWN_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

/// KeyCode consumed by our hotkey system in the current frame. 0 = none.
///
/// Our `GetKeyDownInt` hook returns `false` for this key, preventing the game from also processing it.
static CONSUMED_KEYCODE: AtomicI32 = AtomicI32::new(0);

/// Unity KeyCode for the main action shortcut. 0 = disabled. Updated from settings, read per frame.
static MAIN_ACTION_KEYCODE: AtomicI32 = AtomicI32::new(KEYCODE_SPACE);

/// Flag to trigger `LoadBindings()` on the main thread (next frame). Set by ws-client, consumed by hook_update.
static RELOAD_BINDINGS_PENDING: AtomicBool = AtomicBool::new(false);

/// Original trampoline for `ShortcutsManager.InitializeActions()`.
static ORIG_INITIALIZE_ACTIONS: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Tracked `ShortcutsManager` instance (captured in the InitializeActions hook).
static SHORTCUTS_MANAGER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved `ShortcutsManager.LoadBindings()` MethodInfo.
static LOAD_BINDINGS_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `ScreenManager.get_IsInputFocused()` (static).
static IS_INPUT_FOCUSED_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `IsActive() -> bool` (from UIBehaviour, shared by all UI widgets).
pub(super) static IS_ACTIVE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

// ---- Reward screen tracking -----------------------------------------------

/// Per-subclass tracking slot for `GenericRewardsScreenViewController` descendants.
///
/// Each slot holds the IL2CPP class pointer (for runtime dispatch in the AboutToShow/Hide hooks),
/// the currently tracked instance, and the resolved `OnCollectClicked` method info.
struct RewardTarget {
    class: AtomicPtr<()>,
    instance: AtomicPtr<()>,
    on_collect: AtomicPtr<MethodInfo>,
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
/// - Slot 0: `AnimatedRewardsScreenViewController` (own AboutToShow override)
/// - Slot 1: `ShipScrappingRewardsScreenViewController` (inherits base AboutToShow)
/// - Slot 2: `FirstTimeSpenderScreenViewController` (own AboutToShow override)
/// - Slot 3: `RewardPreviewMultipleListViewController` (inherits base AboutToShow)
static REWARD_TARGETS: [RewardTarget; 4] =
    [RewardTarget::new(), RewardTarget::new(), RewardTarget::new(), RewardTarget::new()];

// ---- Widget collect tracking ------------------------------------------------
//
// Widgets with a single collect button that should be triggerable via ESC. Unlike the reward ViewControllers, these
// are Widget<T> subclasses that persist for the entire session, so we track the instance once and gate collection
// through UIBehaviour.IsActive.

/// Tracked instance of `MissionsNotificationPopoutWidget`.
static MISSIONS_POPOUT_INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved `OnCollectButtonClicked` method info for the missions popout.
static MISSIONS_POPOUT_ON_COLLECT: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `MissionsNotificationPopoutWidget.Awake()`.
static ORIG_MISSIONS_POPOUT_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `AnimatedRewardsScreenViewController.AboutToShow()`.
static ORIG_ANIMATED_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `GenericRewardsScreenViewController.AboutToShow()` (shared by slots 1, 3).
static ORIG_BASE_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `FirstTimeSpenderScreenViewController.AboutToShow()`.
static ORIG_FTS_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `AnimatedRewardsScreenViewController.AboutToHide()`.
static ORIG_ANIMATED_HIDE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `GenericRewardsScreenViewController.AboutToHide()` (shared by slots 1, 3).
static ORIG_BASE_HIDE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `FirstTimeSpenderScreenViewController.AboutToHide()`.
static ORIG_FTS_HIDE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking.
static HOOK_INFO: HookInfo = HookInfo::new("Hotkeys");

// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);
type UpdateFn = unsafe extern "C" fn(*mut Il2CppObject);
type GetKeyDownIntFn = unsafe extern "C" fn(i32) -> bool;

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
    invoke::static_bool(ptr, "ScreenManager.get_IsInputFocused").unwrap_or(false)
}

// ---- ShortcutsManager hook ------------------------------------------------

/// Dynamically resolved offset of `ShortcutsManager._actions` (InputActionAsset).
static OFFSET_ACTIONS: AtomicUsize = AtomicUsize::new(0);

/// A game keybinding entry parsed from the InputActionAsset JSON.
#[derive(Clone, Debug)]
struct GameBinding {
    /// The action name (e.g. "ship_locate").
    action: String,
    /// The binding GUID (needed for writing overrides).
    id: String,
}

/// Default game bindings indexed by keyboard path (e.g. "<Keyboard>/space" → [GameBinding]).
/// Populated once from `InitializeActions` post-hook.
static DEFAULT_BINDINGS: Mutex<Option<HashMap<String, Vec<GameBinding>>>> = Mutex::new(None);

/// Partial serde model for Unity's InputActionAsset JSON.
#[derive(Deserialize)]
struct InputActionAssetJson {
    #[serde(default)]
    maps: Vec<InputActionMap>,
}

/// A single action map (e.g. "General", "UI").
#[derive(Deserialize)]
struct InputActionMap {
    #[serde(default)]
    bindings: Vec<InputBinding>,
}

/// A single binding within a map.
#[derive(Deserialize)]
struct InputBinding {
    #[serde(default)]
    action: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    path: String,
}

/// User keybinding overrides stored in PlayerPrefs ("keybindings" key).
#[derive(Deserialize, Serialize)]
struct KeybindingsOverride {
    #[serde(default)]
    bindings: Vec<OverrideEntry>,
}

/// A single keybinding override entry.
#[derive(Deserialize, Serialize)]
struct OverrideEntry {
    action: String,
    id: String,
    path: String,
}

/// Post-hook for `ShortcutsManager.InitializeActions()`.
///
/// Calls the original first (so all actions are set up), then reads the `_actions` field, and calls
/// `InputActionAsset.ToJson()` to parse and store the default bindings.
extern "C" fn hook_initialize_actions(this: *mut Il2CppObject) {
    let orig = ORIG_INITIALIZE_ACTIONS.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }

    HOOK_INFO.run(|| {
        SHORTCUTS_MANAGER.store(this as *mut (), Relaxed);
        parse_default_bindings(this);
    });
}

/// Read `_actions` from the ShortcutsManager instance, call `ToJson()`, and parse the default bindings.
fn parse_default_bindings(manager: *mut Il2CppObject) {
    let Some(api) = super::il2cpp_init::IL2CPP_API.get() else { return };

    // Read _actions field (InputActionAsset) at known offset.
    let actions_offset = OFFSET_ACTIONS.load(Relaxed);
    if actions_offset == 0 {
        warn!(target: "Hotkeys", "ShortcutsManager._actions offset not resolved");
        return;
    }
    let actions_ptr = unsafe { tracker::read_ptr(manager as *const (), actions_offset) } as *mut Il2CppObject;
    if actions_ptr.is_null() {
        warn!(target: "Hotkeys", "ShortcutsManager._actions is null");
        return;
    }

    // Resolve InputActionAsset.ToJson() and call it.
    let Some(asset_class) =
        resolver::resolve_class(api, "Unity.InputSystem", "UnityEngine.InputSystem", "InputActionAsset")
    else {
        warn!(target: "Hotkeys", "InputActionAsset class not found");
        return;
    };

    let Some(method) = resolver::resolve_method(api, asset_class, "ToJson", 0) else {
        warn!(target: "Hotkeys", "InputActionAsset.ToJson not found");
        return;
    };

    let Some(json) = invoke::string(method, actions_ptr, "InputActionAsset.ToJson") else {
        warn!(target: "Hotkeys", "ToJson() returned invalid string");
        return;
    };

    let Ok(asset) = serde_json::from_str::<InputActionAssetJson>(&json) else {
        warn!(target: "Hotkeys", "Failed to parse InputActionAsset JSON");
        return;
    };

    // Index all keyboard bindings by their path.
    let mut bindings: HashMap<String, Vec<GameBinding>> = HashMap::new();
    for map in &asset.maps {
        for binding in &map.bindings {
            if binding.path.starts_with("<Keyboard>/") && !binding.action.is_empty() {
                bindings.entry(binding.path.clone()).or_default().push(GameBinding {
                    action: binding.action.clone(),
                    id: binding.id.clone(),
                });
            }
        }
    }

    let count = bindings.values().map(|v| v.len()).sum::<usize>();
    info!(target: "Hotkeys", "Parsed {count} default keyboard bindings from InputActionAsset");

    *DEFAULT_BINDINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(bindings);

    // Settings may have arrived before bindings were parsed. Resolve pending conflicts now.
    on_shortcuts_changed();
}

// ---- GetKeyDownInt hook ---------------------------------------------------

/// Hook for `Input.GetKeyDownInt(KeyCode)`.
///
/// If a key has been consumed by our hotkey system in this frame, it returns `false` so the game's own shortcut
/// system does not also react to it.
extern "C" fn hook_get_key_down(key: i32) -> bool {
    if key != 0 && key == CONSUMED_KEYCODE.load(Relaxed) {
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
    CONSUMED_KEYCODE.store(0, Relaxed);

    HOOK_INFO.run(super::main_thread::drain_tasks);

    // Process deferred LoadBindings() on the main thread (safe for IL2CPP calls).
    if RELOAD_BINDINGS_PENDING.swap(false, Relaxed) {
        reload_game_bindings();
    }

    HOOK_INFO.run(|| {
        if key_down(KEYCODE_ESCAPE) && collect_reward_screen() {
            CONSUMED_KEYCODE.store(KEYCODE_ESCAPE, Relaxed);
        }
        let main_kc = MAIN_ACTION_KEYCODE.load(Relaxed);
        if main_kc != 0 && key_down(main_kc) && !is_input_focused() && super::main_action::check() {
            CONSUMED_KEYCODE.store(main_kc, Relaxed);
        }
    });

    // Original update runs AFTER our key processing.
    let orig_ptr = ORIGINAL_UPDATE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: UpdateFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

// ---- Reward AboutToShow hooks ---------------------------------------------

/// Match an instance's IL2CPP class pointer against [`REWARD_TARGETS`] and store it in the matching slot.
///
/// Called from each `AboutToShow` hook to track the instance that is currently being displayed.
fn track_reward_instance(this: *mut Il2CppObject) {
    let class = unsafe { tracker::read_ptr(this as *const (), 0) };
    for (i, target) in REWARD_TARGETS.iter().enumerate() {
        if target.class.load(Relaxed) == class {
            target.instance.store(this as *mut (), Relaxed);
            debug!(target: "Hotkeys", "Reward screen shown (slot {i})");
            return;
        }
    }
}

/// AboutToShow hook for `AnimatedRewardsScreenViewController` (slot 0, own override).
extern "C" fn hook_animated_show(this: *mut Il2CppObject) {
    let orig = ORIG_ANIMATED_SHOW.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
    HOOK_INFO.run(|| track_reward_instance(this));
}

/// AboutToShow hook for `GenericRewardsScreenViewController` (shared by slots 1, 3).
///
/// Catches subclasses that inherit AboutToShow without overriding it (ShipScrapping, RewardPreview).
extern "C" fn hook_base_reward_show(this: *mut Il2CppObject) {
    let orig = ORIG_BASE_SHOW.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
    HOOK_INFO.run(|| track_reward_instance(this));
}

/// AboutToShow hook for `FirstTimeSpenderScreenViewController` (slot 2, own override).
extern "C" fn hook_fts_show(this: *mut Il2CppObject) {
    let orig = ORIG_FTS_SHOW.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
    HOOK_INFO.run(|| track_reward_instance(this));
}

// ---- Reward AboutToHide hooks ---------------------------------------------

/// Clear the tracking slot for a reward screen that is being hidden.
///
/// Matches the instance pointer against [`REWARD_TARGETS`] and nulls the slot. This is the symmetric counterpart to
/// [`track_reward_instance`], ensuring no stale pointers remain after a reward screen leaves the UI.
fn untrack_reward_instance(this: *mut Il2CppObject) {
    let ptr = this as *mut ();
    for (i, target) in REWARD_TARGETS.iter().enumerate() {
        if target.instance.load(Relaxed) == ptr {
            target.instance.store(std::ptr::null_mut(), Relaxed);
            debug!(target: "Hotkeys", "Reward screen hidden (slot {i})");
            return;
        }
    }
}

/// AboutToHide hook for `AnimatedRewardsScreenViewController` (slot 0, own override).
extern "C" fn hook_animated_hide(this: *mut Il2CppObject) {
    HOOK_INFO.run(|| untrack_reward_instance(this));
    let orig = ORIG_ANIMATED_HIDE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// AboutToHide hook for `GenericRewardsScreenViewController` (shared by slots 1, 3).
extern "C" fn hook_base_reward_hide(this: *mut Il2CppObject) {
    HOOK_INFO.run(|| untrack_reward_instance(this));
    let orig = ORIG_BASE_HIDE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// AboutToHide hook for `FirstTimeSpenderScreenViewController` (slot 2, own override).
extern "C" fn hook_fts_hide(this: *mut Il2CppObject) {
    HOOK_INFO.run(|| untrack_reward_instance(this));
    let orig = ORIG_FTS_HIDE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

// ---- Widget Awake hooks ---------------------------------------------------

/// Awake hook for `MissionsNotificationPopoutWidget`.
extern "C" fn hook_missions_popout_awake(this: *mut Il2CppObject) {
    HOOK_INFO.run(|| {
        MISSIONS_POPOUT_INSTANCE.store(this as *mut (), Relaxed);
        debug!(target: "Hotkeys", "MissionsNotificationPopout instance tracked");
    });
    let orig = ORIG_MISSIONS_POPOUT_AWAKE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

// ---- Reward collection ----------------------------------------------------

/// Find the first active collect screen and trigger its collect button.
///
/// Checks reward screen ViewControllers (slots 0-3) and widget-based collect screens (MissionsNotificationPopout).
/// The first active instance wins. Returns `true` only when a collect action was actually invoked.
fn collect_reward_screen() -> bool {
    let is_active_ptr = IS_ACTIVE_FN.load(Relaxed);

    // Helper: check IsActive on an instance. Returns true if unresolved (optimistic).
    let check_active = |instance: *mut Il2CppObject| -> bool {
        if is_active_ptr.is_null() {
            return true;
        }
        invoke_bool_noargs(is_active_ptr, instance, "UIBehaviour.IsActive").unwrap_or(false)
    };

    // Reward screen ViewControllers (slots 0-3).
    for target in &REWARD_TARGETS {
        let instance = target.instance.load(Relaxed);
        if instance.is_null() {
            continue;
        }
        let instance = instance as *mut Il2CppObject;
        if !check_active(instance) {
            continue;
        }

        let on_collect_ptr = target.on_collect.load(Relaxed);
        if on_collect_ptr.is_null() {
            continue;
        }

        debug!(target: "Hotkeys", "ESC: collecting reward screen");
        return invoke_void_noargs(on_collect_ptr, instance, "RewardsScreenViewController.OnCollectClicked");
    }

    // Widget-based collect screens.
    let instance = MISSIONS_POPOUT_INSTANCE.load(Relaxed);
    if !instance.is_null() {
        let instance = instance as *mut Il2CppObject;
        if check_active(instance) {
            let on_collect_ptr = MISSIONS_POPOUT_ON_COLLECT.load(Relaxed);
            if !on_collect_ptr.is_null() {
                debug!(target: "Hotkeys", "ESC: collecting missions popout");
                return invoke_void_noargs(
                    on_collect_ptr,
                    instance,
                    "MissionsNotificationPopoutWidget.OnCollectButtonClicked",
                );
            }
        }
    }
    false
}

#[cfg(not(test))]
fn invoke_bool_noargs(method: *const MethodInfo, instance: *mut Il2CppObject, label: &str) -> Option<bool> {
    invoke::bool(method, instance, label)
}

#[cfg(test)]
fn invoke_bool_noargs(method: *const MethodInfo, instance: *mut Il2CppObject, _label: &str) -> Option<bool> {
    if method.is_null() || instance.is_null() {
        return None;
    }
    let ptr = unsafe { (*method).method_pointer };
    if ptr.is_null() {
        return None;
    }
    let f: unsafe extern "C" fn(*mut Il2CppObject) -> bool = unsafe { std::mem::transmute(ptr) };
    Some(unsafe { f(instance) })
}

#[cfg(not(test))]
fn invoke_void_noargs(method: *const MethodInfo, instance: *mut Il2CppObject, label: &str) -> bool {
    invoke::void(method, instance, label)
}

#[cfg(test)]
fn invoke_void_noargs(method: *const MethodInfo, instance: *mut Il2CppObject, _label: &str) -> bool {
    if method.is_null() || instance.is_null() {
        return false;
    }
    let ptr = unsafe { (*method).method_pointer };
    if ptr.is_null() {
        return false;
    }
    let f: unsafe extern "C" fn(*mut Il2CppObject) = unsafe { std::mem::transmute(ptr) };
    unsafe { f(instance) };
    true
}

// ---- Settings callbacks ---------------------------------------------------

/// Map a browser `event.code` to a Unity Input System path (e.g. "Space" → "<Keyboard>/space").
///
/// Browser codes use physical key positions (US layout), which map directly to Unity's Input System paths.
fn event_code_to_input_path(code: &str) -> Option<String> {
    let suffix = match code {
        "Space" => "space",
        // FIXME: Verify STFC's InputActionAsset path for NumpadEnter before splitting it from Enter.
        "Enter" | "NumpadEnter" => "enter",
        "Tab" => "tab",
        "Backspace" => "backspace",
        "Delete" => "delete",
        "Slash" => "slash",
        "Backslash" => "backslash",
        "Minus" => "minus",
        "Equal" => "equals",
        "Period" => "period",
        "Comma" => "comma",
        "Semicolon" => "semicolon",
        "Quote" => "quote",
        "BracketLeft" => "leftBracket",
        "BracketRight" => "rightBracket",
        "Backquote" => "backquote",
        s if s.starts_with("Key") => return Some(format!("<Keyboard>/{}", &s[3..].to_lowercase())),
        s if s.starts_with("Digit") => return Some(format!("<Keyboard>/{}", &s[5..])),
        s if s.starts_with("Numpad") => return Some(format!("<Keyboard>/numpad{}", &s[6..].to_lowercase())),
        s if s.starts_with("F") && s[1..].parse::<u32>().is_ok() => {
            return Some(format!("<Keyboard>/{}", s.to_lowercase()));
        }
        _ => return None,
    };
    Some(format!("<Keyboard>/{suffix}"))
}

/// Map a browser `event.code` to a Unity KeyCode integer. Returns 0 for unknown codes.
///
/// Browser codes use physical key positions (US layout), Unity KeyCodes follow the same convention.
fn event_code_to_keycode(code: &str) -> i32 {
    match code {
        "Space" => 32,
        // FIXME: Verify STFC/Unity KeyCode for NumpadEnter before splitting it from Enter.
        "Enter" | "NumpadEnter" => 13,
        "Tab" => 9,
        "Backspace" => 8,
        "Delete" => 127,
        "Slash" => 47,
        "Backslash" => 92,
        "Minus" => 45,
        "Equal" => 61,
        "Period" => 46,
        "Comma" => 44,
        "Semicolon" => 59,
        "Quote" => 39,
        "BracketLeft" => 91,
        "BracketRight" => 93,
        "Backquote" => 96,
        s if s.starts_with("Key") && s.len() == 4 => {
            let c = s.as_bytes()[3].to_ascii_lowercase();
            c as i32 // Unity KeyCode.A-Z = 97-122
        }
        s if s.starts_with("Digit") && s.len() == 6 => {
            s.as_bytes()[5] as i32 // Unity KeyCode.Alpha0-9 = 48-57
        }
        s if s.starts_with("F") && s[1..].parse::<u32>().is_ok() => {
            let n: u32 = s[1..].parse().unwrap();
            if (1..=15).contains(&n) { 282 + (n as i32 - 1) } else { 0 } // Unity F1=282..F15=296
        }
        // Mouse buttons: Mouse3-6 = side/extra buttons (Mouse0-2 are left/right/middle, reserved).
        // Unity KeyCode: Mouse0=323, Mouse1=324, ..., Mouse6=329.
        s if s.starts_with("Mouse") && s.len() <= 6 => {
            match s[5..].parse::<u32>() {
                Ok(n @ 3..=6) => 323 + n as i32,
                _ => 0, // Mouse0-2 blocked (game needs them), out of range ignored
            }
        }
        _ => 0,
    }
}

/// Update the main action key when shortcut settings change.
///
/// Called from `settings::apply_sync` and `settings::apply_update` when `game.shortcuts` changes.
/// If the configured key conflicts with a game binding, writes a keybindings override to clear it.
pub fn on_shortcuts_changed() {
    on_shortcuts_changed_value(crate::settings::trigger_main_action());
}

pub(crate) fn on_shortcuts_changed_value(key_name: Option<String>) {
    let keycode = key_name.as_ref().map(|k| event_code_to_keycode(k)).unwrap_or(0);
    MAIN_ACTION_KEYCODE.store(keycode, Relaxed);
    debug!(target: "Hotkeys", "Main action keycode: {keycode}");

    if let Some(name) = &key_name {
        resolve_binding_conflicts(name);
    }
}

/// Check if the given key conflicts with any active game binding and write overrides to clear them.
///
/// Merges default bindings with user overrides from the profile TOML to determine the actual active bindings.
/// Only actions that are *currently* on the conflicting key are cleared.
fn resolve_binding_conflicts(key_name: &str) {
    let Some(input_path) = event_code_to_input_path(key_name) else { return };

    let guard = DEFAULT_BINDINGS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(defaults) = guard.as_ref() else { return };

    // Build the effective binding state: start with defaults, apply user overrides.
    // Key = binding ID, Value = (action, current path).
    let mut effective: HashMap<String, (String, String)> = HashMap::new();
    for (path, bindings) in defaults {
        for b in bindings {
            effective.insert(b.id.clone(), (b.action.clone(), path.clone()));
        }
    }

    // Apply user overrides from the profile TOML.
    if let Some(json) = crate::profile_store::get("keybindings")
        && let Ok(overrides) = serde_json::from_str::<KeybindingsOverride>(&json)
    {
        for entry in &overrides.bindings {
            if let Some(existing) = effective.get_mut(&entry.id) {
                existing.1 = entry.path.clone();
            }
        }
    }

    // Find all bindings that are currently on the conflicting key.
    let conflicts: Vec<(String, String)> = effective
        .iter()
        .filter(|(_, (_, path))| *path == input_path)
        .map(|(id, (action, _))| (id.clone(), action.clone()))
        .collect();

    if conflicts.is_empty() {
        debug!(target: "Hotkeys", "No active binding conflict for {input_path}");
        return;
    }

    // Build the keybindings override JSON to clear all conflicting bindings.
    // Preserve existing overrides and add/update the conflicting ones.
    let mut all_overrides: Vec<OverrideEntry> = Vec::new();

    // Keep existing non-conflicting overrides.
    if let Some(json) = crate::profile_store::get("keybindings")
        && let Ok(existing) = serde_json::from_str::<KeybindingsOverride>(&json)
    {
        for entry in existing.bindings {
            if !conflicts.iter().any(|(id, _)| *id == entry.id) {
                all_overrides.push(entry);
            }
        }
    }

    // Add overrides to clear the conflicting bindings.
    for (id, action) in &conflicts {
        info!(target: "Hotkeys", "Clearing conflicting binding: {action} (was on {input_path})");
        all_overrides.push(OverrideEntry {
            action: action.clone(),
            id: id.clone(),
            path: String::new(),
        });
    }

    let override_json = serde_json::to_string(&KeybindingsOverride { bindings: all_overrides }).unwrap_or_default();
    crate::profile_store::record("keybindings", &override_json);
    // Defer LoadBindings() to the main thread. IL2CPP methods cannot be called from our ws-client thread
    // because it lacks thread-static data (CultureInfo, etc.), which causes a NULL pointer crash.
    RELOAD_BINDINGS_PENDING.store(true, Relaxed);
}

/// Trigger `ShortcutsManager.LoadBindings()` so the game picks up keybinding overrides immediately.
fn reload_game_bindings() {
    let manager = SHORTCUTS_MANAGER.load(Relaxed);
    let method = LOAD_BINDINGS_METHOD.load(Relaxed);
    if manager.is_null() || method.is_null() {
        debug!(target: "Hotkeys", "Cannot reload bindings: manager or method not resolved");
        return;
    }

    if invoke::void(method, manager as *mut Il2CppObject, "ShortcutsManager.LoadBindings") {
        debug!(target: "Hotkeys", "Game bindings reloaded via LoadBindings()");
    } else {
        warn!(target: "Hotkeys", "LoadBindings() failed");
    }
}

// ---- Installation ---------------------------------------------------------

/// Install all hotkey-related hooks.
///
/// Hooks Input.GetKeyDownInt for key detection and consumption, tracks all GenericRewardsScreenViewController
/// subclasses for ESC collection, installs main action hooks, and hooks ScreenManager.Update() for per-frame key checks.
pub fn install(api: &Il2CppApi) {
    install_shortcuts_hook(api);
    if !install_input(api) {
        return;
    }
    install_reward_tracking(api);
    install_widget_collect_tracking(api);
    super::main_action::install(api);
    install_update_hook(api);
}

/// Post-hook `ShortcutsManager.InitializeActions()` to parse default bindings and resolve `LoadBindings()`
/// for runtime keybinding reloads.
fn install_shortcuts_hook(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.GameInput", "ShortcutsManager")
    else {
        warn!(target: "Hotkeys", "ShortcutsManager not found");
        return;
    };

    // Resolve _actions field offset for reading the InputActionAsset in the post-hook.
    if OFFSET_ACTIONS.load(Relaxed) == 0 {
        resolver::resolve_field_offset_into(api, class, "_actions", &OFFSET_ACTIONS);
    }

    // Resolve LoadBindings for runtime reloads after conflict resolution.
    if LOAD_BINDINGS_METHOD.load(Relaxed).is_null() {
        resolver::resolve_method_into(api, class, "LoadBindings", 0, &LOAD_BINDINGS_METHOD);
    }

    if !ORIG_INITIALIZE_ACTIONS.load(Relaxed).is_null() {
        return;
    }

    tracker::install_resolved_hook(
        api,
        class,
        "InitializeActions",
        0,
        "InitializeActions",
        hook_initialize_actions as *const (),
        |orig| ORIG_INITIALIZE_ACTIONS.store(orig as *mut (), Relaxed),
    );
}

/// Hook `Input.GetKeyDownInt(KeyCode)` and resolve `ScreenManager.IsInputFocused`.
///
/// GetKeyDownInt is hooked (not just resolved) so consumed keys can be suppressed for the game's own code.
/// Returns `false` if the hook cannot be installed (remaining hooks would be useless).
fn install_input(api: &Il2CppApi) -> bool {
    if !GET_KEY_DOWN_HOOK_INSTALLED.load(Relaxed) {
        let Some(input_class) = resolver::resolve_class(api, "UnityEngine.InputLegacyModule", "UnityEngine", "Input")
        else {
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
                GET_KEY_DOWN_HOOK_INSTALLED.store(true, Relaxed);
                debug!(target: "Hotkeys", "Input.GetKeyDownInt hooked");
            }
            Err(e) => {
                warn!(target: "Hotkeys", "Failed to hook GetKeyDownInt, falling back to direct call: {e}");
                ORIGINAL_GET_KEY_DOWN.store(ptr as *mut (), Relaxed);
            }
        }
    }

    // IsInputFocused is optional; main action and future keys still work without it.
    if IS_INPUT_FOCUSED_FN.load(Relaxed).is_null()
        && let Some(sm_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager")
    {
        resolver::resolve_method_into(api, sm_class, "get_IsInputFocused", 0, &IS_INPUT_FOCUSED_FN);
    }

    true
}

/// Track all `GenericRewardsScreenViewController` subclasses for ESC reward collection.
///
/// Uses symmetric `AboutToShow`/`AboutToHide` hooks for clean lifetime management: `AboutToShow` sets the
/// tracked instance, `AboutToHide` clears it. This ensures pointers are only held while the screen is
/// actively visible, with no reliance on GC behaviour or object pooling assumptions.
///
/// Three hook pairs cover all subclasses:
/// - `AnimatedRewardsScreenViewController` (slot 0, own override)
/// - `GenericRewardsScreenViewController` (slots 1 + 3, inherited by ShipScrapping and RewardPreview)
/// - `FirstTimeSpenderScreenViewController` (slot 2, own override)
fn install_reward_tracking(api: &Il2CppApi) {
    // Subclass definitions: (slot, namespace, class name).
    let subclasses: [(usize, &str, &str); 4] = [
        (0, "Digit.Prime.Missions.UI", "AnimatedRewardsScreenViewController"),
        (1, "Digit.Prime.Ships", "ShipScrappingRewardsScreenViewController"),
        (2, "Digit.Prime.SharedFeatures", "FirstTimeSpenderScreenViewController"),
        (3, "Digit.Prime.SharedFeatures", "RewardPreviewMultipleListViewController"),
    ];

    // Resolve each subclass: store its Il2CppClass pointer and OnCollectClicked.
    for &(slot, ns, name) in &subclasses {
        let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", ns, name) else {
            warn!(target: "Hotkeys", "{name} not found");
            continue;
        };
        REWARD_TARGETS[slot].class.store(class as *mut (), Relaxed);

        resolver::resolve_method_into(api, class, "OnCollectClicked", 0, &REWARD_TARGETS[slot].on_collect);
    }

    // Resolve IsActive from UIBehaviour (shared, only need it once from any resolved class).
    for target in &REWARD_TARGETS {
        let class = target.class.load(Relaxed);
        if !class.is_null()
            && resolver::resolve_method_into(api, class as *mut Il2CppClass, "IsActive", 0, &IS_ACTIVE_FN)
        {
            break;
        }
    }

    // Hook AnimatedRewardsScreenViewController.AboutToShow/Hide (slot 0, own override).
    let animated_class = REWARD_TARGETS[0].class.load(Relaxed);
    if !animated_class.is_null() {
        let class = animated_class as *mut Il2CppClass;
        install_hook_if_missing(
            api,
            class,
            "AboutToShow",
            "RewardAnimatedShow",
            hook_animated_show,
            &ORIG_ANIMATED_SHOW,
        );
        install_hook_if_missing(
            api,
            class,
            "AboutToHide",
            "RewardAnimatedHide",
            hook_animated_hide,
            &ORIG_ANIMATED_HIDE,
        );
    }

    // Hook GenericRewardsScreenViewController.AboutToShow/Hide (shared by slots 1, 3).
    let Some(base_class) = resolver::resolve_class(
        api,
        "Assembly-CSharp",
        "Digit.Prime.SharedFeatures",
        "GenericRewardsScreenViewController",
    ) else {
        warn!(target: "Hotkeys", "GenericRewardsScreenViewController not found");
        return;
    };

    install_hook_if_missing(
        api,
        base_class,
        "AboutToShow",
        "RewardBaseShow",
        hook_base_reward_show,
        &ORIG_BASE_SHOW,
    );
    install_hook_if_missing(
        api,
        base_class,
        "AboutToHide",
        "RewardBaseHide",
        hook_base_reward_hide,
        &ORIG_BASE_HIDE,
    );

    // Hook FirstTimeSpenderScreenViewController.AboutToShow/Hide (slot 2, own override).
    let fts_class = REWARD_TARGETS[2].class.load(Relaxed);
    if !fts_class.is_null() {
        let class = fts_class as *mut Il2CppClass;
        install_hook_if_missing(api, class, "AboutToShow", "RewardFtsShow", hook_fts_show, &ORIG_FTS_SHOW);
        install_hook_if_missing(api, class, "AboutToHide", "RewardFtsHide", hook_fts_hide, &ORIG_FTS_HIDE);
    }
}

/// Install tracking for widget-based collect screens.
///
/// These are `Widget<T>` subclasses with a single collect button that should be triggerable via ESC.
/// They persist for the session, so `Awake` tracking is enough; collection is gated via `IsActive`.
fn install_widget_collect_tracking(api: &Il2CppApi) {
    // MissionsNotificationPopoutWidget
    let Some(class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "MissionsNotificationPopoutWidget")
    else {
        warn!(target: "Hotkeys", "MissionsNotificationPopoutWidget not found");
        return;
    };

    resolver::resolve_method_into(api, class, "OnCollectButtonClicked", 0, &MISSIONS_POPOUT_ON_COLLECT);

    install_hook_if_missing(
        api,
        class,
        "Awake",
        "MissionsPopoutAwake",
        hook_missions_popout_awake,
        &ORIG_MISSIONS_POPOUT_AWAKE,
    );
}

/// Hook `ScreenManager.Update()` for per-frame key checks.
///
/// Falls back to `LateUpdate()` if `Update` is not found (Update may not appear in the IL2CPP dump if it's
/// compiler-generated).
fn install_update_hook(api: &Il2CppApi) {
    if !ORIGINAL_UPDATE.load(Relaxed).is_null() {
        return;
    }

    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager") else {
        return;
    };

    // Try Update first, fall back to LateUpdate.
    let (name, ptr) = if let Some(p) = tracker::resolve_fn(api, class, "Update", 0) {
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

fn install_hook_if_missing(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    hook_name: &str,
    hook: extern "C" fn(*mut Il2CppObject),
    original: &'static AtomicPtr<()>,
) -> bool {
    if !original.load(Relaxed).is_null() {
        return true;
    }

    tracker::install_resolved_hook(api, class, method_name, 0, hook_name, hook as *const (), |orig| {
        original.store(orig as *mut (), Relaxed)
    })
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that mutate the global reward state.
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
    fn event_code_to_keycode_maps_function_keys() {
        assert_eq!(event_code_to_keycode("F1"), 282);
        assert_eq!(event_code_to_keycode("F2"), 283);
        assert_eq!(event_code_to_keycode("F15"), 296);
        assert_eq!(event_code_to_keycode("F16"), 0);
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
    fn consumed_keycode_starts_empty() {
        assert_eq!(CONSUMED_KEYCODE.load(Relaxed), 0);
    }

    extern "C" fn fake_get_key_down_true(_: i32) -> bool {
        true
    }

    #[test]
    fn hook_get_key_down_suppresses_consumed_key_only() {
        let _lock = TEST_LOCK.lock().unwrap();
        ORIGINAL_GET_KEY_DOWN.store(fake_get_key_down_true as *mut (), Relaxed);
        CONSUMED_KEYCODE.store(KEYCODE_ESCAPE, Relaxed);

        assert!(!hook_get_key_down(KEYCODE_ESCAPE));
        assert!(hook_get_key_down(KEYCODE_SPACE));

        CONSUMED_KEYCODE.store(0, Relaxed);
        ORIGINAL_GET_KEY_DOWN.store(std::ptr::null_mut(), Relaxed);
    }

    #[test]
    fn collect_is_noop_without_instance() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();
        // Should not panic when all slots are empty.
        assert!(!collect_reward_screen());
    }

    #[test]
    fn track_reward_dispatches_to_matching_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        // Build a fake IL2CPP object: class pointer at offset 0.
        let fake_class = 0xC1A5_0001usize as *mut ();
        let fake_obj: [*mut (); 1] = [fake_class];
        let fake_ptr = fake_obj.as_ptr() as *mut Il2CppObject;

        // Register the class in REWARD_TARGETS[2] so track_reward_instance matches this slot.
        REWARD_TARGETS[2].class.store(fake_class, Relaxed);

        track_reward_instance(fake_ptr);
        assert_eq!(REWARD_TARGETS[2].instance.load(Relaxed), fake_ptr as *mut ());
        // Other slots untouched.
        assert!(REWARD_TARGETS[0].instance.load(Relaxed).is_null());
        assert!(REWARD_TARGETS[1].instance.load(Relaxed).is_null());
        assert!(REWARD_TARGETS[3].instance.load(Relaxed).is_null());

        reset_reward_targets();
    }

    #[test]
    fn untrack_clears_matching_slot() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let fake_class = 0xC1A5_0002usize as *mut ();
        let fake_obj: [*mut (); 1] = [fake_class];
        let fake_ptr = fake_obj.as_ptr() as *mut Il2CppObject;

        // Track in slot 1, then untrack.
        REWARD_TARGETS[1].class.store(fake_class, Relaxed);
        track_reward_instance(fake_ptr);
        assert_eq!(REWARD_TARGETS[1].instance.load(Relaxed), fake_ptr as *mut ());

        untrack_reward_instance(fake_ptr);
        assert!(REWARD_TARGETS[1].instance.load(Relaxed).is_null());

        reset_reward_targets();
    }

    #[test]
    fn untrack_ignores_unknown_instance() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        // Store a known instance in slot 0.
        let known = 0xDEAD_A000usize as *mut ();
        REWARD_TARGETS[0].instance.store(known, Relaxed);

        // Untrack a different pointer. Slot 0 should remain untouched.
        let unknown = 0xDEAD_B000usize as *mut Il2CppObject;
        untrack_reward_instance(unknown);
        assert_eq!(REWARD_TARGETS[0].instance.load(Relaxed), known);

        reset_reward_targets();
    }

    #[test]
    fn track_reward_ignores_unknown_class() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        let unknown_class = 0xC1A5_FFFFusize as *mut ();
        let fake_obj: [*mut (); 1] = [unknown_class];
        let fake_ptr = fake_obj.as_ptr() as *mut Il2CppObject;

        track_reward_instance(fake_ptr);
        for target in &REWARD_TARGETS {
            assert!(target.instance.load(Relaxed).is_null());
        }

        reset_reward_targets();
    }

    #[test]
    fn collect_skips_slot_without_on_collect() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset_reward_targets();

        // Instance is present but no on_collect resolved.
        let fake = 0xDEAD_0005usize as *mut ();
        REWARD_TARGETS[0].instance.store(fake, Relaxed);
        // on_collect is null, should skip without crash.
        assert!(!collect_reward_screen());

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
        let collect_0 = MethodInfo {
            method_pointer: fake_collect_0 as *const (),
        };
        let collect_2 = MethodInfo {
            method_pointer: fake_collect_2 as *const (),
        };
        REWARD_TARGETS[0].instance.store(inst_0, Relaxed);
        REWARD_TARGETS[0]
            .on_collect
            .store(&collect_0 as *const MethodInfo as *mut MethodInfo, Relaxed);
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2]
            .on_collect
            .store(&collect_2 as *const MethodInfo as *mut MethodInfo, Relaxed);

        assert!(collect_reward_screen());
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
        let collect_2 = MethodInfo {
            method_pointer: fake_collect_2 as *const (),
        };
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2]
            .on_collect
            .store(&collect_2 as *const MethodInfo as *mut MethodInfo, Relaxed);

        assert!(collect_reward_screen());
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
        let is_active = MethodInfo {
            method_pointer: fake_is_active_false as *const (),
        };
        let collect_0 = MethodInfo {
            method_pointer: fake_collect_0 as *const (),
        };
        let collect_2 = MethodInfo {
            method_pointer: fake_collect_2 as *const (),
        };
        IS_ACTIVE_FN.store(&is_active as *const MethodInfo as *mut MethodInfo, Relaxed);
        let inst_0 = 0xDEAD_4000usize as *mut ();
        let inst_2 = 0xDEAD_5000usize as *mut ();
        REWARD_TARGETS[0].instance.store(inst_0, Relaxed);
        REWARD_TARGETS[0]
            .on_collect
            .store(&collect_0 as *const MethodInfo as *mut MethodInfo, Relaxed);
        REWARD_TARGETS[2].instance.store(inst_2, Relaxed);
        REWARD_TARGETS[2]
            .on_collect
            .store(&collect_2 as *const MethodInfo as *mut MethodInfo, Relaxed);

        assert!(!collect_reward_screen());
        // IsActive is global, returns false for ALL instances. Neither should be called.
        assert_eq!(COLLECT_CALLED_SLOT.load(Relaxed), -1);

        reset_reward_targets();
    }
}
