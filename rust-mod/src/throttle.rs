use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Global throttle state: maps log keys to the last time they were emitted.
static THROTTLE_STATE: Mutex<Option<HashMap<&'static str, Instant>>> = Mutex::new(None);

/// Check whether enough time has elapsed since the last log for this key.
///
/// Returns `true` if the message should be emitted, `false` if it should be suppressed.
/// The first call for any key always returns `true`.
pub fn should_log(key: &'static str, interval: Duration) -> bool {
    let mut guard = match THROTTLE_STATE.lock() {
        Ok(g) => g,
        Err(_) => return true, // poisoned mutex, let it through
    };
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();

    match map.get(key) {
        Some(last) if now.duration_since(*last) < interval => false,
        _ => {
            map.insert(key, now);
            true
        }
    }
}

/// Rate-limited logging macro.
///
/// Suppresses repeated log messages for the same key within the given interval.
/// The first call for any key always logs. Usage:
///
/// ```ignore
/// log_throttled!(info, "player_data", Duration::from_secs(300), "Player: {}", name);
/// ```
#[macro_export]
macro_rules! log_throttled {
    ($level:ident, $key:expr, $interval:expr, $($arg:tt)*) => {
        if $crate::throttle::should_log($key, $interval) {
            log::$level!($($arg)*);
        }
    };
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_always_logs() {
        assert!(should_log("test_first_call", Duration::from_secs(60)));
    }

    #[test]
    fn second_call_within_interval_suppressed() {
        let key = "test_suppressed";
        assert!(should_log(key, Duration::from_secs(60)));
        assert!(!should_log(key, Duration::from_secs(60)));
    }

    #[test]
    fn different_keys_are_independent() {
        assert!(should_log("test_key_a", Duration::from_secs(60)));
        assert!(should_log("test_key_b", Duration::from_secs(60)));
    }

    #[test]
    fn zero_interval_always_logs() {
        let key = "test_zero_interval";
        assert!(should_log(key, Duration::ZERO));
        assert!(should_log(key, Duration::ZERO));
    }
}
