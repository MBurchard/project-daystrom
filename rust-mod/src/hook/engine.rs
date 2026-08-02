use std::collections::HashMap;
use std::sync::Mutex;

use log::{debug, warn};

/// State of a hook target while it is being installed or after installation completed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HookState {
    Installing,
    Active,
}

/// Registry of hook target addresses to prevent concurrent and duplicate installations.
///
/// Double-hooking the same address corrupts the first hook's trampoline (the C++ mod had this exact bug with
/// `ProcessResultInternal` where ARM64 generic sharing resolved two different methods to the same address).
static HOOKED_ADDRESSES: Mutex<Option<HashMap<usize, HookState>>> = Mutex::new(None);

/// Reserve an address for installation. Returns `false` if another hook already owns it.
fn try_reserve(addr: usize) -> bool {
    let mut guard = HOOKED_ADDRESSES.lock().unwrap_or_else(|e| e.into_inner());
    let registry = guard.get_or_insert_with(HashMap::new);
    if registry.contains_key(&addr) {
        return false;
    }
    registry.insert(addr, HookState::Installing);
    true
}

/// Mark a reserved address as successfully hooked.
fn activate(addr: usize) {
    let mut guard = HOOKED_ADDRESSES.lock().unwrap_or_else(|e| e.into_inner());
    let registry = guard.get_or_insert_with(HashMap::new);
    registry.insert(addr, HookState::Active);
}

/// Release a failed reservation so a later installation can retry.
fn release(addr: usize) {
    let mut guard = HOOKED_ADDRESSES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(registry) = guard.as_mut()
        && registry.get(&addr) == Some(&HookState::Installing)
    {
        registry.remove(&addr);
    }
}

/// Error type for hook installation failures.
#[derive(Debug)]
pub enum HookError {
    /// The target address is already hooked.
    AlreadyHooked(usize),
    /// The hooking backend returned an error.
    BackendError(String),
}

type HookBackend = unsafe fn(*const (), *const ()) -> Result<*const (), String>;

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
pub fn install_hook(name: &str, target: *const (), replacement: *const ()) -> Result<*const (), HookError> {
    install_hook_with(name, target, replacement, super::inline_hook::install)
}

fn install_hook_with(
    name: &str,
    target: *const (),
    replacement: *const (),
    backend: HookBackend,
) -> Result<*const (), HookError> {
    let addr = target as usize;

    if !try_reserve(addr) {
        warn!(target: "HookEngine", "Skipping double-hook on {name} at {addr:#x}");
        return Err(HookError::AlreadyHooked(addr));
    }

    let original = match unsafe { backend(target, replacement) } {
        Ok(original) => original,
        Err(error) => {
            release(addr);
            return Err(HookError::BackendError(error));
        }
    };
    activate(addr);

    debug!(target: "HookEngine", "Hook '{name}' installed at {addr:#x}");

    Ok(original)
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn successful_backend(target: *const (), _: *const ()) -> Result<*const (), String> {
        Ok(target)
    }

    unsafe fn failing_backend(_: *const (), _: *const ()) -> Result<*const (), String> {
        Err("test failure".to_string())
    }

    #[test]
    fn registry_allows_first_reservation() {
        assert!(try_reserve(0x1111_0001));
    }

    #[test]
    fn registry_prevents_double_reservation() {
        let addr = 0x1111_0002;
        assert!(try_reserve(addr));
        assert!(!try_reserve(addr));
    }

    #[test]
    fn registry_allows_different_reservations() {
        assert!(try_reserve(0x1111_0003));
        assert!(try_reserve(0x1111_0004));
    }

    #[test]
    fn failed_installation_can_be_retried() {
        let target = 0x1111_0005usize as *const ();
        let replacement = 0x2222_0005usize as *const ();

        assert!(matches!(
            install_hook_with("first", target, replacement, failing_backend),
            Err(HookError::BackendError(_))
        ));
        assert!(install_hook_with("retry", target, replacement, successful_backend).is_ok());
    }

    #[test]
    fn successful_installation_stays_reserved() {
        let target = 0x1111_0006usize as *const ();
        let replacement = 0x2222_0006usize as *const ();

        assert!(install_hook_with("first", target, replacement, successful_backend).is_ok());
        assert!(matches!(
            install_hook_with("duplicate", target, replacement, successful_backend),
            Err(HookError::AlreadyHooked(_))
        ));
    }
}
