//! Skip shop reveal sequence animation.
//!
//! Hooks `ShopSceneManager.ShouldShowRevealSequence()` to optionally bypass the loot box opening animation.
//! When the setting is active, the hook returns `false` directly without calling the original, saving the overhead of
//! the original method.

use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `ShopSceneManager.ShouldShowRevealSequence(bool)`.
static ORIGINAL_FN: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ---- Type aliases ---------------------------------------------------------

type ShouldShowFn = unsafe extern "C" fn(*mut Il2CppObject, bool) -> bool;

// ---- Hook -----------------------------------------------------------------

/// Hook for `ShopSceneManager.ShouldShowRevealSequence(bool)`.
///
/// When `skip_reveal_sequence` is enabled, it returns `false` immediately without calling the original.
/// This skips the entire reveal animation and avoids the overhead of the original method.
/// When the setting is off, the original decides.
extern "C" fn hook_should_show(this: *mut Il2CppObject, ignore: bool) -> bool {
    if crate::settings::skip_reveal_sequence() {
        return false;
    }
    let orig_ptr = ORIGINAL_FN.load(Relaxed);
    if orig_ptr.is_null() {
        return true; // Defensive: show animation if original is missing.
    }
    let original: ShouldShowFn = unsafe { std::mem::transmute(orig_ptr) };
    unsafe { original(this, ignore) }
}

// ---- Installation ---------------------------------------------------------

/// Install the shop reveal sequence hook.
///
/// Hooks `ShopSceneManager.ShouldShowRevealSequence` to allow skipping the loot box animation.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Shop", "ShopSceneManager") else {
        log::warn!(target: "ShopReveal", "ShopSceneManager not found");
        return;
    };

    let Some(ptr) = tracker::resolve_fn(api, class, "ShouldShowRevealSequence", 1) else {
        log::warn!(target: "ShopReveal", "ShouldShowRevealSequence not found");
        return;
    };

    match crate::hook::engine::install_hook("ShopReveal", ptr, hook_should_show as *const ()) {
        Ok(orig) => {
            ORIGINAL_FN.store(orig as *mut (), Relaxed);
            debug!(target: "ShopReveal", "ShouldShowRevealSequence hook installed");
        }
        Err(e) => log::warn!(target: "ShopReveal", "Failed to hook ShouldShowRevealSequence: {e}"),
    }
}
