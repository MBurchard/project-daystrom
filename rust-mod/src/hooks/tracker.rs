//! Reusable IL2CPP hook utilities.
//!
//! Provides building blocks for hooking Unity lifecycle methods and resolving IL2CPP method pointers.
//! The `instance_tracker!` macro generates self-contained modules for Awake/OnDestroy instance tracking.

use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

use log::warn;

use crate::hook::engine;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

const LOG_TARGET: &str = "HookEngine.Tracker";

// ---- Method resolution ----------------------------------------------------

/// Resolve a method on a class and return its raw function pointer.
///
/// Thin wrapper around `resolver::resolve_method` that extracts the `method_pointer` field.
/// Returns `None` if the method cannot be found.
pub fn resolve_fn(api: &Il2CppApi, class: *mut Il2CppClass, method_name: &str, param_count: i32) -> Option<*const ()> {
    // Safety: `resolver::resolve_method` returns a MethodInfo pointer owned by IL2CPP metadata.
    resolver::resolve_method(api, class, method_name, param_count).map(|m| unsafe { (*m).method_pointer })
}

// ---- Resolved hook installation -------------------------------------------

type HookInstaller = fn(&str, *const (), *const ()) -> Result<*const (), engine::HookError>;

struct ResolvedHookInstall<'a, StoreOriginal> {
    api: &'a Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &'a str,
    param_count: i32,
    hook_name: &'a str,
    replacement: *const (),
    original: StoreOriginal,
    install: HookInstaller,
}

fn install_resolved_hook_with<StoreOriginal: FnOnce(*const ())>(
    request: ResolvedHookInstall<'_, StoreOriginal>,
) -> bool {
    let ResolvedHookInstall {
        api,
        class,
        method_name,
        param_count,
        hook_name,
        replacement,
        original,
        install,
    } = request;

    let Some(method) = resolver::resolve_method(api, class, method_name, param_count) else {
        return false;
    };

    // Safety: `resolver::resolve_method` returned this MethodInfo pointer from IL2CPP metadata.
    let target = unsafe { (*method).method_pointer };
    match install(hook_name, target, replacement) {
        Ok(orig) => {
            original(orig);
            true
        }
        Err(e) => {
            warn!(target: LOG_TARGET, "Failed to hook {hook_name}: {e}");
            false
        }
    }
}

/// Resolve an IL2CPP method, install an inline hook, and store the original trampoline.
///
/// Returns `false` when method resolution or hook installation fails. Resolution failures are logged by
/// `resolver::resolve_method`; hook installation failures are logged here.
pub fn install_resolved_hook(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    hook_name: &str,
    replacement: *const (),
    original: impl FnOnce(*const ()),
) -> bool {
    install_resolved_hook_with(ResolvedHookInstall {
        api,
        class,
        method_name,
        param_count,
        hook_name,
        replacement,
        original,
        install: engine::install_hook,
    })
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

/// Read an `f32` field at `offset` bytes from an IL2CPP object.
///
/// # Safety
///
/// The caller must ensure that `base` is a valid object pointer and that an `f32` field actually exists at
/// the given offset.
pub unsafe fn read_f32(base: *const (), offset: usize) -> f32 {
    unsafe { *((base as *const u8).add(offset) as *const f32) }
}

// ---- Lifecycle hook installation ------------------------------------------

/// Request data for idempotent Awake/OnDestroy hook installation.
///
/// The original trampoline slots are both inputs and outputs: a non-null slot means that hook was already installed,
/// while a successful install stores the newly returned trampoline.
pub(crate) struct LifecycleHookInstall<'a> {
    pub(crate) api: &'a Il2CppApi,
    pub(crate) class: *mut Il2CppClass,
    pub(crate) label: &'a str,
    pub(crate) awake_hook: extern "C" fn(*mut Il2CppObject),
    pub(crate) destroy_hook: extern "C" fn(*mut Il2CppObject),
    pub(crate) original_awake: &'a AtomicPtr<()>,
    pub(crate) original_destroy: &'a AtomicPtr<()>,
    pub(crate) install: HookInstaller,
}

/// Install missing Awake/OnDestroy hooks and report whether both are available afterwards.
///
/// Already-installed hooks are detected through non-null original trampoline slots and are not installed again.
/// If one hook succeeds and the other fails, a later call retries only the missing hook.
pub(crate) fn install_lifecycle_hooks_once_with(request: LifecycleHookInstall<'_>) -> bool {
    let LifecycleHookInstall {
        api,
        class,
        label,
        awake_hook,
        destroy_hook,
        original_awake,
        original_destroy,
        install,
    } = request;

    let mut awake_installed = !original_awake.load(Relaxed).is_null();
    if !awake_installed {
        let hook_name = format!("{label}Awake");
        awake_installed = install_resolved_hook_with(ResolvedHookInstall {
            api,
            class,
            method_name: "Awake",
            param_count: 0,
            hook_name: &hook_name,
            replacement: awake_hook as *const (),
            original: |p| original_awake.store(p as *mut (), Relaxed),
            install,
        });
    }

    let mut destroy_installed = !original_destroy.load(Relaxed).is_null();
    if !destroy_installed {
        let hook_name = format!("{label}Destroy");
        destroy_installed = install_resolved_hook_with(ResolvedHookInstall {
            api,
            class,
            method_name: "OnDestroy",
            param_count: 0,
            hook_name: &hook_name,
            replacement: destroy_hook as *const (),
            original: |p| original_destroy.store(p as *mut (), Relaxed),
            install,
        });
    }

    awake_installed && destroy_installed
}

/// Install only the Awake hook on a class (no OnDestroy).
///
/// Use this when OnDestroy is handled by a shared hook on a base class.
pub fn install_awake_hook(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    label: &str,
    awake_hook: extern "C" fn(*mut Il2CppObject),
    store_awake: impl FnOnce(*const ()),
) -> bool {
    install_resolved_hook(
        api,
        class,
        "Awake",
        0,
        &format!("{label}Awake"),
        awake_hook as *const (),
        store_awake,
    )
}

// ---- Instance tracker macro -----------------------------------------------

/// Generates a self-contained instance tracker module.
///
/// Creates a child module `$name` with:
/// - `get() -> *mut ()` to read the tracked instance
/// - `install(api, class, label) -> bool` to hook Awake/OnDestroy
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

            type LifecycleFn = unsafe extern "C" fn(*mut $crate::il2cpp::types::Il2CppObject);

            static INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
            static ORIG_AWAKE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
            static ORIG_DESTROY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

            /// Read the currently tracked instance (maybe null).
            pub fn get() -> *mut () {
                INSTANCE.load(Relaxed)
            }

            /// Hook for `Awake()`: stores this instance as the tracked one.
            pub extern "C" fn hook_awake(this: *mut $crate::il2cpp::types::Il2CppObject) {
                INSTANCE.store(this as *mut (), Relaxed);
                let orig_ptr = ORIG_AWAKE.load(Relaxed);
                if !orig_ptr.is_null() {
                    let orig: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
                    unsafe { orig(this) };
                }
            }

            /// OnDestroy hook: clears the tracked instance if it matches.
            pub extern "C" fn hook_destroy(this: *mut $crate::il2cpp::types::Il2CppObject) {
                let _ = INSTANCE.compare_exchange(this as *mut (), std::ptr::null_mut(), Relaxed, Relaxed);
                let orig_ptr = ORIG_DESTROY.load(Relaxed);
                if !orig_ptr.is_null() {
                    let orig: LifecycleFn = unsafe { std::mem::transmute(orig_ptr) };
                    unsafe { orig(this) };
                }
            }

            /// Clear the tracked instance if it matches the given pointer.
            ///
            /// Used by shared OnDestroy hooks that serve multiple trackers
            /// (e.g. when subclasses share a base class OnDestroy).
            pub fn clear_if_match(ptr: *mut ()) {
                let _ = INSTANCE.compare_exchange(ptr, std::ptr::null_mut(), Relaxed, Relaxed);
            }

            /// Set up no-op originals so `hook_awake`/`hook_destroy` can be
            /// called safely in unit tests.
            #[cfg(test)]
            pub fn _test_init() {
                extern "C" fn noop(_: *mut $crate::il2cpp::types::Il2CppObject) {}
                INSTANCE.store(std::ptr::null_mut(), Relaxed);
                ORIG_AWAKE.store(noop as *mut (), Relaxed);
                ORIG_DESTROY.store(noop as *mut (), Relaxed);
            }

            /// Clear all tracker state for unit tests.
            #[cfg(test)]
            pub fn _test_reset() {
                INSTANCE.store(std::ptr::null_mut(), Relaxed);
                ORIG_AWAKE.store(std::ptr::null_mut(), Relaxed);
                ORIG_DESTROY.store(std::ptr::null_mut(), Relaxed);
            }

            /// Hook Awake and OnDestroy on the given class for automatic
            /// instance tracking.
            pub fn install(
                api: &$crate::il2cpp::api::Il2CppApi,
                class: *mut $crate::il2cpp::types::Il2CppClass,
                label: &str,
            ) -> bool {
                $crate::hooks::tracker::install_lifecycle_hooks_once_with(
                    $crate::hooks::tracker::LifecycleHookInstall {
                        api,
                        class,
                        label,
                        awake_hook: hook_awake,
                        destroy_hook: hook_destroy,
                        original_awake: &ORIG_AWAKE,
                        original_destroy: &ORIG_DESTROY,
                        install: $crate::hook::engine::install_hook,
                    },
                )
            }

            /// Hook only Awake on the given class.
            ///
            /// Use this when OnDestroy is handled by a shared hook (e.g. on the base class) instead of
            /// per-subclass hooks.
            pub fn install_awake(
                api: &$crate::il2cpp::api::Il2CppApi,
                class: *mut $crate::il2cpp::types::Il2CppClass,
                label: &str,
            ) -> bool {
                if !ORIG_AWAKE.load(Relaxed).is_null() {
                    return true;
                }

                $crate::hooks::tracker::install_awake_hook(api, class, label, hook_awake, |p| {
                    ORIG_AWAKE.store(p as *mut (), Relaxed)
                })
            }
        }
    };
}
pub(crate) use instance_tracker;

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::ffi::c_char;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

    use crate::hook::engine::HookError;
    use crate::il2cpp::api::Il2CppApi;
    use crate::il2cpp::types::*;

    use super::*;

    instance_tracker!(subject);

    static METHOD_AVAILABLE: AtomicBool = AtomicBool::new(true);
    static INSTALL_SHOULD_FAIL: AtomicBool = AtomicBool::new(false);
    static INSTALLED_TARGET: AtomicUsize = AtomicUsize::new(0);
    static INSTALLED_REPLACEMENT: AtomicUsize = AtomicUsize::new(0);
    static INSTALL_CALLS: AtomicUsize = AtomicUsize::new(0);
    static HOOK_HELPER_TEST_LOCK: Mutex<()> = Mutex::new(());

    extern "C" fn fake_target() {}
    extern "C" fn fake_replacement() {}
    extern "C" fn fake_original() {}

    static mut RESOLVED_METHOD: MethodInfo = MethodInfo { method_pointer: fake_target as *const () };

    unsafe extern "C" fn fake_domain_get() -> *mut Il2CppDomain {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_domain_assembly_open(_: *mut Il2CppDomain, _: *const c_char) -> *mut Il2CppAssembly {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_assembly_get_image(_: *mut Il2CppAssembly) -> *mut Il2CppImage {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_class_from_name(
        _: *mut Il2CppImage,
        _: *const c_char,
        _: *const c_char,
    ) -> *mut Il2CppClass {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_class_get_method_from_name(
        _: *mut Il2CppClass,
        _: *const c_char,
        _: i32,
    ) -> *const MethodInfo {
        if METHOD_AVAILABLE.load(Relaxed) {
            std::ptr::addr_of!(RESOLVED_METHOD)
        } else {
            std::ptr::null()
        }
    }

    unsafe extern "C" fn fake_method_get_return_type(_: *const MethodInfo) -> *const Il2CppType {
        std::ptr::null()
    }

    unsafe extern "C" fn fake_method_get_param(_: *const MethodInfo, _: u32) -> *const Il2CppType {
        std::ptr::null()
    }

    unsafe extern "C" fn fake_method_get_flags(_: *const MethodInfo, _: *mut u32) -> u32 {
        0
    }

    unsafe extern "C" fn fake_type_get_type(_: *const Il2CppType) -> i32 {
        0
    }

    unsafe extern "C" fn fake_class_from_type(_: *const Il2CppType) -> *mut Il2CppClass {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_class_is_valuetype(_: *const Il2CppClass) -> bool {
        false
    }

    unsafe extern "C" fn fake_class_get_field_from_name(_: *mut Il2CppClass, _: *const c_char) -> *mut FieldInfo {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_field_get_offset(_: *mut FieldInfo) -> usize {
        0
    }

    unsafe extern "C" fn fake_runtime_invoke(
        _: *const MethodInfo,
        _: *mut Il2CppObject,
        _: *mut *mut Il2CppObject,
        _: *mut *mut Il2CppException,
    ) -> *mut Il2CppObject {
        std::ptr::null_mut()
    }

    unsafe extern "C" fn fake_string_new(_: *const c_char) -> *mut Il2CppString {
        std::ptr::null_mut()
    }

    fn fake_api() -> Il2CppApi {
        Il2CppApi {
            domain_get: fake_domain_get,
            domain_assembly_open: fake_domain_assembly_open,
            assembly_get_image: fake_assembly_get_image,
            class_from_name: fake_class_from_name,
            class_get_method_from_name: fake_class_get_method_from_name,
            method_get_return_type: fake_method_get_return_type,
            method_get_param: fake_method_get_param,
            method_get_flags: fake_method_get_flags,
            type_get_type: fake_type_get_type,
            class_from_type: fake_class_from_type,
            class_is_valuetype: fake_class_is_valuetype,
            class_get_field_from_name: fake_class_get_field_from_name,
            field_get_offset: fake_field_get_offset,
            runtime_invoke: fake_runtime_invoke,
            string_new: fake_string_new,
        }
    }

    fn fake_installer(_: &str, target: *const (), replacement: *const ()) -> Result<*const (), HookError> {
        INSTALL_CALLS.fetch_add(1, Relaxed);
        INSTALLED_TARGET.store(target as usize, Relaxed);
        INSTALLED_REPLACEMENT.store(replacement as usize, Relaxed);
        if INSTALL_SHOULD_FAIL.load(Relaxed) {
            Err(HookError::BackendError("test failure".to_string()))
        } else {
            Ok(fake_original as *const ())
        }
    }

    fn fake_installer_fails_destroy(
        name: &str,
        target: *const (),
        replacement: *const (),
    ) -> Result<*const (), HookError> {
        INSTALL_CALLS.fetch_add(1, Relaxed);
        INSTALLED_TARGET.store(target as usize, Relaxed);
        INSTALLED_REPLACEMENT.store(replacement as usize, Relaxed);
        if name.ends_with("Destroy") {
            Err(HookError::BackendError("test failure".to_string()))
        } else {
            Ok(fake_original as *const ())
        }
    }

    fn reset_hook_helper_test_state() {
        METHOD_AVAILABLE.store(true, Relaxed);
        INSTALL_SHOULD_FAIL.store(false, Relaxed);
        INSTALLED_TARGET.store(0, Relaxed);
        INSTALLED_REPLACEMENT.store(0, Relaxed);
        INSTALL_CALLS.store(0, Relaxed);
        subject::_test_reset();
    }

    fn test_resolved_hook_install<'a>(
        api: &'a Il2CppApi,
        original: &'a AtomicPtr<()>,
    ) -> ResolvedHookInstall<'a, impl FnOnce(*const ()) + 'a> {
        ResolvedHookInstall {
            api,
            class: 0xABCDusize as *mut Il2CppClass,
            method_name: "Awake",
            param_count: 0,
            hook_name: "TestHook",
            replacement: fake_replacement as *const (),
            original: |orig| original.store(orig as *mut (), Relaxed),
            install: fake_installer,
        }
    }

    fn install_test_lifecycle_hooks(
        api: &Il2CppApi,
        original_awake: &AtomicPtr<()>,
        original_destroy: &AtomicPtr<()>,
        install: HookInstaller,
    ) -> bool {
        install_lifecycle_hooks_once_with(LifecycleHookInstall {
            api,
            class: 0xABCDusize as *mut Il2CppClass,
            label: "Subject",
            awake_hook: subject::hook_awake,
            destroy_hook: subject::hook_destroy,
            original_awake,
            original_destroy,
            install,
        })
    }

    #[test]
    fn instance_starts_null() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        assert!(subject::get().is_null());
    }

    #[test]
    fn install_resolved_hook_stores_original_on_success() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        let api = fake_api();
        let original = AtomicPtr::new(std::ptr::null_mut());

        let installed = install_resolved_hook_with(test_resolved_hook_install(&api, &original));

        assert!(installed);
        assert_eq!(INSTALLED_TARGET.load(Relaxed), fake_target as *const () as usize);
        assert_eq!(INSTALLED_REPLACEMENT.load(Relaxed), fake_replacement as *const () as usize,);
        assert_eq!(original.load(Relaxed), fake_original as *mut ());
        assert_eq!(INSTALL_CALLS.load(Relaxed), 1);
    }

    #[test]
    fn install_resolved_hook_returns_false_when_method_is_missing() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        METHOD_AVAILABLE.store(false, Relaxed);
        let api = fake_api();
        let original = AtomicPtr::new(std::ptr::null_mut());

        let installed = install_resolved_hook_with(test_resolved_hook_install(&api, &original));

        assert!(!installed);
        assert_eq!(INSTALLED_TARGET.load(Relaxed), 0);
        assert!(original.load(Relaxed).is_null());
    }

    #[test]
    fn install_resolved_hook_returns_false_when_installer_fails() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        INSTALL_SHOULD_FAIL.store(true, Relaxed);
        let api = fake_api();
        let original = AtomicPtr::new(std::ptr::null_mut());

        let installed = install_resolved_hook_with(test_resolved_hook_install(&api, &original));

        assert!(!installed);
        assert_eq!(INSTALLED_TARGET.load(Relaxed), fake_target as *const () as usize);
        assert!(original.load(Relaxed).is_null());
    }

    #[test]
    fn install_lifecycle_hooks_returns_true_only_when_both_hooks_install() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        let api = fake_api();
        let original_awake = AtomicPtr::new(std::ptr::null_mut());
        let original_destroy = AtomicPtr::new(std::ptr::null_mut());

        let installed = install_test_lifecycle_hooks(&api, &original_awake, &original_destroy, fake_installer);

        assert!(installed);
        assert_eq!(original_awake.load(Relaxed), fake_original as *mut ());
        assert_eq!(original_destroy.load(Relaxed), fake_original as *mut ());
        assert_eq!(INSTALL_CALLS.load(Relaxed), 2);
    }

    #[test]
    fn install_lifecycle_hooks_retries_only_missing_hook_after_partial_failure() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        let api = fake_api();
        let original_awake = AtomicPtr::new(std::ptr::null_mut());
        let original_destroy = AtomicPtr::new(std::ptr::null_mut());

        let first_installed =
            install_test_lifecycle_hooks(&api, &original_awake, &original_destroy, fake_installer_fails_destroy);
        assert!(!first_installed);
        assert_eq!(original_awake.load(Relaxed), fake_original as *mut ());
        assert!(original_destroy.load(Relaxed).is_null());
        assert_eq!(INSTALL_CALLS.load(Relaxed), 2);

        let second_installed = install_test_lifecycle_hooks(&api, &original_awake, &original_destroy, fake_installer);

        assert!(second_installed);
        assert_eq!(original_awake.load(Relaxed), fake_original as *mut ());
        assert_eq!(original_destroy.load(Relaxed), fake_original as *mut ());
        assert_eq!(INSTALL_CALLS.load(Relaxed), 3);
    }

    #[test]
    fn awake_stores_and_destroy_clears() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        subject::_test_init();

        let fake = 0x1234usize as *mut Il2CppObject;
        subject::hook_awake(fake);
        assert_eq!(subject::get(), fake as *mut ());

        subject::hook_destroy(fake);
        assert!(subject::get().is_null());
    }

    #[test]
    fn clear_if_match_clears_matching() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        subject::_test_init();

        let fake = 0xCCCCusize as *mut Il2CppObject;
        subject::hook_awake(fake);
        assert_eq!(subject::get(), fake as *mut ());

        subject::clear_if_match(fake as *mut ());
        assert!(subject::get().is_null());
    }

    #[test]
    fn clear_if_match_ignores_mismatch() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        subject::_test_init();

        let fake_a = 0xDDDDusize as *mut Il2CppObject;
        let fake_b = 0xEEEEusize as *mut ();

        subject::hook_awake(fake_a);
        subject::clear_if_match(fake_b);
        assert_eq!(subject::get(), fake_a as *mut ());

        // Cleanup.
        subject::hook_destroy(fake_a);
    }

    #[test]
    fn destroy_preserves_different_instance() {
        let _guard = HOOK_HELPER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_hook_helper_test_state();
        subject::_test_init();

        let fake_a = 0xAAAAusize as *mut Il2CppObject;
        let fake_b = 0xBBBBusize as *mut Il2CppObject;

        subject::hook_awake(fake_b);
        subject::hook_destroy(fake_a);
        assert_eq!(subject::get(), fake_b as *mut ());

        // Cleanup.
        subject::hook_destroy(fake_b);
    }
}
