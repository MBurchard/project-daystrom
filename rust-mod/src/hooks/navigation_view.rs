//! Shared navigation view state.
//!
//! Hooks that observe concrete navigation view changes write the viewed system here. Other hooks can read it to avoid
//! deriving "system view active" from unrelated Unity objects.

use std::sync::Mutex;

static VIEWED_SYSTEM_ID: Mutex<Option<i64>> = Mutex::new(None);

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Set the currently viewed system.
///
/// Returns `true` when the stored view changed.
pub(crate) fn set_viewed_system(system_id: Option<i64>) -> bool {
    let mut guard = VIEWED_SYSTEM_ID.lock().unwrap_or_else(|e| e.into_inner());
    if *guard == system_id {
        return false;
    }

    *guard = system_id;
    true
}

/// Clear the currently viewed system.
///
/// Returns `true` when a system view was active before clearing.
pub(crate) fn clear_viewed_system() -> bool {
    set_viewed_system(None)
}

/// Current system visible in the navigation view.
pub(crate) fn current_viewed_system_id() -> Option<i64> {
    *VIEWED_SYSTEM_ID.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether a concrete system is currently visible in the navigation view.
pub(crate) fn is_viewing_system() -> bool {
    current_viewed_system_id().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset() {
        let _ = clear_viewed_system();
    }

    #[test]
    fn tracks_viewed_system_changes() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset();

        assert_eq!(current_viewed_system_id(), None);
        assert!(!is_viewing_system());

        assert!(set_viewed_system(Some(42)));
        assert_eq!(current_viewed_system_id(), Some(42));
        assert!(is_viewing_system());

        assert!(!set_viewed_system(Some(42)));
        assert!(clear_viewed_system());
        assert_eq!(current_viewed_system_id(), None);
        assert!(!is_viewing_system());

        reset();
    }
}
