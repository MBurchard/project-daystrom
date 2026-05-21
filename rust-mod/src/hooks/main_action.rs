//! Main action hook.
//!
//! Called from the shared `ScreenManager.Update()` hook when the configured main action key is pressed.
//! If a viewer widget is active, executes the primary action: Engage (ships), Mine (nodes), or Warp (star systems).

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::{debug, warn};

use crate::hooks::tracker::{self, instance_tracker};
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Dynamically resolved field offsets -----------------------------------

/// `ObjectViewerBaseWidget._visibilityController` (inherited by all viewers).
static OFFSET_VIS_CTRL: AtomicUsize = AtomicUsize::new(0);

/// `VisibilityController._state`.
static OFFSET_VIS_STATE: AtomicUsize = AtomicUsize::new(0);

/// `PreScanTargetWidget._scanEngageButtonsWidget`.
static OFFSET_SCAN_ENGAGE: AtomicUsize = AtomicUsize::new(0);

/// `PreScanTargetWidget._addToQueueButtonWidget`.
static OFFSET_QUEUE_BUTTON: AtomicUsize = AtomicUsize::new(0);

/// `ScanEngageButtonsWidget._engageButton` (GenericButtonWidget).
static OFFSET_ENGAGE_BUTTON: AtomicUsize = AtomicUsize::new(0);

/// `VisibilityState.Visible` enum value.
const VIS_VISIBLE: i32 = 4;

/// `VisibilityState.Show` enum value (animating to visible).
const VIS_SHOW: i32 = 1;

// ---- Instance trackers (generated) ----------------------------------------

instance_tracker!(prescan);
instance_tracker!(mining);
instance_tracker!(starnode);
instance_tracker!(nav_controller);

// ---- PreScan station tracking (manual, class-dispatched) ------------------

/// Tracked instance of `PreScanStationTargetWidget` (player bases).
///
/// Separate from the `prescan` tracker because both widget types share the same inherited `Awake()`.
/// Without splitting, clicking a station overwrites the prescan tracker and breaks Space for ships.
static STATION_INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved the IL2CPP class pointer for `PreScanStationTargetWidget` (used for runtime dispatch in Awake hook).
static STATION_CLASS: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `PreScanTargetWidget.Awake()` (shared by both widget types).
static ORIG_PRESCAN_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `PreScanTargetWidget.OnDestroy()` (shared by both widget types).
static ORIG_PRESCAN_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ---- State ----------------------------------------------------------------

/// Original OnDestroy trampoline for the shared viewer destroy hook.
static ORIG_VIEWER_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `ScanEngageButtonsWidget.OnEngageButtonClicked()`.
static ON_ENGAGE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `PreScanTargetWidget.OnAddToQueueClickedEventHandler()`.
static ON_QUEUE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `UIBehaviour.IsActive() -> bool`.
static IS_ACTIVE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `GenericButtonWidget.get_Interactable() -> bool`.
static GET_INTERACTABLE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `MiningObjectViewerWidget.MineClicked()`.
static MINE_CLICKED_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `StarNodeObjectViewerWidget.InitiateWarp()`.
static INITIATE_WARP_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `NavigationInteractionUIViewController.OnSetCourseButtonClick()`.
static ON_SET_COURSE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the first main action has been logged.
static LOGGED_FIRST_ACTION: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type LifecycleFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- PreScan Awake/OnDestroy hooks (class-dispatched) ---------------------

/// `Awake` hook for `PreScanTargetWidget` (catches both base and station subclass).
///
/// Reads the IL2CPP class pointer from the object header and dispatches to the correct tracker.
/// `PreScanStationTargetWidget` goes to [`STATION_INSTANCE`], everything else to [`prescan`].
extern "C" fn hook_prescan_awake(this: *mut Il2CppObject) {
    let class = unsafe { tracker::read_ptr(this as *const (), 0) };
    let station_class = STATION_CLASS.load(Relaxed);
    if !station_class.is_null() && class == station_class {
        STATION_INSTANCE.store(this as *mut (), Relaxed);
    } else {
        // Delegate to the macro-generated hook (stores in prescan::INSTANCE).
        // ORIG_AWAKE inside the macro is null, so it only stores, no double-call.
        prescan::hook_awake(this);
    }

    let orig = ORIG_PRESCAN_AWAKE.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

/// OnDestroy hook for `PreScanTargetWidget` (catches both base and station subclass).
///
/// Clears both trackers via compare-exchange (only the matching one changes).
extern "C" fn hook_prescan_destroy(this: *mut Il2CppObject) {
    let ptr = this as *mut ();
    prescan::clear_if_match(ptr);
    let _ = STATION_INSTANCE.compare_exchange(ptr, std::ptr::null_mut(), Relaxed, Relaxed);

    let orig = ORIG_PRESCAN_DESTROY.load(Relaxed);
    if !orig.is_null() {
        let f: LifecycleFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this) };
    }
}

// ---- Shared OnDestroy hook ------------------------------------------------

/// Shared OnDestroy hook for all ObjectViewerBaseWidget subclasses.
///
/// Some viewer subclasses (e.g. Mining, StarNode) share the same inherited OnDestroy, so hooking it per-class
/// causes double-hook errors. This single hook checks all trackers and clears any match.
extern "C" fn hook_viewer_destroy(this: *mut Il2CppObject) {
    let ptr = this as *mut ();
    prescan::clear_if_match(ptr);
    let _ = STATION_INSTANCE.compare_exchange(ptr, std::ptr::null_mut(), Relaxed, Relaxed);
    mining::clear_if_match(ptr);
    starnode::clear_if_match(ptr);

    let orig_ptr = ORIG_VIEWER_DESTROY.load(Relaxed);
    if !orig_ptr.is_null() {
        let orig: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { orig(this) };
    }
}

// ---- Visibility check -----------------------------------------------------

/// Check whether a viewer widget (ObjectViewerBaseWidget subclass) is visible.
///
/// Reads `_visibilityController` -> `_state` and accepts both `Visible` (fully shown) and
/// `Show` (animation in progress).
/// This matches the `IsShownOrShowing` property on `VisibilityController`.
unsafe fn is_viewer_visible(instance: *const ()) -> bool {
    let ctrl_offset = OFFSET_VIS_CTRL.load(Relaxed);
    let state_offset = OFFSET_VIS_STATE.load(Relaxed);
    if ctrl_offset == 0 || state_offset == 0 {
        return false;
    }
    let vis_ctrl = unsafe { tracker::read_ptr(instance, ctrl_offset) };
    if vis_ctrl.is_null() {
        return false;
    }
    let state = unsafe { tracker::read_i32(vis_ctrl as *const (), state_offset) };
    state == VIS_VISIBLE || state == VIS_SHOW
}

// ---- Action execution -----------------------------------------------------

/// Called from `hotkeys::hook_update()` when the main action key is pressed and no input field is focused.
///
/// Checks viewers in priority order and executes the primary action:
/// 1. PreScan (engage target / station)
/// 2. Mining (mine node)
/// 3. StarNode (initiate warp)
/// 4. Fallback: set course (sends fleet to selected target)
///
/// Returns `true` if an action was executed (the key should be consumed).
pub fn check() -> bool {
    let p = prescan::get();
    if !p.is_null() && unsafe { is_viewer_visible(p) } && try_engage(p) {
        return true;
    }

    // Station prescan (player bases) uses the same engage logic (inherited fields).
    let st = STATION_INSTANCE.load(Relaxed);
    if !st.is_null() && unsafe { is_viewer_visible(st) } && try_engage(st) {
        return true;
    }

    let m = mining::get();
    if !m.is_null() && unsafe { is_viewer_visible(m) } && try_mine(m) {
        return true;
    }

    let s = starnode::get();
    if !s.is_null() && unsafe { is_viewer_visible(s) } {
        return try_warp(s);
    }

    // Fallback: no viewer open, but a navigation controller exists. Trigger "Set Course" which sends
    // the selected fleet to the targeted location (works across systems).
    try_set_course()
}

/// Attempt to engage or queue on the PreScanTargetWidget.
///
/// Checks the engage button first (normal attack). If invisible, checks the queue button.
/// Queue is only triggered if the button is both active and interactable (not full).
fn try_engage(prescan: *mut ()) -> bool {
    // Try normal engage: check if the engage button inside ScanEngageButtonsWidget is active.
    if try_normal_engage(prescan) {
        return true;
    }
    // Engage button not available, try queue attack.
    try_queue_attack(prescan)
}

/// Try normal engage via `ScanEngageButtonsWidget.OnEngageButtonClicked()`.
///
/// Reads `_scanEngageButtonsWidget` → `_engageButton` and checks if the button is active (visible).
fn try_normal_engage(prescan: *mut ()) -> bool {
    let method = ON_ENGAGE_METHOD.load(Relaxed);
    if method.is_null() {
        return false;
    }
    let scan_offset = OFFSET_SCAN_ENGAGE.load(Relaxed);
    if scan_offset == 0 {
        return false;
    }
    let scan_widget = unsafe { tracker::read_ptr(prescan, scan_offset) };
    if scan_widget.is_null() {
        return false;
    }

    // Check if the engage button itself is active (visible).
    let btn_offset = OFFSET_ENGAGE_BUTTON.load(Relaxed);
    if btn_offset != 0 {
        let btn = unsafe { tracker::read_ptr(scan_widget, btn_offset) };
        if !btn.is_null() && !is_widget_active(btn) {
            return false; // Engage button exists but is not visible.
        }
    }

    log_first("engaging target");
    invoke::void(
        method,
        scan_widget as *mut Il2CppObject,
        "ScanEngageButtonsWidget.OnEngageButtonClicked",
    )
}

/// Try queue attack via `PreScanTargetWidget.OnAddToQueueClickedEventHandler()`.
///
/// Checks if the queue button is active (visible) and interactable (queue not full).
fn try_queue_attack(prescan: *mut ()) -> bool {
    let method = ON_QUEUE_METHOD.load(Relaxed);
    if method.is_null() {
        return false;
    }
    let btn_offset = OFFSET_QUEUE_BUTTON.load(Relaxed);
    if btn_offset == 0 {
        return false;
    }
    let btn = unsafe { tracker::read_ptr(prescan as *const (), btn_offset) };
    if btn.is_null() || !is_widget_active(btn) {
        return false; // Queue button not visible.
    }
    if !is_button_interactable(btn) {
        return false; // Queue full.
    }

    log_first("queueing attack");
    invoke::void(
        method,
        prescan as *mut Il2CppObject,
        "PreScanTargetWidget.OnAddToQueueClickedEventHandler",
    )
}

/// Check if a widget's GameObject is active (visible) via `UIBehaviour.IsActive()`.
fn is_widget_active(widget: *const ()) -> bool {
    let method = IS_ACTIVE_METHOD.load(Relaxed);
    if method.is_null() {
        return true; // Optimistic if unresolved.
    }
    invoke::bool(method, widget as *mut Il2CppObject, "UIBehaviour.IsActive").unwrap_or(false)
}

/// Check if a GenericButtonWidget is interactable via `get_Interactable()`.
fn is_button_interactable(widget: *const ()) -> bool {
    let method = GET_INTERACTABLE_METHOD.load(Relaxed);
    if method.is_null() {
        return true; // Optimistic if unresolved.
    }
    invoke::bool(method, widget as *mut Il2CppObject, "GenericButtonWidget.get_Interactable").unwrap_or(false)
}

/// Call `MineClicked()` on the MiningObjectViewerWidget.
fn try_mine(mining: *mut ()) -> bool {
    let method = MINE_CLICKED_METHOD.load(Relaxed);
    if method.is_null() {
        return false;
    }
    log_first("mining node");
    invoke::void(method, mining as *mut Il2CppObject, "MiningObjectViewerWidget.MineClicked")
}

/// Call `InitiateWarp()` on the StarNodeObjectViewerWidget.
fn try_warp(starnode: *mut ()) -> bool {
    let method = INITIATE_WARP_METHOD.load(Relaxed);
    if method.is_null() {
        return false;
    }
    log_first("initiating warp");
    invoke::void(method, starnode as *mut Il2CppObject, "StarNodeObjectViewerWidget.InitiateWarp")
}

/// Trigger `OnSetCourseButtonClick()` on the `NavigationInteractionUIViewController`.
///
/// This is the fallback when no viewer widget is open. It sends the selected fleet to the targeted
/// location, which works even when the target is in a different system.
fn try_set_course() -> bool {
    let method = ON_SET_COURSE_METHOD.load(Relaxed);
    if method.is_null() {
        return false;
    }
    let nav = nav_controller::get();
    if nav.is_null() {
        return false;
    }
    log_first("setting course");
    invoke::void(
        method,
        nav as *mut Il2CppObject,
        "NavigationInteractionUIViewController.OnSetCourseButtonClick",
    )
}

/// Log the first main action (once per session).
fn log_first(action: &str) {
    if !LOGGED_FIRST_ACTION.swap(true, Relaxed) {
        debug!(target: "Hotkeys", "Main action: {action}");
    }
}

// ---- Installation ---------------------------------------------------------

/// Install all main action related hooks.
///
/// Resolves viewer classes, hooks Awake/OnDestroy for instance tracking, and resolves action methods.
/// Called from `hotkeys::install()`.
pub fn install(api: &Il2CppApi) {
    // Resolve shared visibility offsets (used by all viewer types).
    // _visibilityController is inherited from ObjectViewerBaseWidget; resolving on any
    // concrete subclass works because IL2CPP traverses the class hierarchy.
    if let Some(c) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "VisibilityController") {
        resolver::resolve_field_offset_into(api, c, "_state", &OFFSET_VIS_STATE);
    }

    // PreScanTargetWidget has its own OnDestroy override, so installing both hooks is safe.
    if let Some(c) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "PreScanTargetWidget") {
        // Resolve _visibilityController offset on a concrete viewer subclass.
        resolver::resolve_field_offset_into(api, c, "_visibilityController", &OFFSET_VIS_CTRL);

        resolver::resolve_field_offset_into(api, c, "_scanEngageButtonsWidget", &OFFSET_SCAN_ENGAGE);

        resolver::resolve_field_offset_into(api, c, "_addToQueueButtonWidget", &OFFSET_QUEUE_BUTTON);

        resolver::resolve_method_into(api, c, "OnAddToQueueClickedEventHandler", 0, &ON_QUEUE_METHOD);

        // Hook Awake/OnDestroy manually with class-dispatching hooks instead of prescan::install().
        // PreScanStationTargetWidget (player bases) inherits these methods, so a single hook pair
        // catches both widget types. The hooks dispatch to separate trackers based on the IL2CPP class.
        tracker::install_resolved_hook(api, c, "Awake", 0, "PreScanAwake", hook_prescan_awake as *const (), |orig| {
            ORIG_PRESCAN_AWAKE.store(orig as *mut (), Relaxed)
        });
        tracker::install_resolved_hook(
            api,
            c,
            "OnDestroy",
            0,
            "PreScanDestroy",
            hook_prescan_destroy as *const (),
            |orig| ORIG_PRESCAN_DESTROY.store(orig as *mut (), Relaxed),
        );
    }

    // Resolve PreScanStationTargetWidget class pointer for runtime dispatch in the Awake hook.
    if let Some(c) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "PreScanStationTargetWidget")
    {
        STATION_CLASS.store(c as *mut (), Relaxed);
        debug!(target: "Hotkeys", "PreScanStationTargetWidget class resolved for dispatch");
    } else {
        warn!(target: "Hotkeys", "PreScanStationTargetWidget class not found, station dispatch disabled");
    }

    // ScanEngageButtonsWidget.OnEngageButtonClicked (no tracking needed, reached via
    // PreScanTargetWidget._scanEngageButtonsWidget field).
    if let Some(c) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "ScanEngageButtonsWidget") {
        resolver::resolve_method_into(api, c, "OnEngageButtonClicked", 0, &ON_ENGAGE_METHOD);

        resolver::resolve_method_into(api, c, "IsActive", 0, &IS_ACTIVE_METHOD);

        resolver::resolve_field_offset_into(api, c, "_engageButton", &OFFSET_ENGAGE_BUTTON);
    }

    // GenericButtonWidget.get_Interactable (needed for queue button state check).
    if let Some(c) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "GenericButtonWidget") {
        resolver::resolve_method_into(api, c, "get_Interactable", 0, &GET_INTERACTABLE_METHOD);
    }

    // MiningObjectViewerWidget and StarNodeObjectViewerWidget share the base class OnDestroy,
    // so we hook Awake individually and OnDestroy once via the shared viewer hook.
    if let Some(c) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.ObjectViewer", "MiningObjectViewerWidget")
    {
        mining::install_awake(api, c, "Mining");
        install_shared_destroy(api, c);
        resolver::resolve_method_into(api, c, "MineClicked", 0, &MINE_CLICKED_METHOD);
    }

    if let Some(c) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.ObjectViewer", "StarNodeObjectViewerWidget")
    {
        starnode::install_awake(api, c, "StarNode");
        resolver::resolve_method_into(api, c, "InitiateWarp", 0, &INITIATE_WARP_METHOD);
    }

    // NavigationInteractionUIViewController: fallback "Set Course" when no viewer is open.
    if let Some(c) = resolver::resolve_class(
        api,
        "Assembly-CSharp",
        "Digit.Prime.Navigation",
        "NavigationInteractionUIViewController",
    ) {
        nav_controller::install(api, c, "NavController");
        resolver::resolve_method_into(api, c, "OnSetCourseButtonClick", 0, &ON_SET_COURSE_METHOD);
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
    tracker::install_resolved_hook(
        api,
        class,
        "OnDestroy",
        0,
        "ViewerDestroy",
        hook_viewer_destroy as *const (),
        |orig| ORIG_VIEWER_DESTROY.store(orig as *mut (), Relaxed),
    );
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
    fn try_normal_engage_returns_false_without_fn() {
        assert!(!try_normal_engage(std::ptr::null_mut()));
    }

    #[test]
    fn try_queue_attack_returns_false_without_fn() {
        assert!(!try_queue_attack(std::ptr::null_mut()));
    }

    #[test]
    fn try_mine_returns_false_without_fn() {
        assert!(!try_mine(std::ptr::null_mut()));
    }

    #[test]
    fn try_warp_returns_false_without_fn() {
        assert!(!try_warp(std::ptr::null_mut()));
    }

    #[test]
    fn is_widget_active_optimistic_when_unresolved() {
        // IS_ACTIVE_METHOD is null by default in tests, should return true (optimistic).
        assert!(is_widget_active(std::ptr::null()));
    }

    #[test]
    fn is_button_interactable_optimistic_when_unresolved() {
        // GET_INTERACTABLE_METHOD is null by default in tests, should return true (optimistic).
        assert!(is_button_interactable(std::ptr::null()));
    }
}
