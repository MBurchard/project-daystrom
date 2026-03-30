//! Auto-expand job queue panel on game start.
//!
//! Hooks `JobQueuePanelViewController.RegenerateLists()` to detect when the job queue UI is ready.
//! When `auto_expand_job_queue` is enabled and the panel is in compact view,
//! simulates a click on the contract/expand button to switch to full view.
//!
//! Note: `OnEnable()` cannot be hooked because it resolves to a generic `ViewController<T>`
//! vtable slot (IL2CPP generic sharing), causing a C++ foreign exception.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Constants ------------------------------------------------------------

/// Offset of `_listCollapsed` (bool) on JobQueuePanelViewController.
const OFFSET_LIST_COLLAPSED: usize = 0x88;

// ---- State ----------------------------------------------------------------

/// Tracked JobQueuePanelViewController instance (set by the RegenerateLists hook).
static JOB_QUEUE_PANEL: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved function pointer for `OnContractExpandButtonClickEventHandler()`.
static EXPAND_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `JobQueuePanelViewController.RegenerateLists()`.
static ORIG_REGENERATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether expand has been attempted (prevents double expand).
static EXPANDED: AtomicBool = AtomicBool::new(false);

/// Whether the RegenerateLists hook has been logged at least once.
static REGENERATE_LOGGED: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type VoidFn = unsafe extern "C" fn(*mut Il2CppObject);

// ---- Hooks ----------------------------------------------------------------

/// Hook for `JobQueuePanelViewController.RegenerateLists()`.
///
/// Captures the controller instance and triggers an expand check.
/// RegenerateLists is called when the panel rebuilds its job list,
/// which reliably indicates the panel is active and ready.
extern "C" fn hook_regenerate_lists(this: *mut Il2CppObject) {
    // Call the original first, before any of our logic.
    let orig_ptr = ORIG_REGENERATE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    // Wrap all our logic in catch_unwind to prevent panics from aborting the game.
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        JOB_QUEUE_PANEL.store(this, Relaxed);
        if !REGENERATE_LOGGED.swap(true, Relaxed) {
            debug!(target: "JobQueue", "JobQueuePanelViewController.RegenerateLists fired");
        }
        try_expand();
    }));
}

// ---- Expand ---------------------------------------------------------------

/// Called when settings are synced from Daystrom.
///
/// Triggers an expand check in case the panel is already active but settings were not available
/// yet at that time.
pub fn on_settings_synced() {
    try_expand();
}

/// Attempt to auto-expand the job queue panel.
///
/// Called from two places to handle either timing scenario:
/// - `hook_regenerate_lists` — panel is ready, settings may not be synced yet
/// - `on_settings_synced` — settings arrived, panel may not be ready yet
///
/// On success, triggers `OnContractExpandButtonClickEventHandler` to expand through the game's
/// normal flow.
fn try_expand() {
    if EXPANDED.load(Relaxed) {
        return;
    }
    let panel = JOB_QUEUE_PANEL.load(Relaxed);
    if panel.is_null() {
        return;
    }
    if !crate::settings::auto_expand_job_queue() {
        return;
    }

    // Only expand if the panel is currently collapsed.
    let collapsed = unsafe {
        let ptr = (panel as *mut u8).add(OFFSET_LIST_COLLAPSED) as *const bool;
        ptr.read()
    };
    if !collapsed {
        debug!(target: "JobQueue", "Panel already expanded, skipping");
        EXPANDED.store(true, Relaxed);
        return;
    }

    let expand_ptr = EXPAND_FN.load(Relaxed);
    if expand_ptr.is_null() {
        debug!(target: "JobQueue", "Auto-expand skipped: OnContractExpandButtonClickEventHandler not resolved");
        return;
    }

    EXPANDED.store(true, Relaxed);

    let expand: VoidFn = unsafe { std::mem::transmute(expand_ptr) };
    unsafe { expand(panel) };
    debug!(target: "JobQueue", "Auto-expanded job queue panel");
}

// ---- Installation ---------------------------------------------------------

/// Install job queue panel hooks.
///
/// Hooks `JobQueuePanelViewController.RegenerateLists` (expand trigger) and resolves
/// `OnContractExpandButtonClickEventHandler` as a callable function.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.HUD", "JobQueuePanelViewController",
    ) else {
        log::warn!(target: "JobQueue", "JobQueuePanelViewController not found");
        return;
    };

    // Hook RegenerateLists: non-virtual, class-specific method that fires when the panel
    // rebuilds its job list. Unlike OnEnable (inherited from the generic ViewController<T>),
    // this resolves to a concrete method pointer and is safe to hook.
    if let Some(ptr) = tracker::resolve_fn(api, class, "RegenerateLists", 0) {
        match crate::hook::engine::install_hook(
            "JobQueue.RegenerateLists", ptr, hook_regenerate_lists as *const (),
        ) {
            Ok(orig) => {
                ORIG_REGENERATE.store(orig as *mut (), Relaxed);
                debug!(target: "JobQueue", "RegenerateLists hook installed");
            }
            Err(e) => log::warn!(target: "JobQueue", "Failed to hook RegenerateLists: {e}"),
        }
    } else {
        log::warn!(target: "JobQueue", "RegenerateLists not found");
    }

    // Resolve OnContractExpandButtonClickEventHandler (called during expand, not hooked).
    if let Some(ptr) = tracker::resolve_fn(api, class, "OnContractExpandButtonClickEventHandler", 0) {
        EXPAND_FN.store(ptr as *mut (), Relaxed);
        debug!(target: "JobQueue", "OnContractExpandButtonClickEventHandler resolved");
    } else {
        log::warn!(target: "JobQueue", "OnContractExpandButtonClickEventHandler not found");
    }
}
