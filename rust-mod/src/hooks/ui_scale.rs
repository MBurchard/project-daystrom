use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering::Relaxed};

use log::{debug, error};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// Offset of `m_canvasRootScaler` within `ScreenManager` (from IL2CPP dump).
const M_CANVAS_ROOT_SCALER_OFFSET: usize = 0x28;

/// Offset of `m_ScaleFactor` within `CanvasScaler` (from IL2CPP dump).
const M_SCALE_FACTOR_OFFSET: usize = 0x28;

/// Original function pointer:
/// `void UpdateCanvasRootScaleFactor(ScreenManager* this)`.
static ORIGINAL_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Cached pointer to the root `CanvasScaler` instance.
/// Set on the first hook call, used for live updates from WebSocket.
static CACHED_SCALER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// The game's original `m_ScaleFactor` value (stored as f32 bits).
/// Captured after the original `UpdateCanvasRootScaleFactor()` runs, so it reflects the game's own computation.
/// All our scaling is relative to this.
static ORIGINAL_SCALE_BITS: AtomicU32 = AtomicU32::new(0);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("UiScale");

/// Whether the first-call debug log has been emitted.
static LOGGED_FIRST_CALL: AtomicBool = AtomicBool::new(false);

/// Type alias for the original UpdateCanvasRootScaleFactor signature.
type UpdateFn = unsafe extern "C" fn(*mut Il2CppObject);

/// Read the `m_canvasRootScaler` pointer from a ScreenManager instance.
///
/// # Safety
/// Caller must ensure `this` points to a valid ScreenManager instance.
unsafe fn get_canvas_scaler(this: *mut Il2CppObject) -> *mut Il2CppObject {
    unsafe {
        let ptr = (this as *const u8).add(M_CANVAS_ROOT_SCALER_OFFSET);
        *(ptr as *const *mut Il2CppObject)
    }
}

/// Read the `m_ScaleFactor` field from a CanvasScaler instance.
///
/// # Safety
/// Caller must ensure `scaler` points to a valid CanvasScaler instance.
unsafe fn read_scale_factor(scaler: *mut Il2CppObject) -> f32 {
    unsafe {
        let ptr = (scaler as *const u8).add(M_SCALE_FACTOR_OFFSET) as *const f32;
        ptr.read()
    }
}

/// Write the `m_ScaleFactor` field on a CanvasScaler instance.
///
/// `Handle()` picks up this value on the next frame and applies it to `Canvas.scaleFactor`, so there is no need
/// to call `SetScaleFactor()`.
///
/// # Safety
/// Caller must ensure `scaler` points to a valid CanvasScaler instance.
unsafe fn write_scale_factor(scaler: *mut Il2CppObject, value: f32) {
    unsafe {
        let ptr = (scaler as *mut u8).add(M_SCALE_FACTOR_OFFSET) as *mut f32;
        ptr.write(value);
    }
}

/// Apply the current scale setting to the cached root CanvasScaler.
///
/// Called from the hook (on game-triggered updates) and from
/// [`crate::settings::apply_update`] / [`crate::settings::apply_sync`]
/// for live slider updates via WebSocket.
pub fn apply_current_scale() {
    let scaler = CACHED_SCALER.load(Relaxed) as *mut Il2CppObject;
    if scaler.is_null() {
        return;
    }

    let original_base = f32::from_bits(ORIGINAL_SCALE_BITS.load(Relaxed));
    if original_base <= 0.0 {
        return;
    }

    let scale_pct = crate::settings::get_scale();
    let value = if scale_pct == 100 {
        original_base
    } else {
        original_base * (scale_pct as f32 / 100.0)
    };

    unsafe { write_scale_factor(scaler, value) };
}

/// Hook replacement for `ScreenManager.UpdateCanvasRootScaleFactor()`.
///
/// Calls the original (which computes and sets the game's root canvas scale),
/// caches the scaler pointer and original scale factor, then applies the user's scale multiplier.
/// Only touches the root canvas, leaving other canvases (daily goals, away teams, etc.) untouched.
extern "C" fn hook_update(this: *mut Il2CppObject) {
    let original: UpdateFn = unsafe { std::mem::transmute(ORIGINAL_FN.load(Relaxed)) };

    // Always call the original first so the game's normal scaling runs.
    unsafe { original(this) };

    if !HOOK_INFO.is_active() {
        return;
    }

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let scaler = unsafe { get_canvas_scaler(this) };
        if scaler.is_null() {
            return;
        }

        // Cache pointer and original value for live updates.
        let base = unsafe { read_scale_factor(scaler) };
        CACHED_SCALER.store(scaler as *mut (), Relaxed);
        ORIGINAL_SCALE_BITS.store(base.to_bits(), Relaxed);

        if !LOGGED_FIRST_CALL.swap(true, Relaxed) {
            let scale_pct = crate::settings::get_scale();
            debug!(
                target: "UiScale",
                "Cached original scale: {base:.4}, current setting: {scale_pct}%"
            );
        }

        apply_current_scale();
    }));

    if result.is_err() {
        HOOK_INFO.record_error();
    }
}

/// Install the UI scale hook via IL2CPP reflection.
///
/// Hooks `ScreenManager.UpdateCanvasRootScaleFactor()` to multiply the game's
/// computed root canvas scale factor by the user's scale setting. Only the root
/// canvas is affected.
pub fn install(api: &Il2CppApi) {
    let Some(sm_class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager",
    ) else {
        return;
    };

    let Some(update_method) =
        resolver::resolve_method(api, sm_class, "UpdateCanvasRootScaleFactor", 0)
    else {
        return;
    };
    let update_target = unsafe { (*update_method).method_pointer };

    match engine::install_hook("UiScale", update_target, hook_update as *const ()) {
        Ok(original) => {
            ORIGINAL_FN.store(original as *mut (), Relaxed);
            debug!(
                target: "HookEngine",
                "UiScale hook installed (ScreenManager.UpdateCanvasRootScaleFactor)"
            );
        }
        Err(e) => {
            error!(
                target: "HookEngine",
                "Failed to hook UpdateCanvasRootScaleFactor (UiScale): {e}"
            );
        }
    }
}
