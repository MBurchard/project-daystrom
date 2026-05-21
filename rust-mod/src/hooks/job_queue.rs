//! Auto-expand job queue panel on game start.
//!
//! Hooks `JobQueuePanelViewController.RegenerateLists()` to detect when the job queue UI is ready.
//! When `auto_expand_job_queue` is enabled and the panel is in compact view,
//! simulates a click on the contract/expand button to switch to full view.
//!
//! Note: `OnEnable()` cannot be hooked because it resolves to a generic `ViewController<T>`
//! vtable slot (IL2CPP generic sharing), causing a C++ foreign exception.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::debug;

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Dynamically resolved field offsets -----------------------------------

/// Offset of `_listCollapsed` (bool) on JobQueuePanelViewController.
static OFFSET_LIST_COLLAPSED: AtomicUsize = AtomicUsize::new(0);

// ---- State ----------------------------------------------------------------

/// Tracked JobQueuePanelViewController instance (set by the RegenerateLists hook).
static JOB_QUEUE_PANEL: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `OnContractExpandButtonClickEventHandler()`.
static EXPAND_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `JobQueuePanelViewController.RegenerateLists()`.
static ORIG_REGENERATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether expand has been attempted (prevents double expand).
static EXPANDED: AtomicBool = AtomicBool::new(false);

/// Whether the RegenerateLists hook has been logged at least once.
static REGENERATE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Latest auto-expand setting, updated from the main-thread settings executor.
static AUTO_EXPAND_JOB_QUEUE: AtomicBool = AtomicBool::new(false);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("JobQueue");

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

    HOOK_INFO.run(|| {
        JOB_QUEUE_PANEL.store(this, Relaxed);
        if !REGENERATE_LOGGED.swap(true, Relaxed) {
            debug!(target: "JobQueue", "JobQueuePanelViewController.RegenerateLists fired");
        }
        try_expand();
    });
}

// ---- Expand ---------------------------------------------------------------

pub(crate) fn on_settings_synced_value(auto_expand_job_queue: bool) {
    AUTO_EXPAND_JOB_QUEUE.store(auto_expand_job_queue, Relaxed);
    HOOK_INFO.run(try_expand);
}

/// Attempt to auto-expand the job queue panel.
///
/// Called from two places to handle either timing scenario:
/// - `hook_regenerate_lists` — panel is ready, settings may not be synced yet
/// - `on_settings_synced_value` — settings arrived, panel may not be ready yet
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
    if !AUTO_EXPAND_JOB_QUEUE.load(Relaxed) {
        return;
    }

    // Only expand if the panel is currently collapsed.
    let collapsed_offset = OFFSET_LIST_COLLAPSED.load(Relaxed);
    if collapsed_offset == 0 {
        debug!(target: "JobQueue", "Auto-expand skipped: _listCollapsed offset not resolved");
        return;
    }
    let collapsed = unsafe {
        let ptr = (panel as *mut u8).add(collapsed_offset) as *const bool;
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

    invoke::void(
        expand_ptr,
        panel,
        "JobQueuePanelViewController.OnContractExpandButtonClickEventHandler",
    );
    debug!(target: "JobQueue", "Auto-expanded job queue panel");
}

// ---- Installation ---------------------------------------------------------

/// Install job queue panel hooks.
///
/// Hooks `JobQueuePanelViewController.RegenerateLists` (expand trigger) and resolves
/// `OnContractExpandButtonClickEventHandler` as a guarded callable method.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "JobQueuePanelViewController")
    else {
        log::warn!(target: "JobQueue", "JobQueuePanelViewController not found");
        return;
    };

    // Resolve field offset dynamically via IL2CPP reflection.
    resolver::resolve_field_offset_into(api, class, "_listCollapsed", &OFFSET_LIST_COLLAPSED);

    // Hook RegenerateLists: non-virtual, class-specific method that fires when the panel
    // rebuilds its job list. Unlike OnEnable (inherited from the generic ViewController<T>),
    // this resolves to a concrete method pointer and is safe to hook.
    tracker::install_resolved_hook(
        api,
        class,
        "RegenerateLists",
        0,
        "JobQueue.RegenerateLists",
        hook_regenerate_lists as *const (),
        |orig| ORIG_REGENERATE.store(orig as *mut (), Relaxed),
    );

    // Resolve OnContractExpandButtonClickEventHandler (called during expand, not hooked).
    resolver::resolve_method_into(api, class, "OnContractExpandButtonClickEventHandler", 0, &EXPAND_FN);
}
