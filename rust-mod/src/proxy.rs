//! Windows DLL proxy for `version.dll`.
//!
//! When the game loads `version.dll` from its directory (our mod), it expects all the
//! standard Version API functions. This module loads the real system `version.dll` from
//! `C:\Windows\System32\` and forwards every call to it.
//!
//! The `.def` file (linked via `build.rs`) tells the linker to export these symbols.
//! Each exported function resolves the real function pointer on first call via `OnceLock`.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

// ---- FFI declarations -----------------------------------------------------

type HModule = *mut c_void;
type FarProc = *const c_void;

unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> HModule;
    fn GetProcAddress(module: HModule, name: *const u8) -> FarProc;
}

// ---- Real version.dll handle ----------------------------------------------

/// Wrapper around a raw pointer to make it `Send + Sync` for use in `OnceLock`.
///
/// This is safe because the HMODULE is a process-wide handle that never changes once loaded.
struct SyncHModule(HModule);
unsafe impl Send for SyncHModule {}
unsafe impl Sync for SyncHModule {}

/// Handle to the real system `version.dll`, loaded once on first use.
static REAL_VERSION: OnceLock<SyncHModule> = OnceLock::new();

/// Load the real `version.dll` from System32.
fn real_dll() -> HModule {
    REAL_VERSION
        .get_or_init(|| {
            // UTF-16 encoded "C:\Windows\System32\version.dll\0"
            let path: Vec<u16> = "C:\\Windows\\System32\\version.dll\0"
                .encode_utf16()
                .collect();
            let handle = unsafe { LoadLibraryW(path.as_ptr()) };
            if handle.is_null() {
                // Nothing we can do, the game will crash anyway without version.dll
                std::process::abort();
            }
            SyncHModule(handle)
        })
        .0
}

/// Resolve a function from the real version.dll by name.
///
/// Aborts if the function cannot be found (should never happen with the system DLL).
fn resolve(name: &[u8]) -> usize {
    let proc = unsafe { GetProcAddress(real_dll(), name.as_ptr()) };
    if proc.is_null() {
        std::process::abort();
    }
    proc as usize
}

// ---- Forwarding macros ----------------------------------------------------

/// Define a forwarding function that loads the real function pointer on first call.
///
/// Uses `AtomicUsize` for the cached pointer (0 = not yet resolved). The first call resolves
/// the real function via `GetProcAddress`, subsequent calls are a single atomic load.
macro_rules! forward {
    ($name:ident, $static_name:ident, $c_name:literal, $( $arg:ident : $ty:ty ),* => $ret:ty) => {
        static $static_name: AtomicUsize = AtomicUsize::new(0);

        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn $name( $( $arg : $ty ),* ) -> $ret {
            let mut fp = $static_name.load(Relaxed);
            if fp == 0 {
                fp = resolve($c_name);
                $static_name.store(fp, Relaxed);
            }
            let f: unsafe extern "system" fn( $( $ty ),* ) -> $ret = unsafe { std::mem::transmute(fp) };
            unsafe { f( $( $arg ),* ) }
        }
    };
}

// ---- Forwarded functions --------------------------------------------------

// Note: parameter names and exact types don't matter for forwarding,
// only the ABI, argument count, and sizes must match.

forward!(GetFileVersionInfoA, FP_GET_FILE_VERSION_INFO_A, b"GetFileVersionInfoA\0",
    filename: *const u8, handle: u32, len: u32, data: *mut c_void => i32);

forward!(GetFileVersionInfoW, FP_GET_FILE_VERSION_INFO_W, b"GetFileVersionInfoW\0",
    filename: *const u16, handle: u32, len: u32, data: *mut c_void => i32);

forward!(GetFileVersionInfoByHandle, FP_GET_FILE_VERSION_INFO_BY_HANDLE, b"GetFileVersionInfoByHandle\0",
    => ());

forward!(GetFileVersionInfoExA, FP_GET_FILE_VERSION_INFO_EX_A, b"GetFileVersionInfoExA\0",
    flags: u32, filename: *const u8, handle: u32, len: u32, data: *mut c_void => i32);

forward!(GetFileVersionInfoExW, FP_GET_FILE_VERSION_INFO_EX_W, b"GetFileVersionInfoExW\0",
    flags: u32, filename: *const u16, handle: u32, len: u32, data: *mut c_void => i32);

forward!(GetFileVersionInfoSizeA, FP_GET_FILE_VERSION_INFO_SIZE_A, b"GetFileVersionInfoSizeA\0",
    filename: *const u8, handle: *mut u32 => u32);

forward!(GetFileVersionInfoSizeW, FP_GET_FILE_VERSION_INFO_SIZE_W, b"GetFileVersionInfoSizeW\0",
    filename: *const u16, handle: *mut u32 => u32);

forward!(GetFileVersionInfoSizeExA, FP_GET_FILE_VERSION_INFO_SIZE_EX_A, b"GetFileVersionInfoSizeExA\0",
    flags: u32, filename: *const u8, handle: *mut u32 => u32);

forward!(GetFileVersionInfoSizeExW, FP_GET_FILE_VERSION_INFO_SIZE_EX_W, b"GetFileVersionInfoSizeExW\0",
    flags: u32, filename: *const u16, handle: *mut u32 => u32);

forward!(VerFindFileA, FP_VER_FIND_FILE_A, b"VerFindFileA\0",
    flags: u32, filename: *const u8, windir: *const u8, appdir: *const u8,
    curdir: *mut u8, curdir_len: *mut u32, destdir: *mut u8, destdir_len: *mut u32 => u32);

forward!(VerFindFileW, FP_VER_FIND_FILE_W, b"VerFindFileW\0",
    flags: u32, filename: *const u16, windir: *const u16, appdir: *const u16,
    curdir: *mut u16, curdir_len: *mut u32, destdir: *mut u16, destdir_len: *mut u32 => u32);

forward!(VerInstallFileA, FP_VER_INSTALL_FILE_A, b"VerInstallFileA\0",
    flags: u32, srcname: *const u8, destname: *const u8, srcdir: *const u8,
    destdir: *const u8, curdir: *const u8, tmpfile: *mut u8, tmpfile_len: *mut u32 => u32);

forward!(VerInstallFileW, FP_VER_INSTALL_FILE_W, b"VerInstallFileW\0",
    flags: u32, srcname: *const u16, destname: *const u16, srcdir: *const u16,
    destdir: *const u16, curdir: *const u16, tmpfile: *mut u16, tmpfile_len: *mut u32 => u32);

forward!(VerLanguageNameA, FP_VER_LANGUAGE_NAME_A, b"VerLanguageNameA\0",
    lang: u32, buf: *mut u8, buflen: u32 => u32);

forward!(VerLanguageNameW, FP_VER_LANGUAGE_NAME_W, b"VerLanguageNameW\0",
    lang: u32, buf: *mut u16, buflen: u32 => u32);

forward!(VerQueryValueA, FP_VER_QUERY_VALUE_A, b"VerQueryValueA\0",
    block: *const c_void, subblock: *const u8, buffer: *mut *mut c_void, len: *mut u32 => i32);

forward!(VerQueryValueW, FP_VER_QUERY_VALUE_W, b"VerQueryValueW\0",
    block: *const c_void, subblock: *const u16, buffer: *mut *mut c_void, len: *mut u32 => i32);
