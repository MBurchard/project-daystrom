use std::collections::HashSet;
use std::sync::Mutex;

use log::{debug, warn};

/// Set of already-hooked addresses to prevent double-hooking.
///
/// Double-hooking the same address corrupts the first hook's trampoline (the C++ mod had this exact bug with
/// `ProcessResultInternal` where ARM64 generic sharing resolved two different methods to the same address).
static HOOKED_ADDRESSES: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

/// Register an address as hooked. Returns `false` if already registered.
fn try_register(addr: usize) -> bool {
    let mut guard = HOOKED_ADDRESSES.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    set.insert(addr)
}

/// Error type for hook installation failures.
#[derive(Debug)]
pub enum HookError {
    /// The target address is already hooked.
    AlreadyHooked(usize),
    /// The hooking backend returned an error.
    BackendError(String),
}

impl std::fmt::Display for HookError {
    /// Format the error for logging.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyHooked(addr) => write!(f, "Address {addr:#x} is already hooked"),
            Self::BackendError(msg) => write!(f, "Hook backend error: {msg}"),
        }
    }
}

/// Install an inline hook on a target function via Dobby.
///
/// Returns the original function pointer (trampoline) on success. The caller must cast it to the
/// correct function type and store it for calling the original implementation.
///
/// Prevents double-hooking: if `target` was already hooked, returns `HookError::AlreadyHooked`.
pub fn install_hook(
    name: &str,
    target: *const (),
    replacement: *const (),
) -> Result<*const (), HookError> {
    let addr = target as usize;

    if !try_register(addr) {
        warn!(target: "HookEngine", "Skipping double-hook on {name} at {addr:#x}");
        return Err(HookError::AlreadyHooked(addr));
    }

    let original = unsafe {
        super::inline_hook::install(target, replacement)
    }.map_err(HookError::BackendError)?;

    debug!(target: "HookEngine", "Hook '{name}' installed at {addr:#x}");

    Ok(original)
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_allows_first_registration() {
        assert!(try_register(0x1111_0001));
    }

    #[test]
    fn registry_prevents_double_registration() {
        let addr = 0x1111_0002;
        assert!(try_register(addr));
        assert!(!try_register(addr));
    }

    #[test]
    fn registry_allows_different_addresses() {
        assert!(try_register(0x1111_0003));
        assert!(try_register(0x1111_0004));
    }
}
