use std::ffi::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

use libloading::Library;
use log::{debug, error};

use crate::hook::engine;
use crate::il2cpp::api::{self, Il2CppApi, Il2CppInitFn};

/// Global IL2CPP API handle, initialized once after `il2cpp_init` completes.
pub static IL2CPP_API: OnceLock<Il2CppApi> = OnceLock::new();

/// Original `il2cpp_init` function pointer (set by the hook installer).
static ORIGINAL: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Loaded GameAssembly library handle. Kept alive for the entire process lifetime so that all
/// IL2CPP symbols remain valid.
static GAME_ASSEMBLY: OnceLock<Library> = OnceLock::new();

/// Hook for `il2cpp_init`. Called once during game startup.
///
/// Calls the original first (IL2CPP must initialize before we can use the reflection API),
/// then loads all IL2CPP API functions and installs the game hooks.
extern "C" fn il2cpp_init_hook(domain_name: *const c_char) -> i64 {
    // Always call original first
    let original: Il2CppInitFn = unsafe { std::mem::transmute(ORIGINAL.load(Relaxed)) };
    let result = unsafe { original(domain_name) };

    debug!(target: "HookEngine", "il2cpp_init completed, loading IL2CPP API...");

    match api::load() {
        Ok(api) => {
            IL2CPP_API.set(api).ok();
            debug!(target: "HookEngine", "IL2CPP API loaded, installing game hooks...");
            if catch_unwind(AssertUnwindSafe(super::install_all_hooks)).is_err() {
                error!(target: "HookEngine", "Hook installation panicked; continuing startup");
            }
        }
        Err(e) => {
            error!(target: "HookEngine", "Failed to load IL2CPP API: {e}");
        }
    }

    result
}

/// Get a reference to the loaded GameAssembly library handle.
///
/// Returns `None` if `install()` hasn't been called yet.
pub fn game_assembly() -> Option<&'static Library> {
    GAME_ASSEMBLY.get()
}

/// Load the GameAssembly library, find `il2cpp_init`, and install our hook on it.
///
/// This runs from `#[ctor]` before `main()`. The GameAssembly is not yet loaded by the game at
/// this point, so we load it ourselves via `libloading`. The OS reference-counts library handles,
/// so when the game loads it later, it gets the same instance.
pub fn install() -> Result<(), String> {
    let lib = load_game_assembly()?;
    let target = find_symbol(&lib, "il2cpp_init")?;

    // Keep the library handle alive for the entire process
    GAME_ASSEMBLY.set(lib).ok();

    match engine::install_hook("il2cpp_init", target, il2cpp_init_hook as *const ()) {
        Ok(original) => {
            ORIGINAL.store(original as *mut (), Relaxed);
            Ok(())
        }
        Err(e) => Err(format!("Failed to hook il2cpp_init: {e}")),
    }
}

/// Load the GameAssembly shared library into the process.
///
/// On macOS: path relative to the game executable (`../Frameworks/GameAssembly.dylib`).
/// On Windows: `GameAssembly.dll` from the application directory.
fn load_game_assembly() -> Result<Library, String> {
    #[cfg(target_os = "macos")]
    {
        let exe_path = macos_executable_path().ok_or("Could not determine executable path")?;
        let lib_path = std::path::Path::new(&exe_path)
            .parent()
            .ok_or("Invalid executable path")?
            .join("../Frameworks/GameAssembly.dylib");
        debug!(target: "HookEngine", "Loading GameAssembly from {}", lib_path.display());
        unsafe { Library::new(&lib_path) }.map_err(|e| format!("Failed to load GameAssembly: {e}"))
    }

    #[cfg(target_os = "windows")]
    {
        debug!(target: "HookEngine", "Loading GameAssembly.dll");
        unsafe { Library::new("GameAssembly.dll") }.map_err(|e| format!("Failed to load GameAssembly.dll: {e}"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Unsupported platform".to_string())
    }
}

/// Resolve a symbol from the loaded library and return it as a raw pointer.
fn find_symbol(lib: &Library, name: &str) -> Result<*const (), String> {
    let c_name = std::ffi::CString::new(name).map_err(|_| format!("Invalid symbol name: {name}"))?;
    let sym = unsafe { lib.get::<*const ()>(c_name.as_bytes_with_nul()) }
        .map_err(|e| format!("Symbol '{name}' not found: {e}"))?;
    Ok(*sym)
}

/// Get the path to the currently running executable on macOS.
#[cfg(target_os = "macos")]
fn macos_executable_path() -> Option<String> {
    use std::ffi::CStr;

    unsafe extern "C" {
        fn _NSGetExecutablePath(buf: *mut c_char, bufsize: *mut u32) -> i32;
    }

    let mut buf = vec![0u8; 1024];
    let mut bufsize = buf.len() as u32;
    let result = unsafe { _NSGetExecutablePath(buf.as_mut_ptr() as *mut c_char, &mut bufsize) };
    if result != 0 {
        return None;
    }
    let c_str = unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) };
    c_str.to_str().ok().map(|s| s.to_string())
}
