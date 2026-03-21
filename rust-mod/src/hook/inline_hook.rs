use std::ffi::c_void;

/// Size of the hook trampoline entry sequence in bytes.
///
/// ARM64: `LDR X16, #8` (4 bytes) + `BR X16` (4 bytes) + target address (8 bytes) = 16 bytes.
/// This overwrites exactly 4 ARM64 instructions at the hook target.
#[cfg(target_arch = "aarch64")]
const HOOK_SIZE: usize = 16;

/// Size of the hook trampoline entry sequence in bytes.
///
/// x86_64: `JMP [RIP+0]` (6 bytes) + target address (8 bytes) = 14 bytes.
#[cfg(target_arch = "x86_64")]
const HOOK_SIZE: usize = 14;

/// Install an inline hook at `target`, redirecting it to `replacement`.
///
/// Returns a pointer to the trampoline (the original function that can be called to execute the
/// unhooked behaviour). The trampoline is allocated in executable memory and contains the
/// overwritten instructions followed by a branch back to the original function.
///
/// # Safety
///
/// - `target` must point to the start of a function with at least `HOOK_SIZE` bytes.
/// - The overwritten instructions must not contain PC-relative operations (function prologues
///   are typically safe). PC-relative relocation is not yet implemented.
/// - This function is not thread-safe: the target function must not be executing on another
///   thread while the hook is being installed.
pub unsafe fn install(target: *const (), replacement: *const ()) -> Result<*const (), String> {
    let target_addr = target as usize;

    // Save original bytes
    let mut saved = [0u8; HOOK_SIZE];
    unsafe { std::ptr::copy_nonoverlapping(target as *const u8, saved.as_mut_ptr(), HOOK_SIZE) };

    // Allocate executable trampoline: saved instructions + branch back to (target + HOOK_SIZE)
    let trampoline = unsafe { allocate_trampoline(&saved, target_addr + HOOK_SIZE)? };

    // Make target memory writable
    unsafe { make_writable(target_addr, HOOK_SIZE)? };

    // Write the hook: branch to replacement
    unsafe { write_branch(target_addr, replacement as usize) };

    // Restore target memory to executable (read + execute, no write)
    unsafe { make_executable(target_addr, HOOK_SIZE)? };

    // Flush instruction cache so the CPU sees the new code
    flush_icache(target_addr, HOOK_SIZE);

    Ok(trampoline)
}

// ---- ARM64 branch encoding ------------------------------------------------

/// Write a branch-to-address sequence at `addr` that jumps to `target`.
///
/// ARM64 sequence (16 bytes):
/// ```text
/// LDR X16, #8      // Load address from 8 bytes ahead
/// BR X16            // Branch to X16
/// <8-byte address>  // The target address
/// ```
#[cfg(target_arch = "aarch64")]
unsafe fn write_branch(addr: usize, target: usize) {
    let p = addr as *mut u32;
    // LDR X16, #8  →  0x58000050
    unsafe { p.write(0x5800_0050) };
    // BR X16       →  0xD61F0200
    unsafe { p.add(1).write(0xD61F_0200) };
    // 8-byte target address
    unsafe { (p.add(2) as *mut u64).write(target as u64) };
}

/// Write a branch-to-address sequence at `addr` that jumps to `target`.
///
/// x86_64 sequence (14 bytes):
/// ```text
/// FF 25 00 00 00 00  // JMP [RIP+0]
/// <8-byte address>   // The target address
/// ```
#[cfg(target_arch = "x86_64")]
unsafe fn write_branch(addr: usize, target: usize) {
    let p = addr as *mut u8;
    // JMP [RIP+0]
    unsafe {
        p.write(0xFF);
        p.add(1).write(0x25);
        (p.add(2) as *mut u32).write(0);
        // 8-byte target address
        (p.add(6) as *mut u64).write(target as u64);
    }
}

// ---- Trampoline allocation ------------------------------------------------

/// Allocate executable memory for the trampoline.
///
/// The trampoline contains the saved original instructions followed by a branch back to the
/// continuation address (original function + HOOK_SIZE).
///
/// The allocated memory is intentionally never freed. Trampolines must remain valid for the entire
/// process lifetime because the hooked function's original code has been overwritten and the
/// trampoline is the only way to call the original implementation.
unsafe fn allocate_trampoline(saved: &[u8; HOOK_SIZE], continuation: usize) -> Result<*const (), String> {
    // Trampoline = saved instructions + branch sequence
    let trampoline_size = HOOK_SIZE + HOOK_SIZE;
    let mem = unsafe { alloc_executable(trampoline_size)? };

    // Copy saved instructions
    unsafe { std::ptr::copy_nonoverlapping(saved.as_ptr(), mem as *mut u8, HOOK_SIZE) };

    // Write branch back to original function (after our hook patch)
    unsafe { write_branch(mem as usize + HOOK_SIZE, continuation) };

    // Ensure trampoline is executable
    unsafe { make_executable(mem as usize, trampoline_size)? };
    flush_icache(mem as usize, trampoline_size);

    Ok(mem as *const ())
}

// ---- Platform: macOS (ARM64 + x86_64) -------------------------------------

/// Allocate writable memory via `mmap` for code that will be made executable later.
///
/// Apple Silicon enforces W^X at the hardware level: a page cannot be both writable and executable
/// in the same thread. We allocate as RW first, write the trampoline code, then switch to RX.
#[cfg(target_os = "macos")]
unsafe fn alloc_executable(size: usize) -> Result<*mut c_void, String> {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
    }
    Ok(ptr)
}

/// Make a memory region writable using `mach_vm_protect` with `VM_PROT_COPY`.
///
/// `VM_PROT_COPY` creates a copy-on-write mapping, which is the only way to make signed code
/// pages writable on macOS ARM64.
#[cfg(target_os = "macos")]
unsafe fn make_writable(addr: usize, size: usize) -> Result<(), String> {
    let page_size = page_size();
    let page_start = addr & !(page_size - 1);
    let page_end = (addr + size + page_size - 1) & !(page_size - 1);
    let region_size = page_end - page_start;

    // VM_PROT_COPY = 0x10, allows COW on signed code pages
    let prot = libc::PROT_READ | libc::PROT_WRITE | 0x10;
    let kr = unsafe { mach_vm_protect(mach_task_self(), page_start as u64, region_size as u64, 0, prot) };
    if kr != 0 {
        return Err(format!("mach_vm_protect (writable) failed: kern_return {kr}"));
    }
    Ok(())
}

/// Restore a memory region to read-execute.
#[cfg(target_os = "macos")]
unsafe fn make_executable(addr: usize, size: usize) -> Result<(), String> {
    let page_size = page_size();
    let page_start = addr & !(page_size - 1);
    let page_end = (addr + size + page_size - 1) & !(page_size - 1);
    let region_size = page_end - page_start;

    let prot = libc::PROT_READ | libc::PROT_EXEC;
    let kr = unsafe { mach_vm_protect(mach_task_self(), page_start as u64, region_size as u64, 0, prot) };
    if kr != 0 {
        return Err(format!("mach_vm_protect (executable) failed: kern_return {kr}"));
    }
    Ok(())
}

/// Flush the instruction cache for the modified code region.
#[cfg(target_os = "macos")]
fn flush_icache(addr: usize, size: usize) {
    unsafe {
        sys_icache_invalidate(addr as *mut c_void, size);
    }
}

/// Get the system page size.
#[cfg(target_os = "macos")]
fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

// ---- macOS FFI declarations -----------------------------------------------

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_task_self() -> u32;
    fn mach_vm_protect(task: u32, address: u64, size: u64, set_maximum: i32, protection: i32) -> i32;
    fn sys_icache_invalidate(start: *mut c_void, size: usize);
}

// ---- Platform: Windows (x86_64) -------------------------------------------

#[cfg(target_os = "windows")]
unsafe fn alloc_executable(size: usize) -> Result<*mut c_void, String> {
    unsafe extern "system" {
        fn VirtualAlloc(addr: *mut c_void, size: usize, alloc_type: u32, protect: u32) -> *mut c_void;
    }
    // MEM_COMMIT | MEM_RESERVE = 0x3000, PAGE_EXECUTE_READWRITE = 0x40
    let ptr = unsafe { VirtualAlloc(std::ptr::null_mut(), size, 0x3000, 0x40) };
    if ptr.is_null() {
        return Err("VirtualAlloc failed".to_string());
    }
    Ok(ptr)
}

#[cfg(target_os = "windows")]
unsafe fn make_writable(addr: usize, size: usize) -> Result<(), String> {
    unsafe extern "system" {
        fn VirtualProtect(addr: *mut c_void, size: usize, new_protect: u32, old_protect: *mut u32) -> i32;
    }
    let mut old_protect: u32 = 0;
    // PAGE_EXECUTE_READWRITE = 0x40
    let ok = unsafe { VirtualProtect(addr as *mut c_void, size, 0x40, &mut old_protect) };
    if ok == 0 {
        return Err("VirtualProtect (writable) failed".to_string());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn make_executable(addr: usize, size: usize) -> Result<(), String> {
    unsafe extern "system" {
        fn VirtualProtect(addr: *mut c_void, size: usize, new_protect: u32, old_protect: *mut u32) -> i32;
    }
    let mut old_protect: u32 = 0;
    // PAGE_EXECUTE_READ = 0x20
    let ok = unsafe { VirtualProtect(addr as *mut c_void, size, 0x20, &mut old_protect) };
    if ok == 0 {
        return Err("VirtualProtect (executable) failed".to_string());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn flush_icache(addr: usize, size: usize) {
    unsafe extern "system" {
        fn FlushInstructionCache(process: *mut c_void, addr: *const c_void, size: usize) -> i32;
        fn GetCurrentProcess() -> *mut c_void;
    }
    unsafe {
        FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, size);
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn hook_size_is_16_on_arm64() {
        assert_eq!(HOOK_SIZE, 16);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn hook_size_is_14_on_x86_64() {
        assert_eq!(HOOK_SIZE, 14);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn write_branch_arm64_encoding() {
        let mut buf = [0u8; 16];
        let target: usize = 0xDEAD_BEEF_CAFE_BABE;
        unsafe { write_branch(buf.as_mut_ptr() as usize, target) };

        // LDR X16, #8
        assert_eq!(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 0x5800_0050);
        // BR X16
        assert_eq!(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]), 0xD61F_0200);
        // Target address (little-endian)
        let addr = u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]);
        assert_eq!(addr, target as u64);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn write_branch_x86_64_encoding() {
        let mut buf = [0u8; 14];
        let target: usize = 0xDEAD_BEEF_CAFE_BABE;
        unsafe { write_branch(buf.as_mut_ptr() as usize, target) };

        // JMP [RIP+0]
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[1], 0x25);
        assert_eq!(u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]), 0);
        // Target address (little-endian)
        let addr = u64::from_le_bytes([buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13]]);
        assert_eq!(addr, target as u64);
    }
}
