use std::ffi::c_char;

use libloading::{Library, Symbol};
use log::error;

use super::types::*;

// ---- Function pointer types -----------------------------------------------

/// Type alias for `il2cpp_init(domain_name) -> status`.
pub type Il2CppInitFn = unsafe extern "C" fn(*const c_char) -> i64;

/// Type alias for `il2cpp_domain_get() -> domain`.
type Il2CppDomainGetFn = unsafe extern "C" fn() -> *mut Il2CppDomain;

/// Type alias for `il2cpp_domain_assembly_open(domain, name) -> assembly`.
type Il2CppDomainAssemblyOpenFn = unsafe extern "C" fn(*mut Il2CppDomain, *const c_char) -> *mut Il2CppAssembly;

/// Type alias for `il2cpp_assembly_get_image(assembly) -> image`.
type Il2CppAssemblyGetImageFn = unsafe extern "C" fn(*mut Il2CppAssembly) -> *mut Il2CppImage;

/// Type alias for `il2cpp_class_from_name(image, namespace, name) -> class`.
type Il2CppClassFromNameFn = unsafe extern "C" fn(*mut Il2CppImage, *const c_char, *const c_char) -> *mut Il2CppClass;

/// Type alias for `il2cpp_class_get_method_from_name(class, name, argc) -> method_info`.
type Il2CppClassGetMethodFromNameFn = unsafe extern "C" fn(*mut Il2CppClass, *const c_char, i32) -> *const MethodInfo;

/// Type alias for `il2cpp_method_get_return_type(method) -> type`.
type Il2CppMethodGetReturnTypeFn = unsafe extern "C" fn(*const MethodInfo) -> *const Il2CppType;

/// Type alias for `il2cpp_method_get_param(method, index) -> type`.
type Il2CppMethodGetParamFn = unsafe extern "C" fn(*const MethodInfo, u32) -> *const Il2CppType;

/// Type alias for `il2cpp_method_get_flags(method, iflags) -> flags`.
type Il2CppMethodGetFlagsFn = unsafe extern "C" fn(*const MethodInfo, *mut u32) -> u32;

/// Type alias for `il2cpp_type_get_type(type) -> Il2CppTypeEnum`.
type Il2CppTypeGetTypeFn = unsafe extern "C" fn(*const Il2CppType) -> i32;

/// Type alias for `il2cpp_class_from_type(type) -> class`.
type Il2CppClassFromTypeFn = unsafe extern "C" fn(*const Il2CppType) -> *mut Il2CppClass;

/// Type alias for `il2cpp_class_is_valuetype(class) -> bool`.
type Il2CppClassIsValueTypeFn = unsafe extern "C" fn(*const Il2CppClass) -> bool;

/// Type alias for `il2cpp_object_get_class(object) -> class`.
type Il2CppObjectGetClassFn = unsafe extern "C" fn(*mut Il2CppObject) -> *mut Il2CppClass;

/// Type alias for `il2cpp_runtime_invoke(method, obj, params, exc) -> result`.
pub type Il2CppRuntimeInvokeFn = unsafe extern "C" fn(
    *const MethodInfo,
    *mut Il2CppObject,
    *mut *mut Il2CppObject,
    *mut *mut Il2CppException,
) -> *mut Il2CppObject;

/// Type alias for `il2cpp_class_get_field_from_name(class, name) -> field_info`.
type Il2CppClassGetFieldFromNameFn = unsafe extern "C" fn(*mut Il2CppClass, *const c_char) -> *mut FieldInfo;

/// Type alias for `il2cpp_field_get_offset(field) -> offset`.
type Il2CppFieldGetOffsetFn = unsafe extern "C" fn(*mut FieldInfo) -> usize;

/// Type alias for `il2cpp_string_new(str) -> Il2CppString*`.
type Il2CppStringNewFn = unsafe extern "C" fn(*const c_char) -> *mut Il2CppString;

// ---- IL2CPP API struct ----------------------------------------------------

/// All IL2CPP API functions resolved at runtime from the GameAssembly library.
///
/// All function pointers are guaranteed non-null after successful construction.
/// The library handle is kept alive separately in `hooks::il2cpp_init::GAME_ASSEMBLY`.
pub struct Il2CppApi {
    pub domain_get: Il2CppDomainGetFn,
    pub domain_assembly_open: Il2CppDomainAssemblyOpenFn,
    pub assembly_get_image: Il2CppAssemblyGetImageFn,
    pub class_from_name: Il2CppClassFromNameFn,
    pub class_get_method_from_name: Il2CppClassGetMethodFromNameFn,
    pub method_get_return_type: Il2CppMethodGetReturnTypeFn,
    pub method_get_param: Il2CppMethodGetParamFn,
    pub method_get_flags: Il2CppMethodGetFlagsFn,
    pub type_get_type: Il2CppTypeGetTypeFn,
    pub class_from_type: Il2CppClassFromTypeFn,
    pub class_is_valuetype: Il2CppClassIsValueTypeFn,
    pub object_get_class: Il2CppObjectGetClassFn,
    pub class_get_field_from_name: Il2CppClassGetFieldFromNameFn,
    pub field_get_offset: Il2CppFieldGetOffsetFn,
    pub runtime_invoke: Il2CppRuntimeInvokeFn,
    pub string_new: Il2CppStringNewFn,
}

// ---- Loading --------------------------------------------------------------

/// Error type for IL2CPP API loading failures.
#[derive(Debug)]
pub enum Il2CppError {
    /// The GameAssembly library handle is not available (il2cpp_init hook didn't run yet).
    LibraryNotLoaded,
    /// A required IL2CPP function symbol could not be resolved.
    SymbolNotFound(&'static str),
}

impl std::fmt::Display for Il2CppError {
    /// Format the error for logging.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LibraryNotLoaded => write!(f, "GameAssembly not loaded (il2cpp_init hook missing?)"),
            Self::SymbolNotFound(name) => write!(f, "IL2CPP symbol not found: {name}"),
        }
    }
}

/// Resolve a single function symbol from the loaded library.
///
/// Returns a typed function pointer or `Il2CppError::SymbolNotFound`.
unsafe fn resolve<T: Copy>(lib: &Library, name: &'static str) -> Result<T, Il2CppError> {
    let sym: Symbol<T> = unsafe { lib.get(name.as_bytes()) }.map_err(|_| {
        error!(target: "IL2CPP", "Symbol not found: {}", name);
        Il2CppError::SymbolNotFound(name)
    })?;
    Ok(*sym)
}

/// Resolve all IL2CPP API functions from the already-loaded GameAssembly library.
///
/// The library must have been loaded previously by `hooks::il2cpp_init::install()`.
/// Called from inside the `il2cpp_init` hook callback after IL2CPP has initialized.
pub fn load() -> Result<Il2CppApi, Il2CppError> {
    let lib = crate::hooks::il2cpp_init::game_assembly().ok_or(Il2CppError::LibraryNotLoaded)?;

    unsafe {
        Ok(Il2CppApi {
            domain_get: resolve(lib, "il2cpp_domain_get\0")?,
            domain_assembly_open: resolve(lib, "il2cpp_domain_assembly_open\0")?,
            assembly_get_image: resolve(lib, "il2cpp_assembly_get_image\0")?,
            class_from_name: resolve(lib, "il2cpp_class_from_name\0")?,
            class_get_method_from_name: resolve(lib, "il2cpp_class_get_method_from_name\0")?,
            method_get_return_type: resolve(lib, "il2cpp_method_get_return_type\0")?,
            method_get_param: resolve(lib, "il2cpp_method_get_param\0")?,
            method_get_flags: resolve(lib, "il2cpp_method_get_flags\0")?,
            type_get_type: resolve(lib, "il2cpp_type_get_type\0")?,
            class_from_type: resolve(lib, "il2cpp_class_from_type\0")?,
            class_is_valuetype: resolve(lib, "il2cpp_class_is_valuetype\0")?,
            object_get_class: resolve(lib, "il2cpp_object_get_class\0")?,
            class_get_field_from_name: resolve(lib, "il2cpp_class_get_field_from_name\0")?,
            field_get_offset: resolve(lib, "il2cpp_field_get_offset\0")?,
            runtime_invoke: resolve(lib, "il2cpp_runtime_invoke\0")?,
            string_new: resolve(lib, "il2cpp_string_new\0")?,
        })
    }
}
