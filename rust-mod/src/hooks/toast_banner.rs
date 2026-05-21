//! Suppress specific toast banner notifications.
//!
//! Hooks `ToastObserver.AreToastsAllowed(Toast)` to check the toast's state against the user's disabled list.
//! Returns `false` for suppressed types, causing the game to skip the toast through its own control flow.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering::Relaxed};

use log::debug;

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Dynamically resolved field offsets -----------------------------------

/// Offset of `<State>k__BackingField` (ToastState enum, i32) on Toast.
static TOAST_STATE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Mapping of ToastState variant names to their integer values (game v145).
///
/// Used to convert human-readable names from settings into the bitfield representation used in the hot path.
const TOAST_STATES: &[(&str, u32)] = &[
    ("Standard", 0),
    ("FactionWarning", 1),
    ("FactionLevelUp", 2),
    ("FactionLevelDown", 3),
    ("FactionDiscovered", 4),
    ("IncomingAttack", 5),
    ("IncomingAttackFaction", 6),
    ("FleetBattle", 7),
    ("StationBattle", 8),
    ("StationVictory", 9),
    ("Victory", 10),
    ("Defeat", 11),
    ("StationDefeat", 12),
    // value 13 unused
    ("Tournament", 14),
    ("ArmadaCreated", 15),
    ("ArmadaCanceled", 16),
    ("ArmadaIncomingAttack", 17),
    ("ArmadaBattleWon", 18),
    ("ArmadaBattleLost", 19),
    ("DiplomacyUpdated", 20),
    ("JoinedTakeover", 21),
    ("CompetitorJoinedTakeover", 22),
    ("AbandonedTerritory", 23),
    ("TakeoverVictory", 24),
    ("TakeoverDefeat", 25),
    ("TreasuryProgress", 26),
    ("TreasuryFull", 27),
    ("Achievement", 28),
    ("AssaultVictory", 29),
    ("AssaultDefeat", 30),
    ("ChallengeComplete", 31),
    ("ChallengeFailed", 32),
    ("StrikeHit", 33),
    ("StrikeDefeat", 34),
    ("WarchestProgress", 35),
    ("WarchestFull", 36),
    ("PartialVictory", 37),
    ("ArenaTimeLeft", 38),
    ("ChainedEventScored", 39),
    ("FleetPresetApplied", 40),
    ("SurgeWarmUpEnded", 41),
    ("SurgeHostileGroupDefeated", 42),
    ("SurgeTimeLeft", 43),
    ("QueueForLeaseActivated", 44),
    ("QueueForLeaseExpired", 45),
    ("PermanentQueuePurchased", 46),
    ("OutpostStartedOrEnded", 47),
    ("CrossAllianceArmadaVictory", 48),
    ("CrossAllianceArmadaDefeat", 49),
    ("CrossAllianceArmadaPartialVictory", 50),
    ("FactionWeeklyEventsProgress", 51),
    ("FactionWeeklyEventsComplete", 52),
    ("ArmadaPlayerBlocked", 53),
    ("ArmadaPlayerUnblocked", 54),
    ("DynamicCrisisUpdate", 55),
    ("DynamicCrisisFailed", 56),
    ("DynamicCrisisCompleted", 57),
];

// ---- State ----------------------------------------------------------------

/// Original function pointer for `ToastObserver.AreToastsAllowed(Toast)`.
static ORIG_ARE_TOASTS_ALLOWED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Whether all banners should be suppressed (kill switch).
static DISABLE_ALL: AtomicBool = AtomicBool::new(false);

/// Bitfield of disabled ToastState values. Bit N is set when ToastState N is disabled.
/// All 58 values (0-57) fit in a u64 bitfield.
static DISABLED_BITS: AtomicU64 = AtomicU64::new(0);

/// Bitfield tracking which allowed ToastState values have already been logged.
static ALLOWED_LOGGED_BITS: AtomicU64 = AtomicU64::new(0);

/// Bitfield tracking which suppressed ToastState values have already been logged.
static SUPPRESSED_LOGGED_BITS: AtomicU64 = AtomicU64::new(0);

// ---- Type aliases ---------------------------------------------------------

type BoolToastFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject) -> bool;

// ---- Helpers --------------------------------------------------------------

/// Look up the human-readable name for a ToastState value.
fn state_name(value: u32) -> &'static str {
    TOAST_STATES
        .iter()
        .find(|(_, v)| *v == value)
        .map(|(n, _)| *n)
        .unwrap_or("Unknown")
}

/// Convert human-readable ToastState names into a u64 bitfield for O(1) lookup.
fn toast_state_bits(names: &[String]) -> u64 {
    let mut bits: u64 = 0;
    for name in names {
        if let Some(&(_, value)) = TOAST_STATES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            bits |= 1u64 << value;
        }
    }
    bits
}

// ---- Settings callback ----------------------------------------------------

/// Called when banner settings change (sync or incremental update).
///
/// Converts the human-readable name list into a u64 bitfield for O(1) lookup.
pub fn on_settings_changed() {
    DISABLE_ALL.store(crate::settings::disable_all_banners(), Relaxed);

    let names = crate::settings::disabled_banner_types();
    let bits = toast_state_bits(&names);
    DISABLED_BITS.store(bits, Relaxed);
    // Reset dedup bits so the new state is logged on next occurrence.
    ALLOWED_LOGGED_BITS.store(0, Relaxed);
    SUPPRESSED_LOGGED_BITS.store(0, Relaxed);
    debug!(target: "ToastBanner", "Settings updated: disable_all={}, disabled_bits=0x{bits:016X}",
        DISABLE_ALL.load(Relaxed));
}

// ---- Hook -----------------------------------------------------------------

/// Hook for `ToastObserver.AreToastsAllowed(Toast toast)`.
///
/// Returns `false` for suppressed toast types, letting the game skip them naturally.
/// Calls the original first to respect any game-internal suppression logic.
extern "C" fn hook_are_toasts_allowed(this: *mut Il2CppObject, toast: *mut Il2CppObject) -> bool {
    // Call the original to get the game's own decision.
    let orig_ptr = ORIG_ARE_TOASTS_ALLOWED.load(Relaxed);
    let original_result = if !orig_ptr.is_null() {
        let original: BoolToastFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this, toast) }
    } else {
        true
    };

    // If the game already says no, respect that.
    if !original_result {
        return false;
    }

    // Wrap our logic in catch_unwind to prevent panics from aborting the game.
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Kill switch: suppress all banners.
        if DISABLE_ALL.load(Relaxed) {
            return false;
        }

        if toast.is_null() {
            return true;
        }

        // Read Toast.State (ToastState enum backing field).
        let state_offset = TOAST_STATE_OFFSET.load(Relaxed);
        if state_offset == 0 {
            return true;
        }
        let state_value = unsafe { tracker::read_i32(toast as *const (), state_offset) };
        if state_value < 0 {
            return true;
        }
        let state = state_value as u64;
        let name = state_name(state_value as u32);

        let bits = DISABLED_BITS.load(Relaxed);
        if state < 64 && (bits >> state) & 1 == 1 {
            let prev = SUPPRESSED_LOGGED_BITS.fetch_or(1u64 << state, Relaxed);
            if (prev >> state) & 1 == 0 {
                debug!(target: "ToastBanner", "Suppressed: {name}");
            }
            return false;
        }

        if state < 64 {
            let prev = ALLOWED_LOGGED_BITS.fetch_or(1u64 << state, Relaxed);
            if (prev >> state) & 1 == 0 {
                debug!(target: "ToastBanner", "Allowed: {name}");
            }
        }
        true
    }));

    result.unwrap_or(original_result)
}

// ---- Installation ---------------------------------------------------------

/// Install toast banner suppression hooks.
///
/// Hooks `ToastObserver.AreToastsAllowed` which is called before any toast is displayed.
/// A single hook covers all toast enqueue paths.
pub fn install(api: &Il2CppApi) {
    // Resolve Toast field offset for reading the toast state in the hook.
    if let Some(toast_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "Toast") {
        if let Some(offset) = resolver::resolve_field_offset(api, toast_class, "<State>k__BackingField") {
            TOAST_STATE_OFFSET.store(offset, Relaxed);
            debug!(target: "ToastBanner", "Toast.<State>k__BackingField offset: {offset:#x}");
        } else {
            log::warn!(target: "ToastBanner", "Could not resolve Toast.<State>k__BackingField");
        }
    } else {
        log::warn!(target: "ToastBanner", "Toast class not found");
    }

    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "ToastObserver") else {
        log::warn!(target: "ToastBanner", "ToastObserver not found");
        return;
    };

    if let Some(ptr) = tracker::resolve_fn(api, class, "AreToastsAllowed", 1) {
        match crate::hook::engine::install_hook(
            "ToastBanner.AreToastsAllowed",
            ptr,
            hook_are_toasts_allowed as *const (),
        ) {
            Ok(orig) => {
                ORIG_ARE_TOASTS_ALLOWED.store(orig as *mut (), Relaxed);
                debug!(target: "ToastBanner", "AreToastsAllowed hook installed");
            }
            Err(e) => log::warn!(target: "ToastBanner", "Failed to hook AreToastsAllowed: {e}"),
        }
    } else {
        log::warn!(target: "ToastBanner", "AreToastsAllowed not found");
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_states_covers_all_values() {
        // Verify no duplicate names.
        let mut names: Vec<&str> = TOAST_STATES.iter().map(|(n, _)| *n).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate names in TOAST_STATES");
    }

    #[test]
    fn toast_states_no_duplicate_values() {
        let mut values: Vec<u32> = TOAST_STATES.iter().map(|(_, v)| *v).collect();
        let count = values.len();
        values.sort_unstable();
        values.dedup();
        assert_eq!(values.len(), count, "duplicate values in TOAST_STATES");
    }

    #[test]
    fn toast_states_max_fits_in_u64() {
        let max = TOAST_STATES.iter().map(|(_, v)| *v).max().unwrap_or(0);
        assert!(max < 64, "max ToastState value {max} exceeds u64 bitfield");
    }

    #[test]
    fn bitfield_conversion() {
        let names = vec!["Victory".to_string(), "Defeat".to_string(), "Unknown".to_string()];
        let bits = toast_state_bits(&names);
        // Victory=10, Defeat=11
        assert_ne!(bits & (1 << 10), 0, "Victory bit not set");
        assert_ne!(bits & (1 << 11), 0, "Defeat bit not set");
        assert_eq!(bits & (1 << 0), 0, "Standard bit should not be set");
    }

    #[test]
    fn case_insensitive_lookup() {
        let name = "victory";
        let found = TOAST_STATES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name));
        assert!(found.is_some(), "case-insensitive lookup failed for '{name}'");
        assert_eq!(found.unwrap().1, 10);
    }

    #[test]
    fn state_name_known_value() {
        assert_eq!(state_name(0), "Standard");
        assert_eq!(state_name(10), "Victory");
        assert_eq!(state_name(57), "DynamicCrisisCompleted");
    }

    #[test]
    fn state_name_unknown_value() {
        assert_eq!(state_name(13), "Unknown");
        assert_eq!(state_name(99), "Unknown");
    }

    #[test]
    fn json_categories_match_toast_states() {
        let json: std::collections::HashMap<String, Vec<String>> = serde_json::from_str(include_str!(
            "../../../app/modules/app/src/components/toast-banner-categories.json"
        ))
        .unwrap();
        let json_names: std::collections::BTreeSet<&str> =
            json.values().flat_map(|v| v.iter().map(String::as_str)).collect();
        let rust_names: std::collections::BTreeSet<&str> = TOAST_STATES.iter().map(|(n, _)| *n).collect();
        assert_eq!(json_names, rust_names, "JSON categories and TOAST_STATES diverged");
    }
}
