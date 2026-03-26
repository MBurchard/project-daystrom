use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::{debug, error};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

/// Offset of `m_ScaleFactor` within `CanvasScaler` (from IL2CPP dump).
const M_SCALE_FACTOR_OFFSET: usize = 0x28;

/// Original function pointer: `void Handle(CanvasScaler* this)`.
static ORIGINAL_HANDLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Function pointer for `SetScaleFactor(CanvasScaler* this, float scaleFactor)`.
static SET_SCALE_FACTOR_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("UiScale");

/// Whether the first-call debug log has been emitted.
static LOGGED_FIRST_CALL: AtomicBool = AtomicBool::new(false);

/// Type alias for the original Handle function signature.
type HandleFn = unsafe extern "C" fn(*mut Il2CppObject);

/// Type alias for SetScaleFactor.
type SetScaleFactorFn = unsafe extern "C" fn(*mut Il2CppObject, f32);

/// Read the `m_ScaleFactor` field directly from a CanvasScaler instance.
///
/// This is the serialised base value set by the game designer. It is never modified by
/// `Handle()` or `SetScaleFactor()`, so reading it every frame gives us a stable base
/// without compounding.
///
/// # Safety
/// Caller must ensure `this` points to a valid CanvasScaler instance.
unsafe fn read_base_scale(this: *mut Il2CppObject) -> f32 {
    let ptr = (this as *const u8).add(M_SCALE_FACTOR_OFFSET) as *const f32;
    ptr.read()
}

/// Hook replacement for `CanvasScaler.Handle()`.
///
/// Calls the original Handle (which sets Canvas.scaleFactor to the game's base value),
/// then applies our UI scale multiplier via `SetScaleFactor`. Because `m_ScaleFactor`
/// (the serialised field) is never modified, there is no compounding across frames.
extern "C" fn hook_handle(this: *mut Il2CppObject) {
    let original: HandleFn = unsafe { std::mem::transmute(ORIGINAL_HANDLE.load(Relaxed)) };

    // Always call the original first so the game's normal scaling runs.
    unsafe { original(this) };

    if !HOOK_INFO.is_active() {
        return;
    }

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let scale_pct = crate::settings::get_scale();

        // At 100% we have nothing to do — the original already set the correct scale.
        if scale_pct == 100 {
            return;
        }

        let base = unsafe { read_base_scale(this) };
        let modified = base * (scale_pct as f32 / 100.0);

        if !LOGGED_FIRST_CALL.swap(true, Relaxed) {
            debug!(
                target: "UiScale",
                "First apply: base={base:.4}, setting={scale_pct}%, modified={modified:.4}"
            );
        }

        let ptr = SET_SCALE_FACTOR_FN.load(Relaxed);
        if ptr.is_null() {
            return;
        }
        let set_scale: SetScaleFactorFn = unsafe { std::mem::transmute(ptr) };
        unsafe { set_scale(this, modified) };
    }));

    if result.is_err() {
        HOOK_INFO.record_error();
    }
}

/// Install the UI scale hook via IL2CPP reflection.
///
/// Hooks `CanvasScaler.Handle()` (called every frame via `Canvas_preWillRenderCanvases`)
/// and resolves `SetScaleFactor` for applying the modified scale after the original
/// handler runs.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "UnityEngine.UI", "UnityEngine.UI", "CanvasScaler",
    ) else {
        return;
    };

    // Resolve SetScaleFactor (protected, 1 param) for calling from our hook.
    let Some(set_scale_method) =
        resolver::resolve_method(api, class, "SetScaleFactor", 1)
    else {
        return;
    };
    let set_scale_ptr = unsafe { (*set_scale_method).method_pointer };
    SET_SCALE_FACTOR_FN.store(set_scale_ptr as *mut (), Relaxed);

    // Hook Handle (virtual, 0 params) — called every frame.
    let Some(handle_method) =
        resolver::resolve_method(api, class, "Handle", 0)
    else {
        return;
    };
    let handle_target = unsafe { (*handle_method).method_pointer };

    match engine::install_hook("UiScale", handle_target, hook_handle as *const ()) {
        Ok(original) => {
            ORIGINAL_HANDLE.store(original as *mut (), Relaxed);
            debug!(target: "HookEngine", "UiScale hook installed (Handle + SetScaleFactor)");
        }
        Err(e) => {
            error!(target: "HookEngine", "Failed to hook Handle (UiScale): {e}");
        }
    }
}
