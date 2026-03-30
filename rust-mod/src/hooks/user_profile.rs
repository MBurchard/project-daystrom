use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};
use std::sync::OnceLock;
use std::time::Duration;

use log::{debug, error, warn};

use crate::hook::engine;
use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;
/// Original function pointer: `UserProfile* GetLocalUserProfile(UserProfileManager* this)`.
static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("GetLocalUserProfile");

/// Thread-safe wrapper for a raw IL2CPP class pointer.
///
/// Raw pointers are not `Send`/`Sync`, but IL2CPP class pointers are stable for the entire process
/// lifetime and only read after initialization. This wrapper is safe because the pointer is set
/// once and never mutated.
struct ClassPtr(*mut Il2CppClass);
unsafe impl Send for ClassPtr {}
unsafe impl Sync for ClassPtr {}

/// Cached UserProfile class pointer, resolved once on the first call.
static PROFILE_CLASS: OnceLock<Option<ClassPtr>> = OnceLock::new();

/// Type alias for the original function signature.
type GetLocalUserProfileFn = unsafe extern "C" fn(*mut Il2CppObject) -> *mut Il2CppObject;

/// Assemblies to search for the UserProfile class.
///
/// IL2CppDumper shows the namespace `Digit.PrimeServer.Models` but doesn't reveal the assembly name. We try
/// the most likely candidates in order.
const PROFILE_ASSEMBLIES: &[&str] = &[
    "Digit.Client.PrimeLib.Runtime",
    "Assembly-CSharp",
    "Assembly-CSharp-firstpass",
];

/// Hook replacement for `UserProfileManager.GetLocalUserProfile()`.
///
/// Always calls the original function and returns its result. Our custom logic (reading player
/// data) runs inside `catch_unwind` so a panic never propagates across the FFI boundary.
extern "C" fn hook(this: *mut Il2CppObject) -> *mut Il2CppObject {
    // Always call original first.
    let orig_ptr = ORIGINAL.load(Relaxed);
    let result = if !orig_ptr.is_null() {
        let original: GetLocalUserProfileFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this) }
    } else {
        std::ptr::null_mut()
    };

    // Run our logic only if the hook is still active and we got a result
    if HOOK_INFO.is_active() && !result.is_null() {
        let caught = std::panic::catch_unwind(AssertUnwindSafe(|| {
            log_player_info(result);
        }));
        if caught.is_err() {
            HOOK_INFO.record_error();
        }
    }

    result
}

/// Resolve the UserProfile class, trying multiple assemblies. Cached after first successful resolution.
/// Returns None permanently after the first failed attempt (to avoid repeated lookups).
fn resolve_profile_class(api: &Il2CppApi) -> Option<*mut Il2CppClass> {
    PROFILE_CLASS.get_or_init(|| {
        for assembly in PROFILE_ASSEMBLIES {
            let class = resolver::resolve_class(api, assembly, "Digit.PrimeServer.Models", "UserProfile");
            if let Some(ptr) = class {
                debug!(target: "PlayerData", "UserProfile class found in assembly '{assembly}'");
                return Some(ClassPtr(ptr));
            }
        }
        warn!(target: "PlayerData", "UserProfile class not found in any assembly, player data logging disabled");
        None
    }).as_ref().map(|c| c.0)
}

/// Read player information from a UserProfile object and push changes to the game state.
///
/// Uses IL2CPP runtime_invoke to call the property getters on the returned object. Throttled to
/// once every 10 seconds to avoid unnecessary reflection calls.
fn log_player_info(profile: *mut Il2CppObject) {
    // Check throttle first to avoid unnecessary reflection calls
    if !crate::throttle::should_log("user_profile", Duration::from_secs(10)) {
        return;
    }

    let Some(api) = super::il2cpp_init::IL2CPP_API.get() else {
        return;
    };
    let Some(class) = resolve_profile_class(api) else {
        return;
    };

    let name = read_string_property(api, class, profile, "get_Name");
    let level = read_int_property(api, class, profile, "get_Level");
    let might = read_ulong_property(api, class, profile, "get_MilitaryMight");

    // Keep the profile store in sync with live player data.
    // This catches values that change server-side without going through PlayerPrefs.
    // Skip in trace-only mode (no store active).
    if !super::is_trace_only() {
        if let Some(ref name) = name {
            crate::profile_store::record("social_username", name);
        }
        if let Some(level) = level {
            crate::profile_store::record_int("player_level", level);
        }
    }

    // Update game state (handles change detection, debug logging, and WS notification)
    crate::game_state::update_player(name, level, might);
}

/// Call a parameterless method that returns an `Il2CppString` and convert the result to a Rust `String`.
fn read_string_property(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    obj: *mut Il2CppObject,
    method_name: &str,
) -> Option<String> {
    let method = resolver::resolve_method(api, class, method_name, 0)?;
    let mut exception: *mut Il2CppException = std::ptr::null_mut();

    let result = unsafe {
        (api.runtime_invoke)(method, obj, std::ptr::null_mut(), &mut exception)
    };

    if !exception.is_null() || result.is_null() {
        return None;
    }

    unsafe { Il2CppString::to_rust_string(result as *const Il2CppString) }
}

/// Call a parameterless method that returns an `int` (unboxed from Il2CppObject).
fn read_int_property(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    obj: *mut Il2CppObject,
    method_name: &str,
) -> Option<i32> {
    let method = resolver::resolve_method(api, class, method_name, 0)?;
    let mut exception: *mut Il2CppException = std::ptr::null_mut();

    let result = unsafe {
        (api.runtime_invoke)(method, obj, std::ptr::null_mut(), &mut exception)
    };

    if !exception.is_null() || result.is_null() {
        return None;
    }

    // Value types are boxed: the value sits right after the Il2CppObject header (2 pointers)
    let value_ptr = unsafe { (result as *const u8).add(2 * size_of::<usize>()) as *const i32 };
    Some(unsafe { *value_ptr })
}

/// Call a parameterless method that returns an ` ulong ` (unboxed from Il2CppObject).
fn read_ulong_property(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    obj: *mut Il2CppObject,
    method_name: &str,
) -> Option<u64> {
    let method = resolver::resolve_method(api, class, method_name, 0)?;
    let mut exception: *mut Il2CppException = std::ptr::null_mut();

    let result = unsafe {
        (api.runtime_invoke)(method, obj, std::ptr::null_mut(), &mut exception)
    };

    if !exception.is_null() || result.is_null() {
        return None;
    }

    let value_ptr = unsafe { (result as *const u8).add(2 * size_of::<usize>()) as *const u64 };
    Some(unsafe { *value_ptr })
}

/// Install the GetLocalUserProfile hook via IL2CPP reflection.
///
/// Called from `install_all_hooks()` after IL2CPP is initialized. If the class or method cannot
/// be resolved (e.g. after a game update), logs a warning and returns without crashing.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(
        api, "Assembly-CSharp", "Digit.Prime.PlayerProfile", "UserProfileManager",
    ) else {
        return;
    };

    let Some(method) = resolver::resolve_method(api, class, "GetLocalUserProfile", 0) else {
        return;
    };

    let target = unsafe { (*method).method_pointer };
    match engine::install_hook("GetLocalUserProfile", target, hook as *const ()) {
        Ok(original) => {
            ORIGINAL.store(original as *mut (), Relaxed);
            debug!(target: "HookEngine", "GetLocalUserProfile hook installed");
        }
        Err(e) => {
            error!(target: "HookEngine", "Failed to hook GetLocalUserProfile: {e}");
        }
    }
}
