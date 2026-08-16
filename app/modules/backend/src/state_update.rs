//! Shared mutation primitive for reactive backend stores.

use std::sync::Mutex;

/// Mutate a mutex-protected value and return a snapshot only when its value changed.
///
/// The lock is released before the returned snapshot can be emitted to frontend listeners.
pub fn update_if_changed<T>(state: &Mutex<T>, updater: impl FnOnce(&mut T)) -> Option<T>
where
    T: Clone + PartialEq,
{
    let mut value = state.lock().unwrap();
    let previous = value.clone();
    updater(&mut value);
    if *value != previous { Some(value.clone()) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_snapshot_after_change() {
        let state = Mutex::new(1);

        let changed = update_if_changed(&state, |value| *value = 2);

        assert_eq!(changed, Some(2));
        assert_eq!(*state.lock().unwrap(), 2);
    }

    #[test]
    fn returns_none_when_value_is_unchanged() {
        let state = Mutex::new(1);

        let changed = update_if_changed(&state, |value| *value = 1);

        assert_eq!(changed, None);
    }
}
