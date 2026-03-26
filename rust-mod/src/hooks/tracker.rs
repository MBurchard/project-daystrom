//! Reusable IL2CPP hook utilities.
//!
//! Provides building blocks for hooking Unity lifecycle methods and resolving IL2CPP method pointers.
//! The `instance_tracker!` macro generates self-contained modules for Awake/OnDestroy instance tracking.

use log::{debug, warn};

use crate::hook::engine;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Method resolution ----------------------------------------------------

/// Resolve a method on a class and return its raw function pointer.
///
/// Thin wrapper around `resolver::resolve_method` that extracts the `method_pointer` field.
/// Returns `None` if the method cannot be found.
pub fn resolve_fn(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
) -> Option<*const ()> {
    resolver::resolve_method(api, class, method_name, param_count)
        .map(|m| unsafe { (*m).method_pointer })
}

// ---- IL2CPP field access --------------------------------------------------

/// Read a pointer-sized field at `offset` bytes from an IL2CPP object.
///
/// # Safety
///
/// The caller must ensure that `base` is a valid object pointer and that a pointer-sized field actually exists
/// at the given offset.
pub unsafe fn read_ptr(base: *const (), offset: usize) -> *mut () {
    unsafe { *((base as *const u8).add(offset) as *const *mut ()) }
}

/// Read an `i32` field at `offset` bytes from an IL2CPP object.
///
/// # Safety
///
/// The caller must ensure that `base` is a valid object pointer and that an `i32` field actually exists at
/// the given offset.
pub unsafe fn read_i32(base: *const (), offset: usize) -> i32 {
    unsafe { *((base as *const u8).add(offset) as *const i32) }
}

// ---- Lifecycle hook installation ------------------------------------------

/// Install Awake and OnDestroy hooks on a class for instance tracking.
///
/// Resolves both methods, installs inline hooks, and passes the original function pointers (trampolines) to the
/// provided closures for storage.
/// Either hook may fail independently without blocking the other.
pub fn install_lifecycle_hooks(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    label: &str,
    awake_hook: extern "C" fn(*mut Il2CppObject),
    destroy_hook: extern "C" fn(*mut Il2CppObject),
    store_awake: impl FnOnce(*const ()),
    store_destroy: impl FnOnce(*const ()),
) {
    if let Some(ptr) = resolve_fn(api, class, "Awake", 0) {
        match engine::install_hook(
            &format!("{label}Awake"), ptr, awake_hook as *const (),
        ) {
            Ok(orig) => {
                store_awake(orig);
                debug!(target: "HookEngine", "{label} Awake hook installed");
            }
            Err(e) => warn!(target: "HookEngine", "Failed to hook {label} Awake: {e}"),
        }
    }
    if let Some(ptr) = resolve_fn(api, class, "OnDestroy", 0) {
        match engine::install_hook(
            &format!("{label}Destroy"), ptr, destroy_hook as *const (),
        ) {
            Ok(orig) => {
                store_destroy(orig);
                debug!(target: "HookEngine", "{label} OnDestroy hook installed");
            }
            Err(e) => warn!(target: "HookEngine", "Failed to hook {label} OnDestroy: {e}"),
        }
    }
}

/// Install only the Awake hook on a class (no OnDestroy).
///
/// Use this when OnDestroy is handled by a shared hook on a base class. See `install_lifecycle_hooks` for the
/// full Awake + OnDestroy variant.
pub fn install_awake_hook(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    label: &str,
    awake_hook: extern "C" fn(*mut Il2CppObject),
    store_awake: impl FnOnce(*const ()),
) {
    if let Some(ptr) = resolve_fn(api, class, "Awake", 0) {
        match engine::install_hook(
            &format!("{label}Awake"), ptr, awake_hook as *const (),
        ) {
            Ok(orig) => {
                store_awake(orig);
                debug!(target: "HookEngine", "{label} Awake hook installed");
            }
            Err(e) => warn!(target: "HookEngine", "Failed to hook {label} Awake: {e}"),
        }
    }
}

// ---- Instance tracker macro -----------------------------------------------

/// Generates a self-contained instance tracker module.
///
/// Creates a child module `$name` with:
/// - `get() -> *mut ()` to read the tracked instance
/// - `install(api, class, label)` to hook Awake/OnDestroy
/// - `hook_awake` / `hook_destroy` (the `extern "C"` hook functions)
///
/// Usage:
/// ```ignore
/// instance_tracker!(reward);
///
/// // During installation:
/// reward::install(api, class, "Reward");
///
/// // At runtime:
/// let instance = reward::get();
/// ```
macro_rules! instance_tracker {
    ($name:ident) => {
        #[allow(dead_code)]
        mod $name {
            use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

            type LifecycleFn =
                unsafe extern "C" fn(*mut $crate::il2cpp::types::Il2CppObject);

            static INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
            static ORIG_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
            static ORIG_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

            /// Read the currently tracked instance (maybe null).
            pub fn get() -> *mut () {
                INSTANCE.load(Relaxed)
            }

            /// Hook for `Awake()`: stores this instance as the tracked one.
            pub extern "C" fn hook_awake(
                this: *mut $crate::il2cpp::types::Il2CppObject,
            ) {
                INSTANCE.store(this as *mut (), Relaxed);
                let orig: LifecycleFn =
                    unsafe { std::mem::transmute(ORIG_AWAKE.load(Relaxed)) };
                unsafe { orig(this) };
            }

            /// OnDestroy hook: clears the tracked instance if it matches.
            pub extern "C" fn hook_destroy(
                this: *mut $crate::il2cpp::types::Il2CppObject,
            ) {
                let _ = INSTANCE.compare_exchange(
                    this as *mut (),
                    std::ptr::null_mut(),
                    Relaxed,
                    Relaxed,
                );
                let orig: LifecycleFn =
                    unsafe { std::mem::transmute(ORIG_DESTROY.load(Relaxed)) };
                unsafe { orig(this) };
            }

            /// Clear the tracked instance if it matches the given pointer.
            ///
            /// Used by shared OnDestroy hooks that serve multiple trackers
            /// (e.g. when subclasses share a base class OnDestroy).
            pub fn clear_if_match(ptr: *mut ()) {
                let _ = INSTANCE.compare_exchange(
                    ptr,
                    std::ptr::null_mut(),
                    Relaxed,
                    Relaxed,
                );
            }

            /// Set up no-op originals so `hook_awake`/`hook_destroy` can be
            /// called safely in unit tests.
            #[cfg(test)]
            pub fn _test_init() {
                extern "C" fn noop(
                    _: *mut $crate::il2cpp::types::Il2CppObject,
                ) {}
                ORIG_AWAKE.store(noop as *mut (), Relaxed);
                ORIG_DESTROY.store(noop as *mut (), Relaxed);
            }

            /// Hook Awake and OnDestroy on the given class for automatic
            /// instance tracking.
            pub fn install(
                api: &$crate::il2cpp::api::Il2CppApi,
                class: *mut $crate::il2cpp::types::Il2CppClass,
                label: &str,
            ) {
                $crate::hooks::tracker::install_lifecycle_hooks(
                    api,
                    class,
                    label,
                    hook_awake,
                    hook_destroy,
                    |p| ORIG_AWAKE.store(p as *mut (), Relaxed),
                    |p| ORIG_DESTROY.store(p as *mut (), Relaxed),
                );
            }

            /// Hook only Awake on the given class.
            ///
            /// Use this when OnDestroy is handled by a shared hook (e.g. on the base class) instead of
            /// per-subclass hooks.
            pub fn install_awake(
                api: &$crate::il2cpp::api::Il2CppApi,
                class: *mut $crate::il2cpp::types::Il2CppClass,
                label: &str,
            ) {
                $crate::hooks::tracker::install_awake_hook(
                    api,
                    class,
                    label,
                    hook_awake,
                    |p| ORIG_AWAKE.store(p as *mut (), Relaxed),
                );
            }
        }
    };
}
pub(crate) use instance_tracker;

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    instance_tracker!(subject);

    #[test]
    fn instance_starts_null() {
        assert!(subject::get().is_null());
    }

    #[test]
    fn awake_stores_and_destroy_clears() {
        subject::_test_init();

        let fake = 0x1234usize as *mut crate::il2cpp::types::Il2CppObject;
        subject::hook_awake(fake);
        assert_eq!(subject::get(), fake as *mut ());

        subject::hook_destroy(fake);
        assert!(subject::get().is_null());
    }

    #[test]
    fn clear_if_match_clears_matching() {
        subject::_test_init();

        let fake = 0xCCCCusize as *mut crate::il2cpp::types::Il2CppObject;
        subject::hook_awake(fake);
        assert_eq!(subject::get(), fake as *mut ());

        subject::clear_if_match(fake as *mut ());
        assert!(subject::get().is_null());
    }

    #[test]
    fn clear_if_match_ignores_mismatch() {
        subject::_test_init();

        let fake_a = 0xDDDDusize as *mut crate::il2cpp::types::Il2CppObject;
        let fake_b = 0xEEEEusize as *mut ();

        subject::hook_awake(fake_a);
        subject::clear_if_match(fake_b);
        assert_eq!(subject::get(), fake_a as *mut ());

        // Cleanup.
        subject::hook_destroy(fake_a);
    }

    #[test]
    fn destroy_preserves_different_instance() {
        subject::_test_init();

        let fake_a =
            0xAAAAusize as *mut crate::il2cpp::types::Il2CppObject;
        let fake_b =
            0xBBBBusize as *mut crate::il2cpp::types::Il2CppObject;

        subject::hook_awake(fake_b);
        subject::hook_destroy(fake_a);
        assert_eq!(subject::get(), fake_b as *mut ());

        // Cleanup.
        subject::hook_destroy(fake_b);
    }
}
