//! Spacebar default-action hook.
//!
//! Checks per frame (from the shared `ScreenManager.Update()` hook) whether SPACE was pressed.
//! If a viewer widget is active, executes the primary action: Engage (ships), Mine (nodes), or Warp (star systems).

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::{debug, warn};

use crate::hook::engine;
use crate::hooks::tracker::{self, instance_tracker};
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Field offsets (from IL2CPP dump v145) --------------------------------

/// `ObjectViewerBaseWidget._visibilityController` (inherited by all viewers).
const OFFSET_VIS_CTRL: usize = 0x80;

/// `VisibilityController._state`.
const OFFSET_VIS_STATE: usize = 0x54;

/// `PreScanTargetWidget._scanEngageButtonsWidget`.
const OFFSET_SCAN_ENGAGE: usize = 0x118;

/// `VisibilityState.Visible` enum value.
const VIS_VISIBLE: i32 = 4;

/// `VisibilityState.Show` enum value (animating to visible).
const VIS_SHOW: i32 = 1;

// ---- Instance trackers (generated) ----------------------------------------

instance_tracker!(prescan);
instance_tracker!(mining);
instance_tracker!(starnode);

// ---- State ----------------------------------------------------------------

/// Original OnDestroy trampoline for the shared viewer destroy hook.
static ORIG_VIEWER_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `ScanEngageButtonsWidget.OnEngageButtonClicked()`.
static ON_ENGAGE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `MiningObjectViewerWidget.MineClicked()`.
static MINE_CLICKED_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `StarNodeObjectViewerWidget.InitiateWarp()`.
static INITIATE_WARP_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the first spacebar action has been logged.
static LOGGED_FIRST_ACTION: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type ActionFn = unsafe extern "C" fn(*mut Il2CppObject);
type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Shared OnDestroy hook ------------------------------------------------

/// Shared OnDestroy hook for all ObjectViewerBaseWidget subclasses.
///
/// Some viewer subclasses (e.g. Mining, StarNode) share the same inherited OnDestroy, so hooking it per-class
/// causes double-hook errors. This single hook checks all trackers and clears any match.
extern "C" fn hook_viewer_destroy(this: *mut Il2CppObject) {
    prescan::clear_if_match(this as *mut ());
    mining::clear_if_match(this as *mut ());
    starnode::clear_if_match(this as *mut ());
    let orig: LifecycleFn =
        unsafe { std::mem::transmute(ORIG_VIEWER_DESTROY.load(Relaxed)) };
    unsafe { orig(this) };
}

// ---- Visibility check -----------------------------------------------------

/// Check whether a viewer widget (ObjectViewerBaseWidget subclass) is visible.
///
/// Reads `_visibilityController` -> `_state` and accepts both `Visible` (fully shown) and
/// `Show` (animation in progress).
/// This matches the `IsShownOrShowing` property on `VisibilityController`.
unsafe fn is_viewer_visible(instance: *const ()) -> bool {
    let vis_ctrl = unsafe { tracker::read_ptr(instance, OFFSET_VIS_CTRL) };
    if vis_ctrl.is_null() {
        return false;
    }
    let state = unsafe { tracker::read_i32(vis_ctrl as *const (), OFFSET_VIS_STATE) };
    state == VIS_VISIBLE || state == VIS_SHOW
}

// ---- Action execution -----------------------------------------------------

/// Called from `hotkeys::hook_update()` when Space is pressed and no input field is focused.
///
/// Checks viewers in priority order and executes the primary action:
/// 1. PreScan (engage target)
/// 2. Mining (mine node)
/// 3. StarNode (initiate warp)
///
/// Returns `true` if an action was executed (the key should be consumed).
pub fn check() -> bool {
    let p = prescan::get();
    if !p.is_null()
        && unsafe { is_viewer_visible(p) }
        && try_engage(p)
    {
        return true;
    }

    let m = mining::get();
    if !m.is_null()
        && unsafe { is_viewer_visible(m) }
        && try_mine(m)
    {
        return true;
    }

    let s = starnode::get();
    if !s.is_null() && unsafe { is_viewer_visible(s) } {
        return try_warp(s);
    }

    false
}

/// Read `_scanEngageButtonsWidget` from the PreScanTargetWidget and call `OnEngageButtonClicked()` on it.
fn try_engage(prescan: *mut ()) -> bool {
    let fn_ptr = ON_ENGAGE_FN.load(Relaxed);
    if fn_ptr.is_null() {
        return false;
    }
    let widget = unsafe { tracker::read_ptr(prescan, OFFSET_SCAN_ENGAGE) };
    if widget.is_null() {
        return false;
    }
    log_first("engaging target");
    let on_engage: ActionFn = unsafe { std::mem::transmute(fn_ptr) };
    unsafe { on_engage(widget as *mut Il2CppObject) };
    true
}

/// Call `MineClicked()` on the MiningObjectViewerWidget.
fn try_mine(mining: *mut ()) -> bool {
    let fn_ptr = MINE_CLICKED_FN.load(Relaxed);
    if fn_ptr.is_null() {
        return false;
    }
    log_first("mining node");
    let mine: ActionFn = unsafe { std::mem::transmute(fn_ptr) };
    unsafe { mine(mining as *mut Il2CppObject) };
    true
}

/// Call `InitiateWarp()` on the StarNodeObjectViewerWidget.
fn try_warp(starnode: *mut ()) -> bool {
    let fn_ptr = INITIATE_WARP_FN.load(Relaxed);
    if fn_ptr.is_null() {
        return false;
    }
    log_first("initiating warp");
    let warp: ActionFn = unsafe { std::mem::transmute(fn_ptr) };
    unsafe { warp(starnode as *mut Il2CppObject) };
    true
}

/// Log the first spacebar action (once per session).
fn log_first(action: &str) {
    if !LOGGED_FIRST_ACTION.swap(true, Relaxed) {
        debug!(target: "Hotkeys", "SPACE: {action}");
    }
}

// ---- Installation ---------------------------------------------------------

/// Install all spacebar-related hooks.
///
/// Resolves viewer classes, hooks Awake/OnDestroy for instance tracking, and resolves action methods.
/// Called from `hotkeys::install()`.
pub fn install(api: &Il2CppApi) {
    // PreScanTargetWidget has its own OnDestroy override, so full install is safe.
    if let Some(c) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.Combat", "PreScanTargetWidget",
    ) {
        prescan::install(api, c, "PreScan");
    }

    // ScanEngageButtonsWidget.OnEngageButtonClicked (no tracking needed, reached via
    // PreScanTargetWidget._scanEngageButtonsWidget field).
    if let Some(c) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.Combat", "ScanEngageButtonsWidget",
    ) {
        if let Some(p) = tracker::resolve_fn(api, c, "OnEngageButtonClicked", 0) {
            ON_ENGAGE_FN.store(p as *mut (), Relaxed);
            debug!(target: "Hotkeys", "OnEngageButtonClicked resolved");
        } else {
            warn!(target: "Hotkeys", "OnEngageButtonClicked not found");
        }
    }

    // MiningObjectViewerWidget and StarNodeObjectViewerWidget share the base class OnDestroy,
    // so we hook Awake individually and OnDestroy once via the shared viewer hook.
    if let Some(c) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.ObjectViewer", "MiningObjectViewerWidget",
    ) {
        mining::install_awake(api, c, "Mining");
        install_shared_destroy(api, c);
        if let Some(p) = tracker::resolve_fn(api, c, "MineClicked", 0) {
            MINE_CLICKED_FN.store(p as *mut (), Relaxed);
            debug!(target: "Hotkeys", "MineClicked resolved");
        } else {
            warn!(target: "Hotkeys", "MineClicked not found");
        }
    }

    if let Some(c) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.ObjectViewer", "StarNodeObjectViewerWidget",
    ) {
        starnode::install_awake(api, c, "StarNode");
        if let Some(p) = tracker::resolve_fn(api, c, "InitiateWarp", 0) {
            INITIATE_WARP_FN.store(p as *mut (), Relaxed);
            debug!(target: "Hotkeys", "InitiateWarp resolved");
        } else {
            warn!(target: "Hotkeys", "InitiateWarp not found");
        }
    }
}

/// Install the shared OnDestroy hook for ObjectViewerBaseWidget subclasses.
///
/// Only installs once (idempotent). Resolves OnDestroy from the given class, which for non-overriding
/// subclasses points to the base class implementation.
fn install_shared_destroy(api: &Il2CppApi, class: *mut Il2CppClass) {
    if !ORIG_VIEWER_DESTROY.load(Relaxed).is_null() {
        return; // Already installed.
    }
    let Some(ptr) = tracker::resolve_fn(api, class, "OnDestroy", 0) else {
        return;
    };
    match engine::install_hook("ViewerDestroy", ptr, hook_viewer_destroy as *const ()) {
        Ok(orig) => {
            ORIG_VIEWER_DESTROY.store(orig as *mut (), Relaxed);
            debug!(target: "HookEngine", "Shared viewer OnDestroy hook installed");
        }
        Err(e) => warn!(target: "HookEngine", "Failed to hook viewer OnDestroy: {e}"),
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_returns_false_without_instances() {
        assert!(!check());
    }

    #[test]
    fn try_engage_returns_false_without_fn() {
        assert!(!try_engage(std::ptr::null_mut()));
    }

    #[test]
    fn try_mine_returns_false_without_fn() {
        assert!(!try_mine(std::ptr::null_mut()));
    }

    #[test]
    fn try_warp_returns_false_without_fn() {
        assert!(!try_warp(std::ptr::null_mut()));
    }
}
