//! Owned fleet scanner data model.

use std::fmt;
use std::time::Instant;

use crate::il2cpp::types::Vector3;

// ---- Game enum values ------------------------------------------------------

pub(super) const DEPLOYED_FLEET_TYPE_PLAYER: i32 = 1;
pub(super) const DEPLOYED_FLEET_TYPE_HOSTILE: i32 = 2;
pub(super) const DEPLOYED_FLEET_TYPE_NPC_INSTANTIATED: i32 = 3;
pub(super) const DEPLOYED_FLEET_TYPE_SENTINEL: i32 = 4;
pub(super) const DEPLOYED_FLEET_TYPE_ALLIANCE: i32 = 5;
pub(super) const DEPLOYED_FLEET_TYPE_CHALLENGE: i32 = 6;

pub(super) const HULL_TYPE_ARMADA_TARGET: i32 = 5;

// ---- Event and snapshot model ---------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PendingFleetEvent {
    EnterSystem { system_id: i64, fleets: Vec<Fleet> },
    Update { reason: &'static str, fleets: Vec<Fleet> },
    Dispose { fleets: Vec<FleetRef> },
}

impl PendingFleetEvent {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::EnterSystem { .. } => "enter_system",
            Self::Update { reason, .. } => reason,
            Self::Dispose { .. } => "fleet_disposed",
        }
    }

    pub(super) fn system_id(&self) -> Option<i64> {
        match self {
            Self::EnterSystem { system_id, .. } => Some(*system_id),
            Self::Update { fleets, .. } => common_fleet_system_id(fleets),
            Self::Dispose { fleets } => common_fleet_ref_system_id(fleets),
        }
    }

    pub(super) fn fleet_count(&self) -> usize {
        match self {
            Self::EnterSystem { fleets, .. } | Self::Update { fleets, .. } => fleets.len(),
            Self::Dispose { fleets } => fleets.len(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct Fleet {
    pub(super) id: i64,
    pub(super) observed_at: Instant,
    pub(super) system_id: Option<i64>,
    pub(super) kind: FleetKind,
    pub(super) combat_class: CombatClass,
    pub(super) fleet_type: Option<i32>,
    pub(super) hull_type: Option<i32>,
    pub(super) hull_name: Option<String>,
    pub(super) local_player: bool,
    pub(super) system_position: Option<Vector3>,
    pub(super) strength: Option<i32>,
    pub(super) level: Option<i32>,
    pub(super) mining: Option<bool>,
    pub(super) max_impulse_speed: Option<f32>,
    pub(super) max_warp_speed: Option<f32>,
    pub(super) travel_direction: Option<Vector3>,
    pub(super) time_since_last_update: Option<f32>,
    pub(super) movement_state: FleetMovementState,
}

impl fmt::Debug for Fleet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fleet")
            .field("id", &self.id)
            .field("system_id", &self.system_id)
            .field("kind", &self.kind)
            .field("combat_class", &self.combat_class)
            .field("fleet_type", &self.fleet_type)
            .field("hull_type", &self.hull_type)
            .field("hull_name", &self.hull_name)
            .field("local_player", &self.local_player)
            .field("system_position", &self.system_position)
            .field("strength", &self.strength)
            .field("level", &self.level)
            .field("mining", &self.mining)
            .field("max_impulse_speed", &self.max_impulse_speed)
            .field("max_warp_speed", &self.max_warp_speed)
            .field("travel_direction", &self.travel_direction)
            .field("time_since_last_update", &self.time_since_last_update)
            .field("movement_state", &self.movement_state)
            .field(
                "observed_age",
                &format_args!("{:.2}s", self.observed_at.elapsed().as_secs_f32()),
            )
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FleetKind {
    Own,
    Player,
    Hostile,
    Armada,
    Npc,
    Sentinel,
    Alliance,
    Challenge,
    Other(i32),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FleetMovementState {
    Unknown,
    Stopped,
    Impulsing,
    Warping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CombatClass {
    Explorer,
    Destroyer,
    Battleship,
    Survey,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FleetRef {
    pub(super) id: i64,
    pub(super) system_id: Option<i64>,
}

// ---- Store results ---------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(super) enum StoreEnterResult {
    Replaced {
        stored: usize,
        changes: Vec<FleetStoreChange>,
    },
    Upserted {
        added: usize,
        updated: usize,
        unchanged: usize,
        total: usize,
        changes: Vec<FleetStoreChange>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StoreUpdateResult {
    pub(super) inserted: usize,
    pub(super) updated: usize,
    pub(super) unchanged: usize,
    pub(super) ignored: usize,
    pub(super) total: usize,
    pub(super) changes: Vec<FleetStoreChange>,
    pub(super) ignored_fleet_ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct StoreRemoveResult {
    pub(super) vanished: usize,
    pub(super) ignored: usize,
    pub(super) total: usize,
    pub(super) changes: Vec<FleetStoreChange>,
    pub(super) ignored_fleet_ids: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FleetStoreChange {
    pub(super) action: FleetStoreAction,
    pub(super) fleet: Fleet,
    pub(super) changed_fields: Vec<FleetFieldChange>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FleetFieldChange {
    pub(super) name: &'static str,
    pub(super) old_value: String,
    pub(super) new_value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FleetStoreAction {
    Inserted,
    Updated,
    Vanished,
}

// ---- Classification --------------------------------------------------------

/// Derive the combat class from hull name first, then hull type.
pub(super) fn classify_combat_class(hull_name: Option<&str>, hull_type: Option<i32>) -> CombatClass {
    if let Some(hull_name) = hull_name {
        if hull_name.contains("Explorer") {
            return CombatClass::Explorer;
        }
        if hull_name.contains("Destroyer") {
            return CombatClass::Destroyer;
        }
        if hull_name.contains("Battleship") {
            return CombatClass::Battleship;
        }
        if hull_name.contains("Survey") {
            return CombatClass::Survey;
        }
    }

    match hull_type {
        Some(0) => CombatClass::Destroyer,
        Some(1) => CombatClass::Survey,
        Some(2) => CombatClass::Explorer,
        Some(3) => CombatClass::Battleship,
        _ => CombatClass::Unknown,
    }
}

/// Classify a fleet from local-player flag and raw game fleet type.
pub(super) fn classify_fleet(local_player: bool, fleet_type: Option<i32>, hull_type: Option<i32>) -> FleetKind {
    if local_player {
        return FleetKind::Own;
    }

    match fleet_type {
        Some(DEPLOYED_FLEET_TYPE_PLAYER) => FleetKind::Player,
        Some(DEPLOYED_FLEET_TYPE_HOSTILE) if hull_type == Some(HULL_TYPE_ARMADA_TARGET) => FleetKind::Armada,
        Some(DEPLOYED_FLEET_TYPE_HOSTILE) => FleetKind::Hostile,
        Some(DEPLOYED_FLEET_TYPE_NPC_INSTANTIATED) => FleetKind::Npc,
        Some(DEPLOYED_FLEET_TYPE_SENTINEL) => FleetKind::Sentinel,
        Some(DEPLOYED_FLEET_TYPE_ALLIANCE) => FleetKind::Alliance,
        Some(DEPLOYED_FLEET_TYPE_CHALLENGE) => FleetKind::Challenge,
        Some(value) => FleetKind::Other(value),
        None => FleetKind::Unknown,
    }
}

/// Return a common system ID when all fleet snapshots agree.
fn common_fleet_system_id(fleets: &[Fleet]) -> Option<i64> {
    common_system_id(fleets.iter().filter_map(|fleet| fleet.system_id))
}

/// Return a common system ID when all fleet references agree.
fn common_fleet_ref_system_id(fleets: &[FleetRef]) -> Option<i64> {
    common_system_id(fleets.iter().filter_map(|fleet| fleet.system_id))
}

/// Return `Some(system)` only for non-empty, single-system iterators.
fn common_system_id(mut system_ids: impl Iterator<Item = i64>) -> Option<i64> {
    let first = system_ids.next()?;
    system_ids.all(|system_id| system_id == first).then_some(first)
}
