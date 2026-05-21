//! Auto-open chat sidebar on game start.
//!
//! Waits for `ChatService.HandleMessageReceived` to settle before detecting that the server has finished delivering
//! message history. Once no new messages arrive for [`MSG_DEBOUNCE`], the sidebar opens. If no messages arrive
//! at all, a [`FALLBACK_TIMEOUT`] after `AboutToShow` ensures the sidebar still opens.
//!
//! Hooks:
//! - `UIFrameManager.OnEnable` captures the manager instance for sidebar resize.
//! - `ChatPreviewController.AboutToShow` captures the controller for the click simulation.
//! - `ChatPreviewController.Update` checks the debounce each frame.
//! - `ChatService.HandleMessageReceived` tracks when the last server message arrived.

use std::panic::AssertUnwindSafe;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::{invoke, resolver, types::*};

// ---- Constants ------------------------------------------------------------

/// Dynamically resolved offset of `_focusedPanel` (ChatChannelCategory, i32) on ChatPreviewController.
static OFFSET_FOCUSED_PANEL: AtomicUsize = AtomicUsize::new(0);

/// `ChatChannelCategory.Alliance` — the default tab for auto-open.
const TAB_ALLIANCE: i32 = 2;

/// Fallback width when GetMaxSideFrameWidth is not available.
/// Intentionally larger than any screen, so the game clamps it to its actual maximum.
const FALLBACK_WIDTH: f32 = 2000.0;

/// Debounce duration for `HandleMessageReceived`. The sidebar opens once no new messages have arrived for
/// this long, indicating the server has finished delivering message history.
const MSG_DEBOUNCE: Duration = Duration::from_millis(500);

/// Fallback timeout after `AboutToShow`. If no `HandleMessageReceived` fires at all (e.g. chat server
/// unreachable), the sidebar opens after this delay to avoid blocking indefinitely.
const FALLBACK_TIMEOUT: Duration = Duration::from_secs(3);

// ---- State ----------------------------------------------------------------

/// Tracked UIFrameManager instance (set by the OnEnable hook).
static FRAME_MGR: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Tracked ChatPreviewController instance (set by the AboutToShow hook).
static CHAT_PREVIEW: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `ChatPreviewController.OnSidePanelButtonClicked()`.
static CLICK_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `UIFrameManager.ResizeSideFrame(float)`.
static RESIZE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `UIFrameManager.GetMaxSideFrameWidth() -> float`.
static MAX_WIDTH_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `UIFrameManager.OnEnable()`.
static ORIG_MGR_ENABLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ChatPreviewController.AboutToShow()`.
static ORIG_CHAT_SHOW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ChatPreviewController.Update()`.
static ORIG_CHAT_UPDATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original function pointer for `ChatService.HandleMessageReceived(Message)`.
static ORIG_MSG_RECEIVED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether restore has been attempted (prevents double restore).
static RESTORED: AtomicBool = AtomicBool::new(false);

/// Timestamp of the last `HandleMessageReceived` call. Used for debounce detection.
static LAST_MSG_RECEIVED: Mutex<Option<Instant>> = Mutex::new(None);

/// The instant when `AboutToShow` first fired. Used as [`FALLBACK_TIMEOUT`] baseline.
static ABOUT_TO_SHOW_TIME: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether the ChatPreviewController AboutToShow hook has been logged at least once.
static CHAT_SHOW_LOGGED: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type VoidFn = unsafe extern "C" fn(*mut Il2CppObject);
type MsgFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject);

// ---- Hooks ----------------------------------------------------------------

/// Hook for `UIFrameManager.OnEnable()`.
///
/// Captures the UIFrameManager instance for later use by [`try_restore`].
/// UIFrameManager has no Awake/OnDestroy, so OnEnable is the lifecycle entry point.
extern "C" fn hook_mgr_enable(this: *mut Il2CppObject) {
    let orig_ptr = ORIG_MGR_ENABLE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        FRAME_MGR.store(this, Relaxed);
        debug!(target: "ChatFrame", "UIFrameManager instance captured");
        try_restore();
    }));
}

/// Hook for `ChatPreviewController.AboutToShow()`.
///
/// Captures the ChatPreviewController instance. The actual auto-open is triggered by the debounce check in
/// [`hook_chat_update`] once `HandleMessageReceived` has settled.
extern "C" fn hook_chat_about_to_show(this: *mut Il2CppObject) {
    let orig_ptr = ORIG_CHAT_SHOW.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        CHAT_PREVIEW.store(this, Relaxed);
        if !CHAT_SHOW_LOGGED.swap(true, Relaxed) {
            if let Ok(mut guard) = ABOUT_TO_SHOW_TIME.lock() {
                *guard = Some(Instant::now());
            }
            debug!(target: "ChatFrame", "ChatPreviewController ready (AboutToShow)");
        }
    }));
}

/// Hook for `ChatPreviewController.Update()`.
///
/// Runs each frame. Checks the [`MSG_DEBOUNCE`] on `HandleMessageReceived` and triggers the auto-open once
/// messages have settled. After [`RESTORED`] is set, the check is a single atomic load.
extern "C" fn hook_chat_update(this: *mut Il2CppObject) {
    if !RESTORED.load(Relaxed) {
        try_restore();
    }

    let orig_ptr = ORIG_CHAT_UPDATE.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: VoidFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }
}

/// Hook for `ChatService.HandleMessageReceived(Message)`.
///
/// Updates the debounce timestamp on every call. [`try_restore`] waits until [`MSG_DEBOUNCE`] has elapsed since
/// the last call, ensuring the server has finished delivering message history.
extern "C" fn hook_msg_received(this: *mut Il2CppObject, message: *mut Il2CppObject) {
    let orig_ptr = ORIG_MSG_RECEIVED.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: MsgFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this, message) };
    }

    if !RESTORED.load(Relaxed)
        && let Ok(mut guard) = LAST_MSG_RECEIVED.lock()
    {
        *guard = Some(Instant::now());
    }
}

// ---- Restore --------------------------------------------------------------

/// Called when settings are synced from Daystrom.
///
/// Triggers a restore check in case the timer has already elapsed but settings were not available yet.
pub fn on_settings_synced() {
    try_restore();
}

/// Attempt to auto-open the chat sidebar.
///
/// Checks four conditions:
/// - UIFrameManager instance captured
/// - ChatPreviewController instance captured
/// - Message history settled ([`MSG_DEBOUNCE`] elapsed since last `HandleMessageReceived`),
///   OR [`FALLBACK_TIMEOUT`] elapsed since `AboutToShow` (in case no messages arrive at all)
/// - `auto_open_sidebar` setting enabled
///
/// On success, triggers `OnSidePanelButtonClicked` to open the chat through the game's normal flow, then calls
/// `ResizeSideFrame` directly on the captured `UIFrameManager` instance to maximize the width.
fn try_restore() {
    if RESTORED.load(Relaxed) {
        return;
    }
    if CHAT_PREVIEW.load(Relaxed).is_null() {
        return;
    }
    if FRAME_MGR.load(Relaxed).is_null() {
        return;
    }
    let debounce_settled = LAST_MSG_RECEIVED
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|t| t.elapsed() >= MSG_DEBOUNCE);
    let fallback_expired = ABOUT_TO_SHOW_TIME
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .is_some_and(|t| t.elapsed() >= FALLBACK_TIMEOUT);
    if !debounce_settled && !fallback_expired {
        return;
    }
    if !crate::settings::auto_open_sidebar() {
        debug!(target: "ChatFrame", "try_restore: all ready, waiting for settings sync");
        return;
    }

    RESTORED.store(true, Relaxed);

    let click_ptr = CLICK_FN.load(Relaxed);
    if click_ptr.is_null() {
        debug!(target: "ChatFrame", "Auto-open skipped: OnSidePanelButtonClicked not resolved");
        return;
    }

    // Set _focusedPanel to Alliance (ChatChannelCategory = 2) before the click,
    // so the sidebar opens on the Alliance chat like a manual button press would.
    let chat = CHAT_PREVIEW.load(Relaxed);
    let panel_offset = OFFSET_FOCUSED_PANEL.load(Relaxed);
    if panel_offset == 0 {
        debug!(target: "ChatFrame", "Auto-open skipped: _focusedPanel offset not resolved");
        return;
    }
    unsafe {
        let ptr = (chat as *mut u8).add(panel_offset) as *mut i32;
        ptr.write(TAB_ALLIANCE);
    }

    invoke::void(click_ptr, chat, "ChatPreviewController.OnSidePanelButtonClicked");
    debug!(target: "ChatFrame", "Auto-opened chat sidebar (Alliance tab)");

    // Resize directly on the captured UIFrameManager instance.
    let mgr = FRAME_MGR.load(Relaxed);
    let resize_ptr = RESIZE_FN.load(Relaxed);
    if !resize_ptr.is_null() {
        let width = get_max_width(mgr);
        invoke::void_f32(resize_ptr, mgr, width, "UIFrameManager.ResizeSideFrame");
        debug!(target: "ChatFrame", "Applied sidebar width: {width:.0}");
    }
}

/// Query the game for the maximum sidebar width, falling back to [`FALLBACK_WIDTH`].
fn get_max_width(mgr: *mut Il2CppObject) -> f32 {
    let ptr = MAX_WIDTH_FN.load(Relaxed);
    if ptr.is_null() {
        return FALLBACK_WIDTH;
    }
    let width = invoke::f32(ptr, mgr, "UIFrameManager.GetMaxSideFrameWidth").unwrap_or(FALLBACK_WIDTH);
    if width > 0.0 { width } else { FALLBACK_WIDTH }
}

// ---- Installation ---------------------------------------------------------

/// Helper to resolve a method pointer, install a hook, and store the original.
fn install_hook(api: &Il2CppApi, class: *mut Il2CppClass, name: &str, hook_fn: *const (), original: &AtomicPtr<()>) {
    tracker::install_resolved_hook(api, class, name, 0, name, hook_fn, |orig| {
        original.store(orig as *mut (), Relaxed)
    });
}

/// Install chat sidebar hooks.
///
/// Hooks `UIFrameManager.OnEnable` to capture the manager instance and resolves `ResizeSideFrame` as a callable
/// function. On `ChatPreviewController`, hooks `AboutToShow` to capture the instance, hooks `Update` for the
/// debounce check, and resolves `OnSidePanelButtonClicked`. On `ChatService`, hooks `HandleMessageReceived`
/// to track when server messages arrive.
pub fn install(api: &Il2CppApi) {
    // ---- UIFrameManager ----
    let Some(frame_mgr_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "UIFrameManager")
    else {
        log::warn!(target: "ChatFrame", "UIFrameManager not found");
        return;
    };

    install_hook(api, frame_mgr_class, "OnEnable", hook_mgr_enable as *const (), &ORIG_MGR_ENABLE);

    // Resolve ResizeSideFrame and GetMaxSideFrameWidth (called during restore, not hooked).
    resolver::resolve_method_into(api, frame_mgr_class, "ResizeSideFrame", 1, &RESIZE_FN);
    resolver::resolve_method_into(api, frame_mgr_class, "GetMaxSideFrameWidth", 0, &MAX_WIDTH_FN);

    // ---- ChatPreviewController ----
    let Some(chat_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Chat", "ChatPreviewController")
    else {
        log::warn!(target: "ChatFrame", "ChatPreviewController not found");
        return;
    };

    resolver::resolve_field_offset_into(api, chat_class, "_focusedPanel", &OFFSET_FOCUSED_PANEL);

    install_hook(
        api,
        chat_class,
        "AboutToShow",
        hook_chat_about_to_show as *const (),
        &ORIG_CHAT_SHOW,
    );

    install_hook(api, chat_class, "Update", hook_chat_update as *const (), &ORIG_CHAT_UPDATE);

    // Resolve OnSidePanelButtonClicked (called during restore, not hooked).
    resolver::resolve_method_into(api, chat_class, "OnSidePanelButtonClicked", 0, &CLICK_FN);

    // ---- ChatService (different assembly) ----
    let Some(chat_service_class) = resolver::resolve_class(
        api,
        "Digit.Client.PrimeLib.Runtime",
        "Digit.PrimePlatform.Services",
        "ChatService",
    ) else {
        log::warn!(target: "ChatFrame", "ChatService not found");
        return;
    };

    tracker::install_resolved_hook(
        api,
        chat_service_class,
        "HandleMessageReceived",
        1,
        "HandleMessageReceived",
        hook_msg_received as *const (),
        |orig| ORIG_MSG_RECEIVED.store(orig as *mut (), Relaxed),
    );
}
