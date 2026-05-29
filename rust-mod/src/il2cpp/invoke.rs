use std::mem::size_of;

use log::warn;

use super::types::{Il2CppException, Il2CppObject, Il2CppString, MethodInfo};

/// Invoke a no-argument IL2CPP instance method and return whether it completed without exception.
pub fn void(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> bool {
    invoke_raw(method, object, label).is_some()
}

/// Invoke a no-argument IL2CPP instance method returning `bool`.
pub fn bool(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<bool> {
    value(method, object, label)
}

/// Invoke a no-argument IL2CPP instance method returning `i32`.
pub fn i32(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<i32> {
    value(method, object, label)
}

/// Invoke a no-argument IL2CPP instance method returning `i64`.
#[allow(dead_code)]
pub fn i64(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<i64> {
    value(method, object, label)
}

/// Invoke a no-argument IL2CPP instance method returning `u64`.
pub fn u64(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<u64> {
    value(method, object, label)
}

/// Invoke a no-argument IL2CPP instance method returning `f32`.
pub fn f32(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<f32> {
    value(method, object, label)
}

/// Invoke a no-argument IL2CPP instance method returning a value type.
pub fn value<T: Copy>(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<T> {
    let result = invoke_raw(method, object, label)?;
    unsafe { unbox_value(result) }
}

/// Invoke a no-argument IL2CPP instance method returning an object reference.
pub fn object(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<*mut Il2CppObject> {
    let result = invoke_raw(method, object, label)?;
    (!result.is_null()).then_some(result)
}

/// Invoke a no-argument IL2CPP instance method returning a string reference.
pub fn string(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<String> {
    let result = invoke_raw(method, object, label)?;
    unsafe { Il2CppString::to_rust_string(result as *const Il2CppString) }
}

/// Invoke a no-argument IL2CPP static method returning `bool`.
pub fn static_bool(method: *const MethodInfo, label: &str) -> Option<bool> {
    let result = invoke_static_raw(method, label)?;
    unsafe { unbox_value(result) }
}

/// Invoke an IL2CPP instance method with one `bool` argument and no return value.
pub fn void_bool(method: *const MethodInfo, object: *mut Il2CppObject, arg: bool, label: &str) -> bool {
    let mut arg = arg;
    let mut args = [(&mut arg as *mut bool).cast::<Il2CppObject>()];
    invoke_raw_with_args(method, object, args.as_mut_ptr(), label).is_some()
}

/// Invoke an IL2CPP instance method with one `i64` argument returning `bool`.
pub fn bool_i64(method: *const MethodInfo, object: *mut Il2CppObject, arg: i64, label: &str) -> Option<bool> {
    let mut arg = arg;
    let mut args = [(&mut arg as *mut i64).cast::<Il2CppObject>()];
    let result = invoke_raw_with_args(method, object, args.as_mut_ptr(), label)?;
    unsafe { unbox_value(result) }
}

/// Invoke an IL2CPP instance method with one `f32` argument and no return value.
pub fn void_f32(method: *const MethodInfo, object: *mut Il2CppObject, arg: f32, label: &str) -> bool {
    let mut arg = arg;
    let mut args = [(&mut arg as *mut f32).cast::<Il2CppObject>()];
    invoke_raw_with_args(method, object, args.as_mut_ptr(), label).is_some()
}

/// Invoke an IL2CPP instance method with two `f32` arguments and no return value.
pub fn void_f32_f32(method: *const MethodInfo, object: *mut Il2CppObject, arg0: f32, arg1: f32, label: &str) -> bool {
    let mut arg0 = arg0;
    let mut arg1 = arg1;
    let mut args = [(&mut arg0 as *mut f32).cast::<Il2CppObject>(), (&mut arg1 as *mut f32).cast::<Il2CppObject>()];
    invoke_raw_with_args(method, object, args.as_mut_ptr(), label).is_some()
}

fn invoke_raw(method: *const MethodInfo, object: *mut Il2CppObject, label: &str) -> Option<*mut Il2CppObject> {
    invoke_raw_with_args(method, object, std::ptr::null_mut(), label)
}

fn invoke_static_raw(method: *const MethodInfo, label: &str) -> Option<*mut Il2CppObject> {
    invoke_raw_allow_static(method, std::ptr::null_mut(), std::ptr::null_mut(), label)
}

fn invoke_raw_with_args(
    method: *const MethodInfo,
    object: *mut Il2CppObject,
    args: *mut *mut Il2CppObject,
    label: &str,
) -> Option<*mut Il2CppObject> {
    if method.is_null() || object.is_null() {
        return None;
    }

    invoke_raw_allow_static(method, object, args, label)
}

fn invoke_raw_allow_static(
    method: *const MethodInfo,
    object: *mut Il2CppObject,
    args: *mut *mut Il2CppObject,
    label: &str,
) -> Option<*mut Il2CppObject> {
    if method.is_null() {
        return None;
    }

    let Some(api) = crate::hooks::il2cpp_init::IL2CPP_API.get() else {
        warn!(target: "IL2CPP", "{label}: IL2CPP API unavailable");
        return None;
    };

    let mut exception: *mut Il2CppException = std::ptr::null_mut();
    let result = unsafe { (api.runtime_invoke)(method, object, args, &mut exception) };
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
