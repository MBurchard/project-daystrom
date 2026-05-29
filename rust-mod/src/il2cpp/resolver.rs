use std::ffi::CString;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::warn;

use super::api::Il2CppApi;
use super::types::*;

/// Resolve an IL2CPP class by assembly name, namespace, and class name.
///
/// Walks the chain: domain → assembly → image → class. Returns `None` and logs a warning if any step fails.
/// This is expected when a game update renames or removes a class.
pub fn resolve_class(api: &Il2CppApi, assembly: &str, namespace: &str, class_name: &str) -> Option<*mut Il2CppClass> {
    let c_assembly = CString::new(assembly).unwrap_or_else(|_| {
        warn!(target: "IL2CPP", "Invalid assembly name (contains null byte): {assembly}");
        CString::default()
    });
    let c_namespace = CString::new(namespace).unwrap_or_else(|_| {
        warn!(target: "IL2CPP", "Invalid namespace (contains null byte): {namespace}");
        CString::default()
    });
    let c_class = CString::new(class_name).unwrap_or_else(|_| {
        warn!(target: "IL2CPP", "Invalid class name (contains null byte): {class_name}");
        CString::default()
    });

    unsafe {
        let domain = (api.domain_get)();
        if domain.is_null() {
            warn!(target: "IL2CPP", "il2cpp_domain_get() returned null");
            return None;
        }

        let asm = (api.domain_assembly_open)(domain, c_assembly.as_ptr());
        if asm.is_null() {
            warn!(target: "IL2CPP", "Assembly not found: {assembly}");
            return None;
        }

        let image = (api.assembly_get_image)(asm);
        if image.is_null() {
            warn!(target: "IL2CPP", "Image not found for assembly: {assembly}");
            return None;
        }

        let class = (api.class_from_name)(image, c_namespace.as_ptr(), c_class.as_ptr());
        if class.is_null() {
            warn!(target: "IL2CPP", "Class not found: {namespace}.{class_name} in {assembly}");
            return None;
        }

        Some(class)
    }
}

/// Assemblies that ship the PrimeServer classes, ordered by how recent game builds package them.
const PRIME_ASSEMBLIES: &[&str] = &["Digit.Client.PrimeLib.Runtime", "Assembly-CSharp", "Assembly-CSharp-firstpass"];

/// Resolve a class from a PrimeServer namespace, searching the known Prime assemblies in order.
///
/// Different game builds place these classes in different assemblies, so this returns the first match.
pub fn resolve_prime_class(api: &Il2CppApi, namespace: &str, class_name: &str) -> Option<*mut Il2CppClass> {
    PRIME_ASSEMBLIES
        .iter()
        .find_map(|assembly| resolve_class(api, assembly, namespace, class_name))
}

/// Resolve a class from the `Digit.PrimeServer.Models` namespace across the known Prime assemblies.
pub fn resolve_prime_model_class(api: &Il2CppApi, class_name: &str) -> Option<*mut Il2CppClass> {
    resolve_prime_class(api, "Digit.PrimeServer.Models", class_name)
}

/// Resolve a field's byte offset within an IL2CPP class by name.
///
/// Returns the offset suitable for pointer arithmetic on object instances.
/// Returns `None` and logs a warning if the field is not found.
pub fn resolve_field_offset(api: &Il2CppApi, class: *mut Il2CppClass, field_name: &str) -> Option<usize> {
    let c_field = CString::new(field_name).unwrap_or_else(|_| {
        warn!(target: "IL2CPP", "Invalid field name (contains null byte): {field_name}");
        CString::default()
    });

    unsafe {
        let field = (api.class_get_field_from_name)(class, c_field.as_ptr());
        if field.is_null() {
            warn!(target: "IL2CPP", "Field not found: {field_name}");
            return None;
        }

        let offset = (api.field_get_offset)(field);
        Some(offset)
    }
}

/// Resolve a field offset and cache it.
///
/// Returns `false` when resolution fails. Failure details are logged by `resolve_field_offset`.
pub fn resolve_field_offset_into(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    field_name: &str,
    target: &AtomicUsize,
) -> bool {
    let Some(offset) = resolve_field_offset(api, class, field_name) else {
        return false;
    };

    target.store(offset, Relaxed);
    true
}

/// Resolve a method on an IL2CPP class by name and parameter count.
///
/// Returns the `MethodInfo` including the raw function pointer for hooking.
/// Returns `None` and logs a warning if the method is not found.
/// Pass `-1` as `param_count` to match any overload.
pub fn resolve_method(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
) -> Option<*const MethodInfo> {
    let c_method = CString::new(method_name).unwrap_or_else(|_| {
        warn!(target: "IL2CPP", "Invalid method name (contains null byte): {method_name}");
        CString::default()
    });

    unsafe {
        let method = (api.class_get_method_from_name)(class, c_method.as_ptr(), param_count);
        if method.is_null() {
            warn!(target: "IL2CPP", "Method not found: {method_name} (params: {param_count})");
            return None;
        }

        let method_ptr = (*method).method_pointer;
        if method_ptr.is_null() {
            warn!(target: "IL2CPP", "Method {method_name} has null method_pointer");
            return None;
        }

        Some(method)
    }
}

/// Resolve a method and cache its `MethodInfo` pointer.
///
/// Returns `false` when resolution fails. Failure details are logged by `resolve_method`.
pub fn resolve_method_into(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    target: &AtomicPtr<MethodInfo>,
) -> bool {
    let Some(method) = resolve_method(api, class, method_name, param_count) else {
        return false;
    };

    target.store(method as *mut MethodInfo, Relaxed);
    true
}
