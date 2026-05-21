use std::mem::size_of;

use log::warn;

use super::types::{Il2CppException, Il2CppObject, MethodInfo};

/// Invoke a no-argument IL2CPP instance method and return whether it completed without exception.
pub fn void(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> bool {
    invoke_raw(method, object, label).is_some()
}

/// Invoke a no-argument IL2CPP instance method returning `bool`.
pub fn bool(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<bool> {
    let result = invoke_raw(method, object, label)?;
    unsafe { unbox_value(result) }
}

fn invoke_raw(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<*mut Il2CppObject> {
    if method.is_null() || object.is_null() {
        return None;
    }

    let Some(api) = crate::hooks::il2cpp_init::IL2CPP_API.get() else {
        warn!(target: "IL2CPP", "{label}: IL2CPP API unavailable");
        return None;
    };

    let mut exception: *mut Il2CppException = std::ptr::null_mut();
    let result = unsafe { (api.runtime_invoke)(method, object, std::ptr::null_mut(), &mut exception) };
    if !exception.is_null() {
        warn!(target: "IL2CPP", "{label}: IL2CPP invocation raised an exception");
        return None;
    }

    Some(result)
}

unsafe fn unbox_value<T: Copy>(object: *mut Il2CppObject) -> Option<T> {
    if object.is_null() {
        return None;
    }

    let value_offset = 2 * size_of::<*const ()>();
    let value_ptr = unsafe { (object as *const u8).add(value_offset) } as *const T;
    Some(unsafe { *value_ptr })
}
