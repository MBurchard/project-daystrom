//! Auto-open chat sidebar on game start.
//!
//! Hooks `UIFrameManager.ShowSideFrame()` to apply the maximum width after the sidebar opens.
//!
//! Hooks `ChatPreviewController.OnEnable()` to detect when the game's chat UI is ready.
//! When `auto_open_sidebar` is enabled, simulates a click on the side panel button and resizes the sidebar to maximum
//! width (the game clamps to its actual limit).

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::Relaxed};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Constants ------------------------------------------------------------

/// Offset of `_focusedPanel` (ChatChannelCategory, i32) on ChatPreviewController.
const OFFSET_FOCUSED_PANEL: usize = 0x90;

/// `ChatChannelCategory.Alliance` — the default tab for auto-open.
const TAB_ALLIANCE: i32 = 2;

/// Width value passed to ResizeSideFrame during restore. Intentionally larger than any screen, so the game clamps it
/// to its actual maximum.
const RESTORE_WIDTH: f32 = 2000.0;

// ---- State ----------------------------------------------------------------

/// Tracked ChatPreviewController instance (set by the OnEnable hook).
static CHAT_PREVIEW: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved function pointer for `ChatPreviewController.OnSidePanelButtonClicked()`.
static CLICK_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved function pointer for `UIFrameManager.ResizeSideFrame(float)`.
static RESIZE_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ChatPreviewController.OnEnable()`.
static ORIG_CHAT_ENABLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `UIFrameManager.ShowSideFrame()`.
static ORIG_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Pending restore width (f32 bits). Non-zero when a restore is in progress.
///
/// Set before calling `OnSidePanelButtonClicked`, consumed by the `ShowSideFrame` hook.
static PENDING_WIDTH: AtomicU32 = AtomicU32::new(0);

/// Whether restore has been attempted (prevents double restore).
static RESTORED: AtomicBool = AtomicBool::new(false);

/// Whether the OnEnable hook has been logged at least once.
static CHAT_ENABLE_LOGGED: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type VoidFn = unsafe extern "C" fn(*mut Il2CppObject);
type ResizeFn = unsafe extern "C" fn(*mut Il2CppObject, f32);

// ---- Hooks ----------------------------------------------------------------

/// Hook for `UIFrameManager.ShowSideFrame()`.
///
/// Delegates to the original, then applies any pending restore width.
/// The pending width is set by [`try_restore`] before triggering the side panel click.
extern "C" fn hook_show(this: *mut Il2CppObject) {
    let orig_ptr = ORIG_SHOW.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    // Apply pending restore width (set by try_restore before the click).
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let width_bits = PENDING_WIDTH.swap(0, Relaxed);
        if width_bits != 0 {
            let width = f32::from_bits(width_bits);
            let resize_ptr = RESIZE_FN.load(Relaxed);
            if !resize_ptr.is_null() {
                let resize: ResizeFn = unsafe { std::mem::transmute(resize_ptr) };
                unsafe { resize(this, width) };
                debug!(target: "ChatFrame", "Applied sidebar width: {width:.0}");
            }
        }
    }));
}

/// Hook for `ChatPreviewController.OnEnable()`.
///
/// Captures the ChatPreviewController instance and triggers a restore check.
/// The chat preview becoming active is a reliable signal that the game's HUD is loaded and interactive.
extern "C" fn hook_chat_enable(this: *mut Il2CppObject) {
    let orig_ptr = ORIG_CHAT_ENABLE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        CHAT_PREVIEW.store(this, Relaxed);
        if !CHAT_ENABLE_LOGGED.swap(true, Relaxed) {
            debug!(target: "ChatFrame", "ChatPreviewController active");
        }
        try_restore();
    }));
}

// ---- Restore --------------------------------------------------------------

/// Called when settings are synced from Daystrom.
///
/// Triggers a restore check in case the ChatPreviewController is already active, but settings were not available yet
/// at that time.
pub fn on_settings_synced() {
    try_restore();
}

/// Attempt to auto-open the chat sidebar.
///
/// Called from two places to handle either timing scenario:
/// - `hook_chat_enable` — chat UI is ready, settings may not be synced yet
/// - `on_settings_synced` — settings arrived, chat UI may not be ready yet
///
/// On success, triggers `OnSidePanelButtonClicked` to open the chat through the game's normal flow.
/// The width is applied by [`hook_show`] when `ShowSideFrame` fires as a result.
fn try_restore() {
    if RESTORED.load(Relaxed) {
        return;
    }
    if CHAT_PREVIEW.load(Relaxed).is_null() {
        return;
    }
    if !crate::settings::auto_open_sidebar() {
        return;
    }

    RESTORED.store(true, Relaxed);

    let click_ptr = CLICK_FN.load(Relaxed);
    if click_ptr.is_null() {
        debug!(target: "ChatFrame", "Auto-open skipped: OnSidePanelButtonClicked not resolved");
        return;
    }

    PENDING_WIDTH.store(RESTORE_WIDTH.to_bits(), Relaxed);

    // Set _focusedPanel to Alliance (ChatChannelCategory = 2) before the click,
    // so the sidebar opens on the Alliance chat like a manual button press would.
    let chat = CHAT_PREVIEW.load(Relaxed);
    unsafe {
        let ptr = (chat as *mut u8).add(OFFSET_FOCUSED_PANEL) as *mut i32;
        ptr.write(TAB_ALLIANCE);
    }

    let click: VoidFn = unsafe { std::mem::transmute(click_ptr) };
    unsafe { click(chat) };
    debug!(target: "ChatFrame", "Auto-opened chat sidebar (Alliance tab)");
}

// ---- Installation ---------------------------------------------------------

/// Install chat sidebar hooks.
///
/// Hooks `UIFrameManager.ShowSideFrame` (pending width application) and
/// `ChatPreviewController.OnEnable` (restore trigger). Resolves `ResizeSideFrame` and
/// `OnSidePanelButtonClicked` as callable functions.
pub fn install(api: &Il2CppApi) {
    // ---- UIFrameManager ----
    let Some(frame_mgr) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Client.UI", "UIFrameManager",
    ) else {
        log::warn!(target: "ChatFrame", "UIFrameManager not found");
        return;
    };

    install_hook(api, frame_mgr, "ShowSideFrame", 0, hook_show as *const (), &ORIG_SHOW);

    // Resolve ResizeSideFrame (called during restore, not hooked).
    if let Some(ptr) = tracker::resolve_fn(api, frame_mgr, "ResizeSideFrame", 1) {
        RESIZE_FN.store(ptr as *mut (), Relaxed);
        debug!(target: "ChatFrame", "ResizeSideFrame resolved");
    }

    // ---- ChatPreviewController ----
    let Some(chat_class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.Chat", "ChatPreviewController",
    ) else {
        log::warn!(target: "ChatFrame", "ChatPreviewController not found");
        return;
    };

    install_hook(
        api, chat_class, "OnEnable", 0, hook_chat_enable as *const (), &ORIG_CHAT_ENABLE,
    );

    // Resolve OnSidePanelButtonClicked (called during restore, not hooked).
    if let Some(ptr) = tracker::resolve_fn(api, chat_class, "OnSidePanelButtonClicked", 0) {
        CLICK_FN.store(ptr as *mut (), Relaxed);
        debug!(target: "ChatFrame", "OnSidePanelButtonClicked resolved");
    } else {
        log::warn!(target: "ChatFrame", "OnSidePanelButtonClicked not found");
    }
}

/// Install a single hook, logging success or failure.
fn install_hook(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    name: &str,
    param_count: i32,
    hook_fn: *const (),
    original: &AtomicPtr<()>,
) {
    let Some(ptr) = tracker::resolve_fn(api, class, name, param_count) else {
        log::warn!(target: "ChatFrame", "{name} not found");
        return;
    };
    match crate::hook::engine::install_hook(name, ptr, hook_fn) {
        Ok(orig) => {
            original.store(orig as *mut (), Relaxed);
            debug!(target: "ChatFrame", "{name} hook installed");
        }
        Err(e) => log::warn!(target: "ChatFrame", "Failed to hook {name}: {e}"),
    }
}
