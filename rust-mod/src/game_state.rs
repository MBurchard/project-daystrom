//! Centralized game state with change detection.
//!
//! Hooks write observed values here.
//! When a value changes, the delta is sent to Daystrom via WebSocket and logged at debug level.

use std::sync::{Mutex, OnceLock};

use log::debug;

// ---- Player data -----------------------------------------------------------

/// Snapshot of the current player profile, updated by the user profile hook.
#[derive(Debug, Default, Clone, PartialEq)]
struct PlayerData {
    name: Option<String>,
    level: Option<i32>,
    might: Option<u64>,
}

/// Global player state, lazily initialized on first access.
static STATE: OnceLock<Mutex<PlayerData>> = OnceLock::new();

/// Access the global player state, initializing on first call.
fn state() -> &'static Mutex<PlayerData> {
    STATE.get_or_init(|| Mutex::new(PlayerData::default()))
}

/// Update player data from the user profile hook.
///
/// Compares each field against the stored state.
/// Only changed fields are logged (debug) and sent to Daystrom via WebSocket as individual `player.update` messages
/// with `key`/`value` pairs.
pub fn update_player(
    name: Option<String>,
    level: Option<i32>,
    might: Option<u64>,
) {
    let state = state();
    let mut data = state.lock().unwrap();

    if name != data.name {
        if let Some(ref v) = name {
            debug!(target: "PlayerData", "name = {v}");
            crate::ws_client::send(
                "player.update",
                serde_json::json!({"key": "name", "value": v}),
            );
        }
        data.name = name;
    }

    if level != data.level {
        if let Some(v) = level {
            debug!(target: "PlayerData", "level = {v}");
            crate::ws_client::send(
                "player.update",
                serde_json::json!({"key": "level", "value": v}),
            );
        }
        data.level = level;
    }

    if might != data.might {
        if let Some(v) = might {
            debug!(target: "PlayerData", "might = {v}");
            crate::ws_client::send(
                "player.update",
                serde_json::json!({"key": "might", "value": v}),
            );
        }
        data.might = might;
    }
}

// ---- Tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that share the global STATE.
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Reset state to default before each test.
    fn reset() {
        *state().lock().unwrap() = PlayerData::default();
    }

    /// Read a snapshot of the current state.
    fn snapshot() -> PlayerData {
        state().lock().unwrap().clone()
    }

    #[test]
    fn initial_update_stores_all_fields() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        update_player(Some("Nabor".into()), Some(42), Some(12345));

        let data = snapshot();
        assert_eq!(data.name.as_deref(), Some("Nabor"));
        assert_eq!(data.level, Some(42));
        assert_eq!(data.might, Some(12345));
    }

    #[test]
    fn unchanged_values_leave_state_intact() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        update_player(Some("Nabor".into()), Some(42), Some(12345));
        update_player(Some("Nabor".into()), Some(42), Some(12345));

        let data = snapshot();
        assert_eq!(data.name.as_deref(), Some("Nabor"));
        assert_eq!(data.level, Some(42));
        assert_eq!(data.might, Some(12345));
    }

    #[test]
    fn partial_change_updates_only_changed_field() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        update_player(Some("Nabor".into()), Some(42), Some(12345));
        update_player(Some("Nabor".into()), Some(43), Some(12345));

        let data = snapshot();
        assert_eq!(data.name.as_deref(), Some("Nabor"));
        assert_eq!(data.level, Some(43));
        assert_eq!(data.might, Some(12345));
    }

    #[test]
    fn some_to_none_clears_field() {
        let _lock = TEST_LOCK.lock().unwrap();
        reset();

        update_player(Some("Nabor".into()), Some(42), Some(12345));
        update_player(None, None, None);

        let data = snapshot();
        assert_eq!(data.name, None);
        assert_eq!(data.level, None);
        assert_eq!(data.might, None);
    }
}
