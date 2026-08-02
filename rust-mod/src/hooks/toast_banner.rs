//! Suppress specific toast banner notifications.
//!
//! Hooks `ToastObserver.AreToastsAllowed(Toast)` to check the toast's state against the user's disabled list.
//! Returns `false` for suppressed types, causing the game to skip the toast through its own control flow.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering::Relaxed};

use log::debug;

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::compatibility;
use crate::il2cpp::compatibility_manifest as manifest;
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
/// All 58 values (0-57) fit in a u64.
static DISABLED_BITS: AtomicU64 = AtomicU64::new(0);

/// Bitfield tracking which allowed ToastState values have already been logged.
static ALLOWED_LOGGED_BITS: AtomicU64 = AtomicU64::new(0);

/// Bitfield tracking which suppressed ToastState values have already been logged.
static SUPPRESSED_LOGGED_BITS: AtomicU64 = AtomicU64::new(0);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("ToastBanner");

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

/// Convert human-readable ToastState names into the bitfield used in the hot path.
fn toast_state_bits_for_names(names: &[String]) -> u64 {
    let mut bits: u64 = 0;
    for name in names {
        if let Some(&(_, value)) = TOAST_STATES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            bits |= 1u64 << value;
        }
    }
    bits
}

// ---- Settings callback ----------------------------------------------------

pub(crate) fn on_settings_changed_value(disable_all: bool, names: Vec<String>) {
    DISABLE_ALL.store(disable_all, Relaxed);

    let bits = toast_state_bits_for_names(&names);
    DISABLED_BITS.store(bits, Relaxed);
    // Reset dedup bits so the new state is logged on next occurrence.
    ALLOWED_LOGGED_BITS.store(0, Relaxed);
    SUPPRESSED_LOGGED_BITS.store(0, Relaxed);
    debug!(target: "ToastBanner", "Settings updated: disable_all={disable_all}, disabled_bits=0x{bits:016X}");
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

    HOOK_INFO.run_or(original_result, || {
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
        // FIXME: Direct backing-field read from Toast. Check the dump for a State getter before relying on this
        // offset long-term.
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
    })
}

// ---- Installation ---------------------------------------------------------

/// Install toast banner suppression hooks.
///
/// Hooks `ToastObserver.AreToastsAllowed` which is called before any toast is displayed.
/// A single hook covers all toast enqueue paths.
pub fn install(api: &Il2CppApi) {
    if !compatibility::is_enabled(manifest::TOAST_BANNER) {
        return;
    }

    // Resolve Toast field offset for reading the toast state in the hook.
    if TOAST_STATE_OFFSET.load(Relaxed) == 0 {
        if let Some(toast_class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "Toast") {
            resolver::resolve_field_offset_into(api, toast_class, "<State>k__BackingField", &TOAST_STATE_OFFSET);
        } else {
            log::warn!(target: "ToastBanner", "Toast class not found");
        }
    }

    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "ToastObserver") else {
        log::warn!(target: "ToastBanner", "ToastObserver not found");
        return;
    };

    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "AreToastsAllowed",
        1,
        "ToastBanner.AreToastsAllowed",
        hook_are_toasts_allowed as *const (),
        &ORIG_ARE_TOASTS_ALLOWED,
    );
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
        let bits = toast_state_bits_for_names(&names);
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
