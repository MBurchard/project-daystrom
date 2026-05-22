use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering::Relaxed};

use log::{debug, warn};

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// Dynamically resolved offset of `m_canvasRootScaler` within `ScreenManager`.
static OFFSET_ROOT_SCALER: AtomicUsize = AtomicUsize::new(0);

/// Dynamically resolved offset of `m_ScaleFactor` within `CanvasScaler`.
static OFFSET_SCALE_FACTOR: AtomicUsize = AtomicUsize::new(0);

/// Original function pointer:
/// `void UpdateCanvasRootScaleFactor(ScreenManager* this)`.
static ORIGINAL_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Cached pointer to the root `CanvasScaler` instance.
/// Set on the first hook call, used for live updates from WebSocket.
/// FIXME: This cached Unity object can become stale across UI lifecycle changes. Prefer a lifecycle clear or a
/// safe game/Unity API if one is available in the dump.
static CACHED_SCALER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// The game's original `m_ScaleFactor` value (stored as f32 bits).
/// Captured after the original `UpdateCanvasRootScaleFactor()` runs, so it reflects the game's own computation.
/// All our scaling is relative to this.
static ORIGINAL_SCALE_BITS: AtomicU32 = AtomicU32::new(0);

/// Latest UI scale setting, updated from the main-thread settings executor.
static CURRENT_SCALE_PCT: AtomicU32 = AtomicU32::new(100);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("UiScale");

/// Whether the first-call debug log has been emitted.
static LOGGED_FIRST_CALL: AtomicBool = AtomicBool::new(false);

/// Type alias for the original UpdateCanvasRootScaleFactor signature.
type UpdateFn = unsafe extern "C" fn(*mut Il2CppObject);

/// Read the `m_canvasRootScaler` pointer from a ScreenManager instance.
///
/// Returns null if the field offset has not been resolved.
///
/// # Safety
/// Caller must ensure `this` points to a valid ScreenManager instance.
unsafe fn get_canvas_scaler(this: *mut Il2CppObject) -> *mut Il2CppObject {
    let offset = OFFSET_ROOT_SCALER.load(Relaxed);
    if offset == 0 {
        return std::ptr::null_mut();
    }
    unsafe {
        let ptr = (this as *const u8).add(offset);
        *(ptr as *const *mut Il2CppObject)
    }
}

/// Read the `m_ScaleFactor` field from a CanvasScaler instance.
///
/// Returns 0.0 if the field offset has not been resolved.
///
/// # Safety
/// Caller must ensure `scaler` points to a valid CanvasScaler instance.
unsafe fn read_scale_factor(scaler: *mut Il2CppObject) -> f32 {
    let offset = OFFSET_SCALE_FACTOR.load(Relaxed);
    if offset == 0 {
        return 0.0;
    }
    unsafe {
        let ptr = (scaler as *const u8).add(offset) as *const f32;
        ptr.read()
    }
}

/// Write the `m_ScaleFactor` field on a CanvasScaler instance.
///
/// `Handle()` picks up this value on the next frame and applies it to `Canvas.scaleFactor`, so there is no need
/// to call `SetScaleFactor()`. No-op if the field offset has not been resolved.
///
/// # Safety
/// Caller must ensure `scaler` points to a valid CanvasScaler instance.
unsafe fn write_scale_factor(scaler: *mut Il2CppObject, value: f32) {
    // FIXME: Direct field writes cannot be protected from stale object pointers. Re-check whether CanvasScaler
    // exposes a stable setter/API we can invoke without changing the game's scaling semantics.
    let offset = OFFSET_SCALE_FACTOR.load(Relaxed);
    if offset == 0 {
        return;
    }
    unsafe {
        let ptr = (scaler as *mut u8).add(offset) as *mut f32;
        ptr.write(value);
    }
}

/// Apply the current scale setting to the cached root CanvasScaler.
///
/// Called from the hook on game-triggered updates and from the main-thread settings dispatcher for live slider
/// updates.
pub fn apply_current_scale() {
    apply_scale_inner_guarded(CURRENT_SCALE_PCT.load(Relaxed));
}

pub(crate) fn apply_scale(scale_pct: u32) {
    CURRENT_SCALE_PCT.store(scale_pct, Relaxed);
    apply_scale_inner_guarded(scale_pct);
}

fn apply_scale_inner_guarded(scale_pct: u32) {
    HOOK_INFO.run(|| apply_scale_inner(scale_pct));
}

fn apply_scale_inner(scale_pct: u32) {
    let scaler = CACHED_SCALER.load(Relaxed) as *mut Il2CppObject;
    if scaler.is_null() {
        return;
    }

    let original_base = f32::from_bits(ORIGINAL_SCALE_BITS.load(Relaxed));
    if original_base <= 0.0 {
        return;
    }

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
    // Always call the original first so the game's normal scaling runs.
    let orig_ptr = ORIGINAL_FN.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: UpdateFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) };
    }

    HOOK_INFO.run(|| {
        let scaler = unsafe { get_canvas_scaler(this) };
        if scaler.is_null() {
            return;
        }

        // Cache pointer and original value for live updates.
        let base = unsafe { read_scale_factor(scaler) };
        CACHED_SCALER.store(scaler as *mut (), Relaxed);
        ORIGINAL_SCALE_BITS.store(base.to_bits(), Relaxed);

        if !LOGGED_FIRST_CALL.swap(true, Relaxed) {
            let scale_pct = CURRENT_SCALE_PCT.load(Relaxed);
            debug!(
                target: "UiScale",
                "Cached original scale: {base:.4}, current setting: {scale_pct}%"
            );
        }

        apply_current_scale();
    });
}

/// Install the UI scale hook via IL2CPP reflection.
///
/// Hooks `ScreenManager.UpdateCanvasRootScaleFactor()` to multiply the game's
/// computed root canvas scale factor by the user's scale setting. Only the root
/// canvas is affected.
pub fn install(api: &Il2CppApi) {
    let Some(sm_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "ScreenManager") else {
        return;
    };

    // Resolve field offsets dynamically via IL2CPP reflection.
    resolver::resolve_field_offset_into(api, sm_class, "m_canvasRootScaler", &OFFSET_ROOT_SCALER);

    if let Some(cs_class) = resolver::resolve_class(api, "UnityEngine.UI", "UnityEngine.UI", "CanvasScaler") {
        resolver::resolve_field_offset_into(api, cs_class, "m_ScaleFactor", &OFFSET_SCALE_FACTOR);
    } else {
        warn!(target: "UiScale", "CanvasScaler class not found");
    }

    tracker::install_resolved_hook(
        api,
        sm_class,
        "UpdateCanvasRootScaleFactor",
        0,
        "UiScale",
        hook_update as *const (),
        |original| ORIGINAL_FN.store(original as *mut (), Relaxed),
    );
}
