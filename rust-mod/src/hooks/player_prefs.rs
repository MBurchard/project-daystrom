use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

use log::{debug, error, info, warn};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;
use crate::profile_store;

// ---- Original function pointers -------------------------------------------

// String
static ORIGINAL_SET_STRING: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_GET_STRING_2: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_GET_STRING_1: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
// Int
static ORIGINAL_SET_INT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_GET_INT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
// Float
static ORIGINAL_SET_FLOAT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_GET_FLOAT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
// HasKey / DeleteKey
static ORIGINAL_HAS_KEY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_DELETE_KEY: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ---- Hook safety tracking -------------------------------------------------

static HOOK_SET: HookInfo = HookInfo::new("PlayerPrefs.SetString");
static HOOK_GET2: HookInfo = HookInfo::new("PlayerPrefs.GetString/2");
static HOOK_GET1: HookInfo = HookInfo::new("PlayerPrefs.GetString/1");
static HOOK_SET_INT: HookInfo = HookInfo::new("PlayerPrefs.SetInt");
static HOOK_GET_INT: HookInfo = HookInfo::new("PlayerPrefs.GetInt");
static HOOK_SET_FLOAT: HookInfo = HookInfo::new("PlayerPrefs.SetFloat");
static HOOK_GET_FLOAT: HookInfo = HookInfo::new("PlayerPrefs.GetFloat");
static HOOK_HAS_KEY: HookInfo = HookInfo::new("PlayerPrefs.HasKey");
static HOOK_DELETE_KEY: HookInfo = HookInfo::new("PlayerPrefs.DeleteKey");

// ---- IL2CPP function signatures -------------------------------------------
//
// Static C# methods have no `this` pointer. IL2CPP appends a `MethodInfo*` as
// the last argument.

/// `static void SetString(string key, string value)`
type SetStringFn = unsafe extern "C" fn(
    *mut Il2CppString,
    *mut Il2CppString,
    *const MethodInfo,
);

/// `static string GetString(string key, string defaultValue)`
type GetString2Fn = unsafe extern "C" fn(
    *mut Il2CppString,
    *mut Il2CppString,
    *const MethodInfo,
) -> *mut Il2CppString;

/// `static string GetString(string key)`
type GetString1Fn = unsafe extern "C" fn(
    *mut Il2CppString,
    *const MethodInfo,
) -> *mut Il2CppString;

/// `static void SetInt(string key, int value)`
type SetIntFn = unsafe extern "C" fn(*mut Il2CppString, i32, *const MethodInfo);

/// `static int GetInt(string key, int defaultValue)`
type GetIntFn = unsafe extern "C" fn(*mut Il2CppString, i32, *const MethodInfo) -> i32;

/// `static void SetFloat(string key, float value)`
type SetFloatFn = unsafe extern "C" fn(*mut Il2CppString, f32, *const MethodInfo);

/// `static float GetFloat(string key, float defaultValue)`
type GetFloatFn = unsafe extern "C" fn(*mut Il2CppString, f32, *const MethodInfo) -> f32;

/// `static bool HasKey(string key)`
type HasKeyFn = unsafe extern "C" fn(*mut Il2CppString, *const MethodInfo) -> i32;

/// `static void DeleteKey(string key)`
type DeleteKeyFn = unsafe extern "C" fn(*mut Il2CppString, *const MethodInfo);

// ---- Sensitive key detection ----------------------------------------------

/// Substrings that indicate a key holds sensitive data (token, password, etc.).
/// Matched case-insensitively against the PlayerPrefs key.
const SENSITIVE_PATTERNS: &[&str] = &[
    "token", "auth", "session", "password", "secret", "credential",
];

/// Maximum number of characters to log for sensitive values.
const SENSITIVE_MAX_LEN: usize = 20;

/// Sanitize a value for logging. Sensitive keys get their values truncated.
fn sanitise_value(key: &str, value: &str) -> String {
    let key_lower = key.to_ascii_lowercase();
    let is_sensitive = SENSITIVE_PATTERNS.iter().any(|p| key_lower.contains(p));
    if is_sensitive && value.len() > SENSITIVE_MAX_LEN {
        format!("{}...", &value[..SENSITIVE_MAX_LEN])
    } else {
        value.to_string()
    }
}

/// Convert an `Il2CppString` pointer to a displayable string for logging.
fn display_string(ptr: *const Il2CppString) -> String {
    if ptr.is_null() {
        return "<null>".to_string();
    }
    unsafe { Il2CppString::to_rust_string(ptr) }.unwrap_or_else(|| "<invalid>".to_string())
}

/// Create an `Il2CppString` from a Rust string using the IL2CPP API.
///
/// Returns null if the API is not available.
fn make_il2cpp_string(value: &str) -> *mut Il2CppString {
    let Some(api) = super::il2cpp_init::IL2CPP_API.get() else {
        return std::ptr::null_mut();
    };
    let c_str = std::ffi::CString::new(value).unwrap_or_default();
    unsafe { (api.string_new)(c_str.as_ptr()) }
}

/// Whether the Registry fallthrough is blocked (NewAccount or Known profile mode).
///
/// When blocked, hooks return empty/default values instead of calling the original
/// PlayerPrefs function for keys not in the store.
fn registry_blocked() -> bool {
    profile_store::should_block_registry()
}

// ---- Hook callbacks -------------------------------------------------------

/// Hook for `PlayerPrefs.SetString(string key, string value)`.
///
/// Routed keys are written to the profile store only (Registry untouched).
/// Unrouted keys pass through to the original and are logged for analysis.
/// In panic, it always falls back to the original to prevent data loss.
extern "C" fn hook_set_string(
    key: *mut Il2CppString,
    value: *mut Il2CppString,
    method_info: *const MethodInfo,
) {
    let original: SetStringFn =
        unsafe { std::mem::transmute(ORIGINAL_SET_STRING.load(Relaxed)) };

    if !HOOK_SET.is_active() {
        unsafe { original(key, value, method_info) };
        return;
    }

    // Try to determine if the key is routed BEFORE doing anything else.
    // If our logic panics at any point, the catch_unwind returns false,
    // and we fall through to calling the original (safe fallback).
    let handled = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let k = display_string(key);
        let v = display_string(value);

        if profile_store::is_routed(&k) {
            let known = profile_store::record(&k, &v);
            if !known {
                debug!(target: "PlayerPrefs", "STORE SET \"{k}\"");
            }
            return true; // handled, don't call original
        }
        false // not handled, the caller should call original
    }));

    match handled {
        Ok(true) => {} // routed key, stored successfully
        Ok(false) => {
            // Unrouted key: pass through to Registry and log
            unsafe { original(key, value, method_info) };
            if let Ok(()) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let k = display_string(key);
                let v = display_string(value);
                let v_safe = sanitise_value(&k, &v);
                info!(target: "PlayerPrefs", "NEW SET \"{k}\" = \"{v_safe}\"");
            })) {}
        }
        Err(_) => {
            // Panic: fall back to the original to prevent data loss
            HOOK_SET.record_error();
            unsafe { original(key, value, method_info) };
        }
    }
}

/// Hook for `PlayerPrefs.GetString(string key, string defaultValue)`.
///
/// Routed keys with a stored value are returned directly from the profile
/// store without touching the Registry. Unknown routed keys fall through
/// to the original and are captured for next time.
extern "C" fn hook_get_string_2(
    key: *mut Il2CppString,
    default_value: *mut Il2CppString,
    method_info: *const MethodInfo,
) -> *mut Il2CppString {
    if HOOK_GET2.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);

            // Try store first
            if let Some(stored) = profile_store::get(&k) {
                return make_il2cpp_string(&stored);
            }

            // Block Registry in NewAccount/Known modes
            if registry_blocked() {
                return default_value;
            }

            // Import mode: fall through to Registry
            let original: GetString2Fn =
                unsafe { std::mem::transmute(ORIGINAL_GET_STRING_2.load(Relaxed)) };
            let result = unsafe { original(key, default_value, method_info) };

            let v = display_string(result);
            let known = profile_store::record(&k, &v);
            if !known {
                let v_safe = sanitise_value(&k, &v);
                info!(target: "PlayerPrefs", "NEW GET \"{k}\" -> \"{v_safe}\"");
            }
            result
        }));
        match result {
            Ok(ptr) => return ptr,
            Err(_) => HOOK_GET2.record_error(),
        }
    }

    let original: GetString2Fn =
        unsafe { std::mem::transmute(ORIGINAL_GET_STRING_2.load(Relaxed)) };
    unsafe { original(key, default_value, method_info) }
}

/// Hook for `PlayerPrefs.GetString(string key)` (no default parameter).
///
/// Same logic as `hook_get_string_2` but without the default value parameter.
extern "C" fn hook_get_string_1(
    key: *mut Il2CppString,
    method_info: *const MethodInfo,
) -> *mut Il2CppString {
    if HOOK_GET1.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);

            if let Some(stored) = profile_store::get(&k) {
                return make_il2cpp_string(&stored);
            }

            if registry_blocked() {
                return make_il2cpp_string("");
            }

            let original: GetString1Fn =
                unsafe { std::mem::transmute(ORIGINAL_GET_STRING_1.load(Relaxed)) };
            let result = unsafe { original(key, method_info) };

            let v = display_string(result);
            let known = profile_store::record(&k, &v);
            if !known {
                let v_safe = sanitise_value(&k, &v);
                info!(target: "PlayerPrefs", "NEW GET \"{k}\" -> \"{v_safe}\"");
            }
            result
        }));
        match result {
            Ok(ptr) => return ptr,
            Err(_) => HOOK_GET1.record_error(),
        }
    }

    let original: GetString1Fn =
        unsafe { std::mem::transmute(ORIGINAL_GET_STRING_1.load(Relaxed)) };
    unsafe { original(key, method_info) }
}

// ---- Int/Float/HasKey/DeleteKey hooks (logging only) ----------------------

/// Hook for `PlayerPrefs.SetInt(string key, int value)`.
extern "C" fn hook_set_int(key: *mut Il2CppString, value: i32, method_info: *const MethodInfo) {
    let original: SetIntFn = unsafe { std::mem::transmute(ORIGINAL_SET_INT.load(Relaxed)) };

    if !HOOK_SET_INT.is_active() {
        unsafe { original(key, value, method_info) };
        return;
    }

    let handled = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let k = display_string(key);
        if profile_store::is_routed(&k) {
            let known = profile_store::record_int(&k, value);
            if !known {
                debug!(target: "PlayerPrefs", "STORE SET_INT \"{k}\" = {value}");
            }
            return true;
        }
        false
    }));

    match handled {
        Ok(true) => {}
        Ok(false) => {
            unsafe { original(key, value, method_info) };
            if let Ok(()) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let k = display_string(key);
                info!(target: "PlayerPrefs", "NEW SET_INT \"{k}\" = {value}");
            })) {}
        }
        Err(_) => {
            HOOK_SET_INT.record_error();
            unsafe { original(key, value, method_info) };
        }
    }
}

/// Hook for `PlayerPrefs.GetInt(string key, int defaultValue)`.
extern "C" fn hook_get_int(
    key: *mut Il2CppString,
    default_value: i32,
    method_info: *const MethodInfo,
) -> i32 {
    if HOOK_GET_INT.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);

            if let Some(stored) = profile_store::get_int(&k) {
                return stored;
            }

            if registry_blocked() {
                return default_value;
            }

            let original: GetIntFn = unsafe { std::mem::transmute(ORIGINAL_GET_INT.load(Relaxed)) };
            let result = unsafe { original(key, default_value, method_info) };

            let known = profile_store::record_int(&k, result);
            if !known {
                info!(target: "PlayerPrefs", "NEW GET_INT \"{k}\" -> {result}");
            }
            result
        }));
        match result {
            Ok(val) => return val,
            Err(_) => HOOK_GET_INT.record_error(),
        }
    }

    let original: GetIntFn = unsafe { std::mem::transmute(ORIGINAL_GET_INT.load(Relaxed)) };
    unsafe { original(key, default_value, method_info) }
}

/// Hook for `PlayerPrefs.SetFloat(string key, float value)`.
extern "C" fn hook_set_float(key: *mut Il2CppString, value: f32, method_info: *const MethodInfo) {
    let original: SetFloatFn = unsafe { std::mem::transmute(ORIGINAL_SET_FLOAT.load(Relaxed)) };

    if !HOOK_SET_FLOAT.is_active() {
        unsafe { original(key, value, method_info) };
        return;
    }

    let handled = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let k = display_string(key);
        if profile_store::is_routed(&k) {
            let known = profile_store::record_float(&k, value);
            if !known {
                debug!(target: "PlayerPrefs", "STORE SET_FLOAT \"{k}\" = {value}");
            }
            return true;
        }
        false
    }));

    match handled {
        Ok(true) => {}
        Ok(false) => {
            unsafe { original(key, value, method_info) };
            if let Ok(()) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let k = display_string(key);
                info!(target: "PlayerPrefs", "NEW SET_FLOAT \"{k}\" = {value}");
            })) {}
        }
        Err(_) => {
            HOOK_SET_FLOAT.record_error();
            unsafe { original(key, value, method_info) };
        }
    }
}

/// Hook for `PlayerPrefs.GetFloat(string key, float defaultValue)`.
extern "C" fn hook_get_float(
    key: *mut Il2CppString,
    default_value: f32,
    method_info: *const MethodInfo,
) -> f32 {
    if HOOK_GET_FLOAT.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);

            if let Some(stored) = profile_store::get_float(&k) {
                return stored;
            }

            if registry_blocked() {
                return default_value;
            }

            let original: GetFloatFn =
                unsafe { std::mem::transmute(ORIGINAL_GET_FLOAT.load(Relaxed)) };
            let result = unsafe { original(key, default_value, method_info) };

            let known = profile_store::record_float(&k, result);
            if !known {
                info!(target: "PlayerPrefs", "NEW GET_FLOAT \"{k}\" -> {result}");
            }
            result
        }));
        match result {
            Ok(val) => return val,
            Err(_) => HOOK_GET_FLOAT.record_error(),
        }
    }

    let original: GetFloatFn = unsafe { std::mem::transmute(ORIGINAL_GET_FLOAT.load(Relaxed)) };
    unsafe { original(key, default_value, method_info) }
}

/// Hook for `PlayerPrefs.HasKey(string key)`.
///
/// If the key is routed and exists in the store, it returns `true` without
/// touching the Registry. Otherwise, falls through to the original.
extern "C" fn hook_has_key(key: *mut Il2CppString, method_info: *const MethodInfo) -> i32 {
    if HOOK_HAS_KEY.is_active() {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);

            // Check the store first for routed keys
            if profile_store::is_routed(&k) {
                // Key is routed: check if we have a value stored
                let exists = profile_store::get(&k).is_some()
                    || profile_store::get_int(&k).is_some()
                    || profile_store::get_float(&k).is_some();
                return exists as i32;
            }

            // Unrouted: fall through to Registry
            let original: HasKeyFn = unsafe { std::mem::transmute(ORIGINAL_HAS_KEY.load(Relaxed)) };
            let result = unsafe { original(key, method_info) };
            let exists = result != 0;
            info!(target: "PlayerPrefs", "NEW HAS_KEY \"{k}\" -> {exists}");
            result
        }));
        match result {
            Ok(val) => return val,
            Err(_) => HOOK_HAS_KEY.record_error(),
        }
    }

    let original: HasKeyFn = unsafe { std::mem::transmute(ORIGINAL_HAS_KEY.load(Relaxed)) };
    unsafe { original(key, method_info) }
}

/// Hook for `PlayerPrefs.DeleteKey(string key)`.
///
/// Removes the key from the profile store. Also calls the original to keep
/// the Registry in sync (harmless and prevents stale values if the mod is removed).
extern "C" fn hook_delete_key(key: *mut Il2CppString, method_info: *const MethodInfo) {
    let original: DeleteKeyFn = unsafe { std::mem::transmute(ORIGINAL_DELETE_KEY.load(Relaxed)) };
    unsafe { original(key, method_info) };

    if HOOK_DELETE_KEY.is_active() {
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let k = display_string(key);
            profile_store::delete(&k);
            debug!(target: "PlayerPrefs", "DELETE \"{k}\"");
        }));
        if caught.is_err() {
            HOOK_DELETE_KEY.record_error();
        }
    }
}

// ---- Hook installation ----------------------------------------------------

/// Install a single hook, storing the original function pointer.
///
/// Logs success or failure without blocking other hooks.
fn install_single(
    class: *mut Il2CppClass,
    api: &Il2CppApi,
    method_name: &str,
    param_count: i32,
    display_name: &str,
    replacement: *const (),
    original: &AtomicPtr<()>,
) {
    let Some(method) = resolver::resolve_method(api, class, method_name, param_count) else {
        warn!(target: "PlayerPrefs", "{display_name} method not found, hook skipped");
        return;
    };

    let target = unsafe { (*method).method_pointer };
    match engine::install_hook(display_name, target, replacement) {
        Ok(orig) => {
            original.store(orig as *mut (), Relaxed);
            debug!(target: "PlayerPrefs", "{display_name} hook installed");
        }
        Err(e) => {
            error!(target: "PlayerPrefs", "Failed to hook {display_name}: {e}");
        }
    }
}

/// Install PlayerPrefs.GetString and SetString logging hooks.
///
/// Called from `install_all_hooks()` after IL2CPP is initialized. Uses the IL2CPP
/// reflection API to resolve the methods, so no hardcoded RVAs are needed.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "UnityEngine.CoreModule", "UnityEngine", "PlayerPrefs",
    ) else {
        warn!(target: "PlayerPrefs", "PlayerPrefs class not found, hooks skipped");
        return;
    };

    install_single(
        class, api, "SetString", 2, "PlayerPrefs.SetString",
        hook_set_string as *const (), &ORIGINAL_SET_STRING,
    );

    install_single(
        class, api, "GetString", 2, "PlayerPrefs.GetString/2",
        hook_get_string_2 as *const (), &ORIGINAL_GET_STRING_2,
    );

    install_single(
        class, api, "GetString", 1, "PlayerPrefs.GetString/1",
        hook_get_string_1 as *const (), &ORIGINAL_GET_STRING_1,
    );

    install_single(
        class, api, "SetInt", 2, "PlayerPrefs.SetInt",
        hook_set_int as *const (), &ORIGINAL_SET_INT,
    );

    install_single(
        class, api, "GetInt", 2, "PlayerPrefs.GetInt",
        hook_get_int as *const (), &ORIGINAL_GET_INT,
    );

    install_single(
        class, api, "SetFloat", 2, "PlayerPrefs.SetFloat",
        hook_set_float as *const (), &ORIGINAL_SET_FLOAT,
    );

    install_single(
        class, api, "GetFloat", 2, "PlayerPrefs.GetFloat",
        hook_get_float as *const (), &ORIGINAL_GET_FLOAT,
    );

    install_single(
        class, api, "HasKey", 1, "PlayerPrefs.HasKey",
        hook_has_key as *const (), &ORIGINAL_HAS_KEY,
    );

    install_single(
        class, api, "DeleteKey", 1, "PlayerPrefs.DeleteKey",
        hook_delete_key as *const (), &ORIGINAL_DELETE_KEY,
    );
}
