use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// Maximum number of panics before a hook auto-deactivates for the rest of the session.
const MAX_HOOK_ERRORS: u32 = 3;

/// Per-hook metadata tracking error state and active status.
///
/// Uses atomics instead of a mutex to avoid any locking overhead in the game thread's hot path.
/// A hook that accumulates `MAX_HOOK_ERRORS` panics deactivates itself permanently (until the
/// next game restart).
pub struct HookInfo {
    /// Human-readable hook name for log messages.
    pub name: &'static str,
    /// Number of panics caught so far.
    error_count: AtomicU32,
    /// Whether the hook logic should run. Set to `false` after too many errors.
    active: AtomicBool,
}

impl HookInfo {
    /// Create a new active hook with zero errors.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            error_count: AtomicU32::new(0),
            active: AtomicBool::new(true),
        }
    }

    /// Check whether this hook's custom logic should run.
    pub fn is_active(&self) -> bool {
        self.active.load(Relaxed)
    }

    /// Record an error (panic caught by `catch_unwind`).
    ///
    /// Increments the error counter and deactivates the hook if the threshold is reached.
    /// Logs every error and the deactivation event.
    pub fn record_error(&self) {
        let count = self.error_count.fetch_add(1, Relaxed) + 1;
        log::error!(target: "HookSafety", "Hook '{}' panicked ({}/{})", self.name, count, MAX_HOOK_ERRORS);
        if count >= MAX_HOOK_ERRORS {
            self.active.store(false, Relaxed);
            log::error!(target: "HookSafety", "Hook '{}' deactivated after {} errors", self.name, count);
        }
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_hook_is_active() {
        let hook = HookInfo::new("test");
        assert!(hook.is_active());
    }

    #[test]
    fn stays_active_below_threshold() {
        let hook = HookInfo::new("test");
        hook.record_error();
        hook.record_error();
        assert!(hook.is_active());
    }

    #[test]
    fn deactivates_at_threshold() {
        let hook = HookInfo::new("test");
        for _ in 0..MAX_HOOK_ERRORS {
            hook.record_error();
        }
        assert!(!hook.is_active());
    }

    #[test]
    fn stays_inactive_after_deactivation() {
        let hook = HookInfo::new("test");
        for _ in 0..MAX_HOOK_ERRORS + 2 {
            hook.record_error();
        }
        assert!(!hook.is_active());
    }
}
