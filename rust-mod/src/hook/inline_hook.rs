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
/// - This function is not thread-safe: the target function must not be executing on another
///   thread while the hook is being installed.
///
/// ARM64 instructions are decoded, and PC-relative references (ADRP, ADR, B, LDR literal, etc.)
/// are relocated so the trampoline computes the same absolute addresses as the original code.
/// On x86_64, instructions are decoded and RIP-relative references are relocated.
#[cfg(target_arch = "aarch64")]
pub unsafe fn install(target: *const (), replacement: *const ()) -> Result<*const (), String> {
    let target_addr = target as usize;
    let num_insns = HOOK_SIZE / 4; // 4 instructions (16 bytes / 4)

    // Save original bytes
    let mut saved = [0u8; HOOK_SIZE];
    unsafe { std::ptr::copy_nonoverlapping(target as *const u8, saved.as_mut_ptr(), HOOK_SIZE) };

    // Allocate trampoline near the target so PC-relative instructions (B, BL, ADR, etc.)
    // can be relocated without exceeding their range limits.
    let trampoline_size = HOOK_SIZE + HOOK_SIZE; // saved + branch sequence
    let trampoline_mem = unsafe { alloc_near(target_addr, trampoline_size)? };
    let trampoline_addr = trampoline_mem as usize;

    // Decode, relocate, and write each instruction to the trampoline
    for i in 0..num_insns {
        let offset = i * 4;
        let raw = u32::from_le_bytes(saved[offset..offset + 4].try_into().unwrap());
        let decoded = super::aarch64::decode(raw);

        let relocated = if let Some(ref reloc) = decoded.reloc {
            let old_pc = (target_addr + offset) as u64;
            let new_pc = (trampoline_addr + offset) as u64;
            super::aarch64::relocate(raw, reloc, old_pc, new_pc)?
        } else {
            raw
        };

        let dst = unsafe { (trampoline_mem as *mut u32).add(i) };
        unsafe { dst.write(relocated) };
    }

    // Write the branch back to the instruction after our hook patch
    unsafe { write_branch(trampoline_addr + HOOK_SIZE, target_addr + HOOK_SIZE) };

    // Make trampoline executable
    unsafe { make_executable(trampoline_addr, trampoline_size)? };
    flush_icache(trampoline_addr, trampoline_size);

    // Patch the target: overwrite with branch to replacement
    unsafe { make_writable(target_addr, HOOK_SIZE)? };
    unsafe { write_branch(target_addr, replacement as usize) };
    unsafe { make_executable(target_addr, HOOK_SIZE)? };
    flush_icache(target_addr, HOOK_SIZE);

    Ok(trampoline_mem as *const ())
}

/// Install an inline hook with x86_64 instruction-aware relocation.
///
/// Decodes instructions at the hook target to find a clean boundary (>= 14 bytes), copies
/// them to a trampoline with RIP-relative displacement fixups, and writes a jump back to the
/// continuation address. This correctly handles LEA [RIP+...], MOV [RIP+...], CALL rel32, etc.
#[cfg(target_arch = "x86_64")]
pub unsafe fn install(target: *const (), replacement: *const ()) -> Result<*const (), String> {
    let target_addr = target as usize;

    // Read enough bytes from the target for instruction decoding.
    // Functions are always larger than 64 bytes, so this won't cross an unmapped page.
    let code = unsafe { std::slice::from_raw_parts(target as *const u8, 64) };

    // Decode instructions until we have enough bytes to fit our 14-byte jump patch
    let mut decoded: Vec<super::x86_64::Insn> = Vec::new();
    let mut total_len = 0;
    while total_len < HOOK_SIZE {
        let insn = super::x86_64::decode(&code[total_len..])
            .map_err(|e| format!("decode at +{total_len}: {e}"))?;
        total_len += insn.len;
        decoded.push(insn);
    }

    // Save original bytes
    let mut saved = vec![0u8; total_len];
    unsafe { std::ptr::copy_nonoverlapping(target as *const u8, saved.as_mut_ptr(), total_len) };

    // Allocate trampoline within ±2GB of target so relocated RIP-relative
    // displacements still fit in 32 bits.
    let trampoline_size = total_len + HOOK_SIZE;
    let trampoline_mem = unsafe { alloc_near(target_addr, trampoline_size)? };
    let trampoline_addr = trampoline_mem as usize;

    // Copy instructions to trampoline, relocating RIP-relative references
    let mut src_offset = 0;
    let mut dst_offset = 0;
    for insn in &decoded {
        let src = &saved[src_offset..src_offset + insn.len];
        let dst = unsafe {
            std::slice::from_raw_parts_mut(
                (trampoline_mem as *mut u8).add(dst_offset),
                insn.len,
            )
        };
        dst.copy_from_slice(src);

        // Fix up RIP-relative displacement if present
        if let Some(reloc_off) = insn.reloc_offset {
            let disp_ptr = unsafe {
                (trampoline_mem as *mut u8).add(dst_offset + reloc_off) as *mut i32
            };
            let old_disp = unsafe { disp_ptr.read_unaligned() } as i64;
            let old_rip = (target_addr + src_offset + insn.len) as i64;
            let new_rip = (trampoline_addr + dst_offset + insn.len) as i64;
            let abs_target = old_rip + old_disp;
            let new_disp = abs_target - new_rip;

            if new_disp > i32::MAX as i64 || new_disp < i32::MIN as i64 {
                return Err(format!(
                    "relocation overflow: trampoline too far from original code \
                     (delta: {new_disp:#x}, max ±{:#x}). \
                     Consider allocating trampoline near the target.",
                    i32::MAX
                ));
            }
            unsafe { disp_ptr.write_unaligned(new_disp as i32) };
        }

        src_offset += insn.len;
        dst_offset += insn.len;
    }

    // Write jump back to the instruction after our patch
    unsafe { write_branch(trampoline_addr + dst_offset, target_addr + total_len) };

    // Make trampoline executable
    unsafe { make_executable(trampoline_addr, trampoline_size)? };
    flush_icache(trampoline_addr, trampoline_size);

    // Patch the target: overwrite with jump to replacement
    unsafe { make_writable(target_addr, HOOK_SIZE)? };
    unsafe { write_branch(target_addr, replacement as usize) };
    unsafe { make_executable(target_addr, HOOK_SIZE)? };
    flush_icache(target_addr, HOOK_SIZE);

    Ok(trampoline_mem as *const ())
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


// ---- Platform: Unix (macOS + Linux) ----------------------------------------

/// Allocate writable memory via `mmap` for code that will be made executable later.
///
/// Pages are allocated as RW first, then switched to RX after writing trampoline code.
/// On Apple Silicon, this also satisfies the hardware W^X requirement.
#[cfg(unix)]
#[allow(dead_code)]
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

/// Allocate writable memory within ±1GB of `target` using Mach VM region queries.
///
/// Scans the virtual address space for unmapped gaps between mapped regions via
/// `mach_vm_region`, then allocates in the first suitable gap. This is the macOS
/// equivalent of the Windows `VirtualQuery` approach: deterministic, no brute-force.
#[cfg(target_os = "macos")]
unsafe fn alloc_near(target: usize, size: usize) -> Result<*mut c_void, String> {
    const MAX_RANGE: usize = 0x4000_0000; // ±1GB
    const GRANULARITY: usize = 0x10000; // 64KB
    const VM_REGION_BASIC_INFO_64: i32 = 9;
    const VM_REGION_BASIC_INFO_COUNT_64: u32 = 9;

    let min_addr = target.saturating_sub(MAX_RANGE).max(GRANULARITY);
    let max_addr = target.saturating_add(MAX_RANGE);
    let mut scan = min_addr as u64;

    while (scan as usize) < max_addr {
        let mut region_addr = scan;
        let mut region_size: u64 = 0;
        let mut info = [0i32; VM_REGION_BASIC_INFO_COUNT_64 as usize];
        let mut info_count = VM_REGION_BASIC_INFO_COUNT_64;
        let mut object_name: u32 = 0;

        let kr = unsafe {
            mach_vm_region(
                mach_task_self(),
                &mut region_addr,
                &mut region_size,
                VM_REGION_BASIC_INFO_64,
                info.as_mut_ptr(),
                &mut info_count,
                &mut object_name,
            )
        };

        // Gap: from scan position to the next mapped region (or max_addr if none)
        let gap_start = scan as usize;
        let gap_end = if kr != 0 {
            max_addr // no more mapped regions, rest is free
        } else {
            (region_addr as usize).min(max_addr)
        };

        // Try to allocate in this gap (aligned to granularity)
        if gap_end > gap_start {
            let alloc_at = (gap_start + GRANULARITY - 1) & !(GRANULARITY - 1);
            if alloc_at + size <= gap_end {
                let ptr = unsafe {
                    libc::mmap(
                        alloc_at as *mut c_void,
                        size,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                        -1,
                        0,
                    )
                };
                if ptr != libc::MAP_FAILED {
                    return Ok(ptr);
                }
            }
        }

        if kr != 0 {
            break; // no more regions to scan
        }

        // Advance past the current mapped region
        scan = region_addr + region_size;
    }

    Err(format!(
        "could not allocate {size} bytes within ±1GB of {target:#x}"
    ))
}

/// Get the system page size.
#[cfg(unix)]
fn page_size() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

// ---- Platform: macOS-specific FFI -----------------------------------------

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_task_self() -> u32;
    fn mach_vm_protect(
        task: u32, address: u64, size: u64, set_maximum: i32, protection: i32,
    ) -> i32;
    fn mach_vm_region(
        task: u32, address: *mut u64, size: *mut u64, flavor: i32, info: *mut i32,
        info_count: *mut u32, object_name: *mut u32,
    ) -> i32;
    fn sys_icache_invalidate(start: *mut c_void, size: usize);
}

// ---- Platform: Linux ------------------------------------------------------

/// Allocate executable memory within ±1GB of `target` using mmap address hints.
///
/// Scans outward from the target in 64KB steps. Linux usually honours the hint, but if the
/// returned address falls outside the required range, we unmap and try the next slot.
#[cfg(target_os = "linux")]
unsafe fn alloc_near(target: usize, size: usize) -> Result<*mut c_void, String> {
    const MAX_RANGE: usize = 0x4000_0000; // ±1GB
    const GRANULARITY: usize = 0x10000; // 64KB

    let min_addr = target.saturating_sub(MAX_RANGE).max(GRANULARITY);
    let max_addr = target.saturating_add(MAX_RANGE);

    let mut hint = target.saturating_sub(MAX_RANGE / 2) & !(GRANULARITY - 1);
    while hint < max_addr {
        if hint < min_addr {
            hint = min_addr;
        }
        let ptr = unsafe {
            libc::mmap(
                hint as *mut c_void,
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr != libc::MAP_FAILED {
            let addr = ptr as usize;
            if addr >= min_addr && addr + size <= max_addr {
                return Ok(ptr);
            }
            unsafe { libc::munmap(ptr, size) };
        }
        hint += GRANULARITY;
    }

    Err(format!(
        "could not allocate {size} bytes within ±1GB of {target:#x}"
    ))
}

/// Make a memory region writable using `mprotect`.
#[cfg(target_os = "linux")]
unsafe fn make_writable(addr: usize, size: usize) -> Result<(), String> {
    let page_size = page_size();
    let page_start = addr & !(page_size - 1);
    let page_end = (addr + size + page_size - 1) & !(page_size - 1);
    let region_size = page_end - page_start;

    let ret = unsafe {
        libc::mprotect(
            page_start as *mut c_void,
            region_size,
            libc::PROT_READ | libc::PROT_WRITE,
        )
    };
    if ret != 0 {
        return Err(format!(
            "mprotect (writable) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Restore a memory region to read-execute using `mprotect`.
#[cfg(target_os = "linux")]
unsafe fn make_executable(addr: usize, size: usize) -> Result<(), String> {
    let page_size = page_size();
    let page_start = addr & !(page_size - 1);
    let page_end = (addr + size + page_size - 1) & !(page_size - 1);
    let region_size = page_end - page_start;

    let ret = unsafe {
        libc::mprotect(
            page_start as *mut c_void,
            region_size,
            libc::PROT_READ | libc::PROT_EXEC,
        )
    };
    if ret != 0 {
        return Err(format!(
            "mprotect (executable) failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Flush the instruction cache.
///
/// No-op on x86_64 (coherent I-cache). On aarch64, uses the GCC/LLVM built-in `__clear_cache`.
#[cfg(target_os = "linux")]
fn flush_icache(_addr: usize, _size: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        unsafe extern "C" {
            fn __clear_cache(start: *mut c_void, end: *mut c_void);
        }
        unsafe {
            __clear_cache(
                _addr as *mut c_void,
                (_addr + _size) as *mut c_void,
            );
        }
    }
}

// ---- Platform: Windows (x86_64) -------------------------------------------

#[cfg(target_os = "windows")]
#[allow(dead_code)] // kept for symmetry with macOS; Windows x86_64 uses alloc_near instead
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

/// Allocate executable memory within ±1GB of `target` for RIP-relative relocation.
///
/// Searches **outward from the target** (first below, then above) to find the closest
/// free region. This ensures relocated RIP-relative displacements fit in a signed 32-bit
/// value even when the original instruction references data up to ~1GB away.
#[cfg(target_os = "windows")]
unsafe fn alloc_near(target: usize, size: usize) -> Result<*mut c_void, String> {
    #[repr(C)]
    struct MemBasicInfo {
        base_address: *mut c_void,
        allocation_base: *mut c_void,
        allocation_protect: u32,
        region_size: usize,
        state: u32,
        protect: u32,
        mem_type: u32,
    }

    unsafe extern "system" {
        fn VirtualQuery(addr: *const c_void, info: *mut MemBasicInfo, len: usize) -> usize;
        fn VirtualAlloc(
            addr: *mut c_void, size: usize, alloc_type: u32, protect: u32,
        ) -> *mut c_void;
    }

    const MEM_FREE: u32 = 0x10000;
    const MEM_COMMIT_RESERVE: u32 = 0x3000;
    const PAGE_EXECUTE_READWRITE: u32 = 0x40;
    const GRANULARITY: usize = 0x10000; // 64KB Windows allocation granularity
    // 1GB range leaves ~1GB headroom for RIP-relative offsets within the target module
    const MAX_RANGE: usize = 0x4000_0000;
    let mbi_size = size_of::<MemBasicInfo>();

    let min_addr = target.saturating_sub(MAX_RANGE).max(GRANULARITY);
    let max_addr = target.saturating_add(MAX_RANGE);

    // Helper: try to allocate within a free region
    let try_alloc = |region_base: usize, region_size: usize| -> *mut c_void {
        let alloc_at = (region_base + GRANULARITY - 1) & !(GRANULARITY - 1);
        if alloc_at + size <= region_base + region_size
            && alloc_at >= min_addr
            && alloc_at + size <= max_addr
        {
            unsafe { VirtualAlloc(alloc_at as *mut c_void, size, MEM_COMMIT_RESERVE, PAGE_EXECUTE_READWRITE) }
        } else {
            std::ptr::null_mut()
        }
    };

    // 1. Search BELOW the target, scanning downward (closest first)
    let mut scan = target & !(GRANULARITY - 1);
    while scan >= min_addr && scan > 0 {
        let mut mbi = unsafe { std::mem::zeroed::<MemBasicInfo>() };
        if unsafe { VirtualQuery(scan as *const c_void, &mut mbi, mbi_size) } == 0 {
            break;
        }
        let base = mbi.base_address as usize;
        if mbi.state == MEM_FREE && mbi.region_size >= size {
            let ptr = try_alloc(base, mbi.region_size);
            if !ptr.is_null() {
                return Ok(ptr);
            }
        }
        // Move to the region before this one
        if base == 0 || base <= min_addr {
            break;
        }
        scan = base.saturating_sub(1);
    }

    // 2. Search ABOVE the target, scanning upward (closest first)
    scan = (target & !(GRANULARITY - 1)) + GRANULARITY;
    while scan < max_addr {
        let mut mbi = unsafe { std::mem::zeroed::<MemBasicInfo>() };
        if unsafe { VirtualQuery(scan as *const c_void, &mut mbi, mbi_size) } == 0 {
            break;
        }
        let base = mbi.base_address as usize;
        let region_size = mbi.region_size;
        if mbi.state == MEM_FREE && region_size >= size {
            let ptr = try_alloc(base, region_size);
            if !ptr.is_null() {
                return Ok(ptr);
            }
        }
        // Move to the next region
        scan = base + region_size.max(GRANULARITY);
    }

    Err(format!(
        "could not allocate {size} bytes within ±1GB of {target:#x}"
    ))
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
