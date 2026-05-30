//! Fleet scanner store, pending queue, and change formatting.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};

use log::{debug, trace};

use super::model::*;
use crate::il2cpp::types::Vector3;

const MAX_PENDING_FLEET_EVENTS: usize = 1000;

pub(super) static FLEET_STORE: Mutex<Option<FleetStore>> = Mutex::new(None);
pub(super) static OWN_FLEET_STORE: LazyLock<Mutex<HashMap<i64, Fleet>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
pub(super) static PENDING_FLEET_EVENTS: Mutex<VecDeque<PendingFleetEvent>> = Mutex::new(VecDeque::new());

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FleetStore {
    pub(super) system_id: Option<i64>,
    pub(super) fleets: HashMap<i64, Fleet>,
}

// ---- Pending queue ---------------------------------------------------------

/// Queue an owned fleet event without retaining any Unity pointers.
pub(super) fn queue_pending_fleet_event(event: PendingFleetEvent) {
    let event_kind = event.kind();
    let event_system_id = event.system_id();
    let fleet_count = event.fleet_count();
    let mut queue = PENDING_FLEET_EVENTS.lock().unwrap_or_else(|e| e.into_inner());

    if queue.len() == MAX_PENDING_FLEET_EVENTS {
        queue.pop_front();
        trace!(
            target: "FleetScanner",
            "Pending fleet queue overflow: dropped_oldest=1, queued={MAX_PENDING_FLEET_EVENTS}",
        );
    }

    queue.push_back(event);
    trace!(
        target: "FleetScanner",
        "Fleet event queued: reason=no_viewed_system, event={event_kind}, system={}, fleets={fleet_count}, queued={}",
        format_optional_i64(event_system_id),
        queue.len(),
    );
}

/// Drain queued events so callers can process them outside the queue lock.
pub(super) fn drain_pending_fleet_events() -> Vec<PendingFleetEvent> {
    let mut queue = PENDING_FLEET_EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    queue.drain(..).collect()
}

/// Return all hostile fleets from the current viewed-system store.
pub(super) fn hostile_fleets() -> Vec<Fleet> {
    let guard = FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = guard.as_ref() else {
        return Vec::new();
    };

    store
        .fleets
        .values()
        .filter(|fleet| fleet.kind == FleetKind::Hostile)
        .cloned()
        .collect()
}

/// Return one known own fleet snapshot by ID.
pub(super) fn own_fleet(fleet_id: i64) -> Option<Fleet> {
    OWN_FLEET_STORE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&fleet_id)
        .cloned()
}

// ---- Store mutations -------------------------------------------------------

/// Outcome of merging an incoming fleet into an existing viewed-system store entry.
enum UpsertOutcome {
    /// No field changed. The entry was refreshed in place without producing a change record.
    Unchanged,
    /// At least one field changed. Carries the resulting change record.
    Updated(FleetStoreChange),
}

/// Merge an incoming `fleet` into its existing store entry, reporting whether anything changed.
///
/// A systemless incoming update inherits the existing system id, so the last known system is kept until an
/// explicit dispose clears it.
fn merge_existing_fleet(existing: &mut Fleet, mut fleet: Fleet) -> UpsertOutcome {
    fleet.system_id = fleet.system_id.or(existing.system_id);
    let changed_fields = diff_fleet(existing, &fleet);
    if changed_fields.is_empty() {
        *existing = fleet;
        UpsertOutcome::Unchanged
    } else {
        *existing = fleet.clone();
        UpsertOutcome::Updated(FleetStoreChange {
            action: FleetStoreAction::Updated,
            fleet,
            changed_fields,
        })
    }
}

/// Insert a brand-new fleet into the store and build its `Inserted` change record.
fn insert_new_fleet(store: &mut FleetStore, fleet: Fleet) -> FleetStoreChange {
    store.fleets.insert(fleet.id, fleet.clone());
    FleetStoreChange {
        action: FleetStoreAction::Inserted,
        fleet,
        changed_fields: Vec::new(),
    }
}

/// Replace or upsert the viewed-system store from an enter-system fleet batch.
pub(super) fn store_enter_fleets(system_id: i64, fleets: Vec<Fleet>) -> StoreEnterResult {
    let mut guard = FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let current_system_id = guard.as_ref().and_then(|store| store.system_id);
    let fleets = fleets.into_iter().filter(is_viewed_system_fleet).collect::<Vec<_>>();

    if current_system_id != Some(system_id) {
        let mut fleet_map = HashMap::with_capacity(fleets.len());
        for fleet in fleets {
            fleet_map.insert(fleet.id, fleet);
        }

        let stored = fleet_map.len();
        let changes = fleet_map
            .values()
            .cloned()
            .map(|fleet| FleetStoreChange {
                action: FleetStoreAction::Inserted,
                fleet,
                changed_fields: Vec::new(),
            })
            .collect::<Vec<_>>();

        *guard = Some(FleetStore {
            system_id: Some(system_id),
            fleets: fleet_map,
        });

        debug!(
            target: "FleetScanner",
            "Fleet store reset: reason=system_changed, system={}",
            format_optional_i64(Some(system_id)),
        );

        return StoreEnterResult::Replaced { stored, changes };
    }

    let store = guard.get_or_insert_with(|| FleetStore {
        system_id: Some(system_id),
        fleets: HashMap::new(),
    });
    let mut added = 0;
    let mut updated = 0;
    let mut unchanged = 0;
    let mut changes = Vec::with_capacity(fleets.len());

    for fleet in fleets {
        if let Some(existing) = store.fleets.get_mut(&fleet.id) {
            match merge_existing_fleet(existing, fleet) {
                UpsertOutcome::Unchanged => unchanged += 1,
                UpsertOutcome::Updated(change) => {
                    updated += 1;
                    changes.push(change);
                }
            }
        } else {
            added += 1;
            changes.push(insert_new_fleet(store, fleet));
        }
    }

    StoreEnterResult::Upserted {
        added,
        updated,
        unchanged,
        total: store.fleets.len(),
        changes,
    }
}

/// Insert or update fleets that belong to the viewed system.
pub(super) fn store_update_fleets(system_id: i64, fleets: Vec<Fleet>) -> StoreUpdateResult {
    let mut guard = FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let store = ensure_store_for_system(&mut guard, system_id);

    let mut inserted = 0;
    let mut updated = 0;
    let mut unchanged = 0;
    let mut ignored = 0;
    let mut changes = Vec::with_capacity(fleets.len());
    let mut ignored_fleet_ids = Vec::new();

    for fleet in fleets {
        if !is_viewed_system_fleet(&fleet) {
            continue;
        }

        if let Some(existing) = store.fleets.get_mut(&fleet.id) {
            match merge_existing_fleet(existing, fleet) {
                UpsertOutcome::Unchanged => unchanged += 1,
                UpsertOutcome::Updated(change) => {
                    updated += 1;
                    changes.push(change);
                }
            }
        } else if fleet.system_id == Some(system_id) {
            inserted += 1;
            changes.push(insert_new_fleet(store, fleet));
        } else {
            ignored += 1;
            ignored_fleet_ids.push(fleet.id);
        }
    }

    StoreUpdateResult {
        inserted,
        updated,
        unchanged,
        ignored,
        total: store.fleets.len(),
        changes,
        ignored_fleet_ids,
    }
}

/// Ensure update events can create an empty store for the active viewed system.
fn ensure_store_for_system(guard: &mut Option<FleetStore>, system_id: i64) -> &mut FleetStore {
    if guard.as_ref().and_then(|store| store.system_id) != Some(system_id) {
        *guard = Some(FleetStore {
            system_id: Some(system_id),
            fleets: HashMap::new(),
        });
    }

    guard.as_mut().expect("store was initialized")
}

/// Remove fleet snapshots by ID from the viewed-system store.
pub(super) fn store_remove_fleets(system_id: i64, fleets: Vec<FleetRef>) -> StoreRemoveResult {
    let mut guard = FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(store) = guard.as_mut() else {
        return StoreRemoveResult {
            vanished: 0,
            ignored: fleets.len(),
            total: 0,
            changes: Vec::new(),
            ignored_fleet_ids: fleets.into_iter().map(|fleet| fleet.id).collect(),
        };
    };

    if store.system_id != Some(system_id) {
        return StoreRemoveResult {
            vanished: 0,
            ignored: fleets.len(),
            total: store.fleets.len(),
            changes: Vec::new(),
            ignored_fleet_ids: fleets.into_iter().map(|fleet| fleet.id).collect(),
        };
    }

    let mut vanished = 0;
    let mut ignored = 0;
    let mut changes = Vec::with_capacity(fleets.len());
    let mut ignored_fleet_ids = Vec::new();

    for fleet_ref in fleets {
        if let Some(fleet) = store.fleets.remove(&fleet_ref.id) {
            vanished += 1;
            changes.push(FleetStoreChange {
                action: FleetStoreAction::Vanished,
                fleet,
                changed_fields: Vec::new(),
            });
        } else {
            ignored += 1;
            ignored_fleet_ids.push(fleet_ref.id);
        }
    }

    StoreRemoveResult {
        vanished,
        ignored,
        total: store.fleets.len(),
        changes,
        ignored_fleet_ids,
    }
}

/// Insert or update own fleets in the global own-fleet store.
pub(super) fn store_own_fleets(fleets: &[Fleet]) -> Vec<FleetStoreChange> {
    let mut store = OWN_FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut changes = Vec::new();

    for fleet in fleets.iter().filter(|fleet| fleet.kind == FleetKind::Own) {
        let mut fleet = fleet.clone();
        if let Some(existing) = store.get_mut(&fleet.id) {
            // A systemless own update is not a leave signal; keep the last known system until an explicit dispose.
            fleet.system_id = fleet.system_id.or(existing.system_id);
            let changed_fields = diff_fleet(existing, &fleet);
            if changed_fields.is_empty() {
                *existing = fleet;
            } else {
                *existing = fleet.clone();
                changes.push(FleetStoreChange {
                    action: FleetStoreAction::Updated,
                    fleet,
                    changed_fields,
                });
            }
        } else {
            store.insert(fleet.id, fleet.clone());
            changes.push(FleetStoreChange {
                action: FleetStoreAction::Inserted,
                fleet,
                changed_fields: Vec::new(),
            });
        }
    }

    changes
}

/// Invalidate live system data for own fleets that are no longer locally deployed.
pub(super) fn invalidate_own_fleet_refs(fleets: &[FleetRef]) -> Vec<FleetStoreChange> {
    let mut store = OWN_FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut changes = Vec::new();

    for fleet_ref in fleets {
        let Some(existing) = store.get_mut(&fleet_ref.id) else {
            continue;
        };

        let mut invalidated = existing.clone();
        invalidated.observed_at = std::time::Instant::now();
        invalidated.system_id = None;
        invalidated.system_position = None;
        invalidated.travel_direction = None;
        invalidated.time_since_last_update = None;
        invalidated.movement_state = FleetMovementState::Unknown;

        let changed_fields = diff_fleet(existing, &invalidated);
        if changed_fields.is_empty() {
            *existing = invalidated;
            continue;
        }

        *existing = invalidated.clone();
        changes.push(FleetStoreChange {
            action: FleetStoreAction::Updated,
            fleet: invalidated,
            changed_fields,
        });
    }

    changes
}

fn is_viewed_system_fleet(fleet: &Fleet) -> bool {
    fleet.kind != FleetKind::Own
}

// ---- Change logging --------------------------------------------------------

/// Emit fleet changes at debug level.
pub(super) fn log_fleet_changes(summary: &str, changes: &[FleetStoreChange]) {
    log_fleet_changes_debug(summary, changes);
}

/// Emit fleet changes at trace level.
pub(super) fn trace_fleet_changes(summary: &str, changes: &[FleetStoreChange]) {
    let message = format_fleet_changes(summary, changes);
    trace!(target: "FleetScanner", "{message}");
}

pub(super) fn is_movement_only_update(changes: &[FleetStoreChange]) -> bool {
    !changes.is_empty()
        && changes.iter().all(|change| {
            change.action == FleetStoreAction::Updated
                && !change.changed_fields.is_empty()
                && change
                    .changed_fields
                    .iter()
                    .all(|field| matches!(field.name, "position" | "direction" | "age"))
        })
}

/// Format and emit a debug fleet-change block.
fn log_fleet_changes_debug(summary: &str, changes: &[FleetStoreChange]) {
    let message = format_fleet_changes(summary, changes);
    debug!(target: "FleetScanner", "{message}");
}

/// Format a summary with optional per-fleet detail lines.
fn format_fleet_changes(summary: &str, changes: &[FleetStoreChange]) -> String {
    if changes.is_empty() {
        return summary.to_string();
    }

    let mut changes = changes.iter().collect::<Vec<_>>();
    changes.sort_by_key(|change| change.fleet.id);

    let mut message = summary.to_string();
    for change in changes {
        message.push('\n');
        message.push('\t');
        message.push_str(&format_fleet_change(change));
    }
    message
}

/// Format one inserted, updated, or vanished fleet detail line.
fn format_fleet_change(change: &FleetStoreChange) -> String {
    let action = match change.action {
        FleetStoreAction::Inserted => "inserted",
        FleetStoreAction::Updated => "updated",
        FleetStoreAction::Vanished => "vanished",
    };
    let fleet = &change.fleet;

    let mut message = format!(
        "Fleet {action}: id={}, kind={:?}, movement={:?}, combat_class={:?}, ship={}, hull={}, fleet_type={}, level={}, strength={}",
        fleet.id,
        fleet.kind,
        fleet.movement_state,
        fleet.combat_class,
        format_optional_str(&fleet.hull_name),
        format_hull_label(&fleet.hull_name, fleet.hull_type),
        format_optional_i32(fleet.fleet_type),
        format_optional_i32(fleet.level),
        format_optional_i32(fleet.strength),
    );

    if change.action == FleetStoreAction::Updated && !change.changed_fields.is_empty() {
        let fields = change
            .changed_fields
            .iter()
            .map(|field| format!("{}={}->{}", field.name, field.old_value, field.new_value))
            .collect::<Vec<_>>()
            .join(", ");
        message = format!(
            "Fleet {action}: id={}, kind={:?}, combat_class={:?}, ship={}, {fields}",
            fleet.id,
            fleet.kind,
            fleet.combat_class,
            format_optional_str(&fleet.hull_name),
        );
    }

    message
}

// ---- Diff helpers ----------------------------------------------------------

/// Build a field-level diff for compact update logging.
fn diff_fleet(old: &Fleet, new: &Fleet) -> Vec<FleetFieldChange> {
    let mut changes = Vec::new();

    push_field_change(&mut changes, "system", old.system_id, new.system_id, format_optional_i64);
    push_field_change(&mut changes, "kind", old.kind, new.kind, |value| format!("{value:?}"));
    push_field_change(&mut changes, "movement", old.movement_state, new.movement_state, |value| {
        format!("{value:?}")
    });
    push_field_change(&mut changes, "combat_class", old.combat_class, new.combat_class, |value| {
        format!("{value:?}")
    });
    push_field_change(&mut changes, "fleet_type", old.fleet_type, new.fleet_type, format_optional_i32);
    push_field_change(&mut changes, "hull_type", old.hull_type, new.hull_type, format_optional_i32);
    push_field_change(
        &mut changes,
        "ship",
        old.hull_name.as_deref(),
        new.hull_name.as_deref(),
        format_optional_str_value,
    );
    push_field_change(&mut changes, "local_player", old.local_player, new.local_player, |value| {
        value.to_string()
    });
    push_field_change(
        &mut changes,
        "position",
        old.system_position,
        new.system_position,
        format_optional_vector3,
    );
    push_field_change(&mut changes, "strength", old.strength, new.strength, format_optional_i32);
    push_field_change(&mut changes, "level", old.level, new.level, format_optional_i32);
    push_field_change(&mut changes, "mining", old.mining, new.mining, format_optional_bool);
    push_field_change(
        &mut changes,
        "impulse",
        old.max_impulse_speed,
        new.max_impulse_speed,
        format_optional_f32,
    );
    push_field_change(
        &mut changes,
        "warp",
        old.max_warp_speed,
        new.max_warp_speed,
        format_optional_f32,
    );
    push_field_change(
        &mut changes,
        "direction",
        old.travel_direction,
        new.travel_direction,
        format_optional_vector3,
    );
    push_field_change(
        &mut changes,
        "age",
        old.time_since_last_update,
        new.time_since_last_update,
        format_optional_f32,
    );

    changes
}

/// Add one formatted diff entry when a copied field changed.
fn push_field_change<T>(
    changes: &mut Vec<FleetFieldChange>,
    name: &'static str,
    old_value: T,
    new_value: T,
    format: impl Fn(T) -> String,
) where
    T: PartialEq + Copy,
{
    if old_value != new_value {
        changes.push(FleetFieldChange {
            name,
            old_value: format(old_value),
            new_value: format(new_value),
        });
    }
}

// ---- Formatting ------------------------------------------------------------

/// Format optional system IDs for human-readable logs.
pub(super) fn format_optional_i64(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

/// Format fleet IDs in stable order for trace logs.
pub(super) fn format_fleet_ids(ids: &[i64]) -> String {
    if ids.is_empty() {
        return "[]".to_string();
    }

    let mut ids = ids.to_vec();
    ids.sort_unstable();
    format!("{ids:?}")
}

/// Format optional i32 values for logs.
fn format_optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

/// Borrow an optional string for log formatting.
fn format_optional_str(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("unknown")
}

/// Format optional borrowed strings for field diffs.
fn format_optional_str_value(value: Option<&str>) -> String {
    value.unwrap_or("unknown").to_string()
}

/// Format optional bool values for field diffs.
fn format_optional_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

/// Format optional f32 values with stable precision for field diffs.
fn format_optional_f32(value: Option<f32>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| format!("{value:.2}"))
}

/// Format optional Vector3 values with stable precision for field diffs.
fn format_optional_vector3(value: Option<Vector3>) -> String {
    value.map_or_else(
        || "unknown".to_string(),
        |value| format!("({:.2}, {:.2}, {:.2})", value.x, value.y, value.z),
    )
}

/// Format hull name and raw hull type as one compact label.
fn format_hull_label(name: &Option<String>, hull_type: Option<i32>) -> String {
    match (name.as_deref(), hull_type) {
        (Some(name), Some(hull_type)) => format!("{name}({hull_type})"),
        (Some(name), None) => name.to_string(),
        (None, Some(hull_type)) => format!("unknown({hull_type})"),
        (None, None) => "unknown".to_string(),
    }
}

#[cfg(test)]
pub(super) fn max_pending_fleet_events() -> usize {
    MAX_PENDING_FLEET_EVENTS
}
