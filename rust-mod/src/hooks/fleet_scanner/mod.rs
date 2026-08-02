//! Fleet scanner hooks for the currently viewed system.
//!
//! Deployment events are converted to owned snapshots immediately. Runtime Unity/IL2CPP pointers are never stored
//! beyond the read operation. Events observed before a concrete system view is known are held in a bounded pending
//! queue and flushed once the viewed system changes.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::{debug, trace, warn};

use crate::hook::safety::HookInfo;
use crate::hooks::navigation_view;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::compatibility;
use crate::il2cpp::compatibility_manifest as manifest;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

mod model;
mod store;

use super::fleet_bar;
use super::target_viewer::{self, TargetViewerEvent};
use model::*;
use store::*;

const LOG_TARGET: &str = "FleetScanner";
const INTERCEPT_EPSILON: f32 = 1.0e-5;
const DEPLOYED_STATE_IDLE: i32 = 0;
const DEPLOYED_STATE_MOVING: i32 = 1;
const DEPLOYED_STATE_WARPING: i32 = 2;
const DEPLOYED_STATE_BATTLING: i32 = 3;
const DEPLOYED_STATE_RECALL: i32 = 4;
const DEPLOYED_STATE_DOCKED: i32 = 5;
const DEPLOYED_STATE_PRE_BATTLE: i32 = 6;

// ---- Dynamically resolved functions ---------------------------------------

/// `FleetDeployedData.get_ID()`.
static GET_FLEET_ID_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_SystemPosition()`.
static GET_FLEET_SYSTEM_POSITION_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_Address()`.
static GET_FLEET_ADDRESS_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_IsLocalPlayer()`.
static GET_FLEET_IS_LOCAL_PLAYER_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_FleetType()`.
static GET_FLEET_TYPE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_Strength()`.
static GET_FLEET_STRENGTH_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_Level()`.
static GET_FLEET_LEVEL_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_IsMining()`.
static GET_FLEET_IS_MINING_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_Hull()`.
static GET_FLEET_HULL_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_MaxImpulseSpeed()`.
static GET_FLEET_MAX_IMPULSE_SPEED_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_MaxWarpSpeed()`.
static GET_FLEET_MAX_WARP_SPEED_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_TravelDirection()`.
static GET_FLEET_TRAVEL_DIRECTION_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_TimeSinceLastUpdate()`.
static GET_FLEET_TIME_SINCE_LAST_UPDATE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetDeployedData.get_CurrentState()`.
static GET_FLEET_CURRENT_STATE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `HullSpec.get_Type()`.
static GET_HULL_TYPE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `HullSpec.get_Name()`.
static GET_HULL_NAME_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `NodeAddress.get_System()`.
static GET_NODE_ADDRESS_SYSTEM_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `NavigationManager.CanSelectPoi(long) -> bool`.
static NAVIGATION_MANAGER_CAN_SELECT_POI_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `NavigationManager.SelectPoi(long) -> bool`.
static NAVIGATION_MANAGER_SELECT_POI_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `CourseData.get_FleetID()`.
static GET_COURSE_FLEET_ID_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `CourseData.get_SystemPosition()`.
static GET_COURSE_SYSTEM_POSITION_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `ChangeViewData.address`.
static OFFSET_CHANGE_VIEW_DATA_ADDRESS: AtomicUsize = AtomicUsize::new(0);

// ---- Original trampolines --------------------------------------------------

static ORIG_COURSE_END: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_DATA_ADDED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_DATA_UPDATED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_DATA_DISPOSED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_DATA_ENTER_SYSTEM: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_DATA_EXIT_SYSTEM: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_EVENT_FLEET_ADDED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_EVENT_FLEET_DISPOSED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_EVENT_FLEET_UPDATED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_EVENT_FLEET_STATE_CHANGED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_PLAYER_FLEET_UPDATED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_CHANGE_VIEW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_NAVIGATION_MANAGER_ENABLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_NAVIGATION_MANAGER_DISABLE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

static NAVIGATION_MANAGER_INSTANCE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

static HOOK_INFO: HookInfo = HookInfo::new(LOG_TARGET);

// ---- Type aliases ----------------------------------------------------------

type InstanceFleetEventFn =
    unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppList<*mut Il2CppObject>, *const MethodInfo);
type InstanceFleetsSystemFn =
    unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, *mut Il2CppList<*mut Il2CppObject>, *const MethodInfo);
type ContainerFleetEventFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, *const MethodInfo);
type CourseEventFn = unsafe extern "C" fn(*mut Il2CppList<*mut Il2CppObject>, *const MethodInfo);
type ChangeViewFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, i64, *const MethodInfo);
type ActionFn = unsafe extern "C" fn(*mut Il2CppObject);

/// Call a hook's stored original trampoline, forwarding the given arguments.
///
/// Loads the trampoline pointer from `$slot`, transmutes it to `$fn_ty`, and invokes it. A null slot (hook not
/// installed) is a no-op. This collapses the repeated load/null-check/transmute/call boilerplate every hook needs.
macro_rules! call_original {
    ($slot:expr, $fn_ty:ty $(, $arg:expr)* $(,)?) => {{
        let orig = $slot.load(Relaxed);
        if !orig.is_null() {
            let original: $fn_ty = unsafe { std::mem::transmute(orig) };
            unsafe { original($($arg),*) };
        }
    }};
}

// ---- Hooks ----------------------------------------------------------------

/// Observe finished courses for diagnostics.
extern "C" fn hook_course_end(courses: *mut Il2CppList<*mut Il2CppObject>, method_info: *const MethodInfo) {
    call_original!(ORIG_COURSE_END, CourseEventFn, courses, method_info);

    HOOK_INFO.run(|| process_course_end(courses));
}

extern "C" fn hook_fleet_data_added(
    this: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_FLEET_DATA_ADDED, InstanceFleetEventFn, this, fleets, method_info);

    HOOK_INFO.run(|| process_fleets_updated("fleet_added", fleets));
}

extern "C" fn hook_fleet_data_updated(
    this: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_FLEET_DATA_UPDATED, InstanceFleetEventFn, this, fleets, method_info);

    HOOK_INFO.run(|| process_fleets_updated("fleet_update", fleets));
}

extern "C" fn hook_fleet_data_disposed(
    this: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_FLEET_DATA_DISPOSED, InstanceFleetEventFn, this, fleets, method_info);

    HOOK_INFO.run(|| process_fleets_disposed(fleets));
}

extern "C" fn hook_fleet_data_enter_system(
    this: *mut Il2CppObject,
    address: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    call_original!(
        ORIG_FLEET_DATA_ENTER_SYSTEM,
        InstanceFleetsSystemFn,
        this,
        address,
        fleets,
        method_info
    );

    HOOK_INFO.run(|| process_enter_system(address, fleets));
}

extern "C" fn hook_fleet_data_exit_system(
    this: *mut Il2CppObject,
    address: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    call_original!(
        ORIG_FLEET_DATA_EXIT_SYSTEM,
        InstanceFleetsSystemFn,
        this,
        address,
        fleets,
        method_info
    );

    HOOK_INFO.run(|| process_exit_system(address, fleets));
}

extern "C" fn hook_player_fleet_updated(
    this: *mut Il2CppObject,
    fleet: *mut Il2CppObject,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_PLAYER_FLEET_UPDATED, ContainerFleetEventFn, this, fleet, method_info);

    HOOK_INFO.run(|| process_player_fleet_updated(fleet));
}

extern "C" fn hook_event_fleet_added(
    this: *mut Il2CppObject,
    fleet: *mut Il2CppObject,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_EVENT_FLEET_ADDED, ContainerFleetEventFn, this, fleet, method_info);

    if ORIG_FLEET_DATA_ADDED.load(Relaxed).is_null() {
        HOOK_INFO.run(|| process_single_fleet_updated("fleet_added", fleet));
    }
}

extern "C" fn hook_event_fleet_disposed(
    this: *mut Il2CppObject,
    fleet: *mut Il2CppObject,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_EVENT_FLEET_DISPOSED, ContainerFleetEventFn, this, fleet, method_info);

    if ORIG_FLEET_DATA_DISPOSED.load(Relaxed).is_null() {
        HOOK_INFO.run(|| process_single_fleet_disposed(fleet));
    }
}

extern "C" fn hook_event_fleet_updated(
    this: *mut Il2CppObject,
    fleet: *mut Il2CppObject,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_EVENT_FLEET_UPDATED, ContainerFleetEventFn, this, fleet, method_info);

    if ORIG_FLEET_DATA_UPDATED.load(Relaxed).is_null() {
        HOOK_INFO.run(|| process_single_fleet_updated("fleet_update", fleet));
    }
}

extern "C" fn hook_event_fleet_state_changed(
    this: *mut Il2CppObject,
    fleet: *mut Il2CppObject,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_EVENT_FLEET_STATE_CHANGED, ContainerFleetEventFn, this, fleet, method_info);

    HOOK_INFO.run(|| process_single_fleet_updated("fleet_state_change", fleet));
}

/// Post-hook on `NavigationManager.ChangeView(ChangeViewData, Nullable<ZoomLevels>)`.
///
/// Reads the `NodeAddress` from `ChangeViewData.address` and extracts the system ID.
/// This replaces the old `TriggerDidChangeViewEvent` hook which was inlined by MSVC on Windows.
extern "C" fn hook_change_view(
    this: *mut Il2CppObject,
    data: *mut Il2CppObject,
    zoom: i64,
    method_info: *const MethodInfo,
) {
    call_original!(ORIG_CHANGE_VIEW, ChangeViewFn, this, data, zoom, method_info);

    HOOK_INFO.run(|| {
        if data.is_null() {
            clear_viewed_system("change_view_null_data");
            return;
        }

        let address_offset = OFFSET_CHANGE_VIEW_DATA_ADDRESS.load(Relaxed);
        if address_offset == 0 {
            warn!(target: LOG_TARGET, "ChangeView fired but ChangeViewData.address offset is unresolved");
            clear_viewed_system("change_view_address_offset_unresolved");
            return;
        }

        let address = unsafe { tracker::read_ptr(data as *const (), address_offset) };
        if address.is_null() {
            warn!(target: LOG_TARGET, "ChangeView fired but ChangeViewData.address is null");
            clear_viewed_system("change_view_null_address");
            return;
        }
        let system_id = node_address_system(address as *mut Il2CppObject);
        if system_id.is_none() {
            debug!(target: LOG_TARGET, "ChangeView fired without a concrete system");
            clear_viewed_system("change_view_without_system");
            return;
        }
        set_viewed_system(system_id);
    });
}

/// Track the NavigationManager instance while it is enabled and usable for POI selection.
extern "C" fn hook_navigation_manager_enable(this: *mut Il2CppObject) {
    call_original!(ORIG_NAVIGATION_MANAGER_ENABLE, ActionFn, this);

    HOOK_INFO.run(|| {
        NAVIGATION_MANAGER_INSTANCE.store(this as *mut (), Relaxed);
        trace!(target: LOG_TARGET, "NavigationManager tracked for target selection");
    });
}

/// Release the tracked NavigationManager and clear the viewed system when it is disabled.
///
/// Unity runs `OnDisable` before destroying the object, so this is safe for clean-up.
/// This replaces the old `TriggerLeaveNavigationViewEvent` hook.
extern "C" fn hook_navigation_manager_disable(this: *mut Il2CppObject) {
    call_original!(ORIG_NAVIGATION_MANAGER_DISABLE, ActionFn, this);

    HOOK_INFO.run(|| {
        if NAVIGATION_MANAGER_INSTANCE
            .compare_exchange(this as *mut (), std::ptr::null_mut(), Relaxed, Relaxed)
            .is_ok()
        {
            clear_viewed_system("navigation_manager_disabled");
            trace!(target: LOG_TARGET, "NavigationManager instance released on disable");
        }
    });
}

// ---- Processing ------------------------------------------------------------

/// The own fleet currently selected in the viewed system, resolved from the fleet bar and the own-fleet store.
///
/// All preconditions for acting on the own fleet are checked here: a system must be viewed, a fleet must be selected
/// in that same system, and a live snapshot must exist. Returns `None` (with a diagnostic) when any fails, e.g. after
/// the fleet was destroyed or left the system.
fn selected_own_fleet() -> Option<Fleet> {
    let system_id = navigation_view::current_viewed_system_id()?;

    let Some(selected_fleet) = fleet_bar::selected_fleet() else {
        debug!(target: LOG_TARGET, "Skipped: selected fleet unavailable for hostile selection");
        return None;
    };

    let Some(fleet_id) = selected_fleet.id else {
        debug!(target: LOG_TARGET, "Skipped: selected fleet ID unavailable, {selected_fleet}");
        return None;
    };

    if selected_fleet.system_id != Some(system_id) {
        debug!(
            target: LOG_TARGET,
            "Skipped: selected fleet is not in viewed system, viewed_system={system_id}, {selected_fleet}",
        );
        return None;
    }

    let Some(own_fleet) = own_fleet(fleet_id).or_else(|| selected_own_fleet_from_location_data(&selected_fleet)) else {
        debug!(
            target: LOG_TARGET,
            "Skipped: selected own fleet snapshot unavailable, viewed_system={system_id}, {selected_fleet}",
        );
        return None;
    };

    Some(own_fleet)
}

fn selected_own_fleet_from_location_data(selected_fleet: &fleet_bar::SelectedFleet) -> Option<Fleet> {
    let location_data = selected_fleet.location_data?;
    let fleet = inspect_fleet(location_data)?;

    if fleet.kind != FleetKind::Own || Some(fleet.id) != selected_fleet.id {
        trace!(
            target: LOG_TARGET,
            "Selected fleet LocationData ignored: expected_id={}, snapshot_id={}, kind={:?}",
            format_optional_i64(selected_fleet.id),
            fleet.id,
            fleet.kind,
        );
        return None;
    }

    let changes = store_own_fleets(std::slice::from_ref(&fleet));
    if changes.is_empty() {
        trace!(
            target: LOG_TARGET,
            "Own fleet store unchanged: reason=selected_fleet_location_data, fleet_id={}",
            fleet.id,
        );
    } else {
        log_fleet_changes(
            &format!(
                "Own fleet store update: reason=selected_fleet_location_data, changed={}",
                changes.len()
            ),
            &changes,
        );
    }

    Some(fleet)
}

/// Select the hostile that can be intercepted fastest from the current fleet scan.
///
/// Returns `true` only when the target was selected in game.
pub(crate) fn try_select_next_hostile() -> bool {
    let Some(own_fleet) = selected_own_fleet() else {
        return false;
    };
    let hostiles = hostile_fleets();

    let Some(selection) = select_fastest_intercept(&own_fleet, &hostiles) else {
        debug!(
            target: LOG_TARGET,
            "Skipped: no interceptable hostile found, own_fleet={:?}, hostiles={}",
            own_fleet,
            hostiles.len(),
        );
        return false;
    };

    select_hostile_target(&selection, &own_fleet, hostiles.len())
}

#[derive(Clone, Copy, Debug)]
struct InterceptSelection<'a> {
    hostile: &'a Fleet,
    time_seconds: f32,
    intercept_position: Vector3,
}

fn select_hostile_target(selection: &InterceptSelection<'_>, own_fleet: &Fleet, hostile_count: usize) -> bool {
    let manager = NAVIGATION_MANAGER_INSTANCE.load(Relaxed) as *mut Il2CppObject;
    if manager.is_null() {
        debug!(target: LOG_TARGET, "Skipped: NavigationManager instance unavailable for hostile selection");
        return false;
    }

    let can_select = invoke::bool_i64(
        NAVIGATION_MANAGER_CAN_SELECT_POI_METHOD.load(Relaxed),
        manager,
        selection.hostile.id,
        "NavigationManager.CanSelectPoi",
    )
    .unwrap_or(false);
    if !can_select {
        debug!(
            target: LOG_TARGET,
            "Skipped: NavigationManager rejects hostile selection, hostile={:?}, hostiles={hostile_count}",
            selection.hostile,
        );
        return false;
    }

    let selected = invoke::bool_i64(
        NAVIGATION_MANAGER_SELECT_POI_METHOD.load(Relaxed),
        manager,
        selection.hostile.id,
        "NavigationManager.SelectPoi",
    )
    .unwrap_or(false);

    trace!(
        target: LOG_TARGET,
        "Selected hostile for intercept: selected={selected}, own_position_mode={}, own_fleet={own_fleet:?}, hostile={:?}, hostiles={hostile_count}, intercept_time={:.2}s, intercept_position={}",
        position_mode(own_fleet.movement_state),
        selection.hostile,
        selection.time_seconds,
        format_vector3(selection.intercept_position),
    );

    selected
}

pub(crate) fn on_target_viewer_show(event: TargetViewerEvent) {
    let Some(target_fleet_id) = event.fleet_id else {
        trace!(target: LOG_TARGET, "Target viewer shown without readable fleet ID");
        return;
    };

    let Some(system_id) = navigation_view::current_viewed_system_id() else {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: no viewed system, target_fleet_id={target_fleet_id}",
        );
        return;
    };

    let Some(selected_fleet) = fleet_bar::selected_fleet() else {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: selected fleet unavailable, target_fleet_id={target_fleet_id}",
        );
        return;
    };

    let Some(own_fleet_id) = selected_fleet.id else {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: selected fleet ID unavailable, target_fleet_id={target_fleet_id}, {selected_fleet}",
        );
        return;
    };

    if selected_fleet.system_id != Some(system_id) {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: selected fleet is not in viewed system, target_fleet_id={target_fleet_id}, viewed_system={system_id}, {selected_fleet}",
        );
        return;
    }

    let Some(own_fleet) = own_fleet(own_fleet_id) else {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: selected own fleet snapshot unavailable, target_fleet_id={target_fleet_id}, {selected_fleet}",
        );
        return;
    };
    let hostiles = hostile_fleets();

    let Some(target_fleet) = hostiles.iter().find(|fleet| fleet.id == target_fleet_id) else {
        trace!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: target fleet not found as hostile in scanner store, target_fleet_id={target_fleet_id}, own_fleet={:?}, hostiles={}",
            own_fleet,
            hostiles.len(),
        );
        return;
    };

    let Some(selection) = intercept_selection_for_hostile(target_fleet, &own_fleet) else {
        debug!(
            target: LOG_TARGET,
            "Target viewer intercept skipped: target hostile is not interceptable, own_fleet={:?}, hostile={target_fleet:?}",
            own_fleet,
        );
        return;
    };

    trace!(
        target: LOG_TARGET,
        "Target viewer intercept diagnostic: own_position_mode={}, hostile_position_mode={}, own_fleet={:?}, hostile={:?}, hostiles={}, intercept_time={:.2}s, intercept_position={}",
        position_mode(own_fleet.movement_state),
        position_mode(target_fleet.movement_state),
        own_fleet,
        target_fleet,
        hostiles.len(),
        selection.time_seconds,
        format_vector3(selection.intercept_position),
    );
}

fn select_fastest_intercept<'a>(own_fleet: &Fleet, hostiles: &'a [Fleet]) -> Option<InterceptSelection<'a>> {
    let own_position = current_position_for_intercept(own_fleet)?;
    let own_speed = impulse_speed(own_fleet)?;
    let mut best: Option<InterceptSelection<'a>> = None;

    for hostile in hostiles {
        let Some(selection) = intercept_selection_from_state(hostile, own_position, own_speed) else {
            continue;
        };

        if best
            .as_ref()
            .map(|current| selection.time_seconds < current.time_seconds)
            .unwrap_or(true)
        {
            best = Some(selection);
        }
    }

    best
}

fn intercept_selection_for_hostile<'a>(hostile: &'a Fleet, own_fleet: &Fleet) -> Option<InterceptSelection<'a>> {
    intercept_selection_from_state(hostile, current_position_for_intercept(own_fleet)?, impulse_speed(own_fleet)?)
}

fn intercept_selection_from_state(
    hostile: &Fleet,
    own_position: Vector3,
    own_speed: f32,
) -> Option<InterceptSelection<'_>> {
    let target_position = current_position_for_intercept(hostile)?;
    let target_velocity = fleet_velocity(hostile);
    let time_seconds = intercept_time(own_position, own_speed, target_position, target_velocity)?;

    Some(InterceptSelection {
        hostile,
        time_seconds,
        intercept_position: vector_add(target_position, vector_scale(target_velocity, time_seconds)),
    })
}

fn current_position_for_intercept(fleet: &Fleet) -> Option<Vector3> {
    match fleet.movement_state {
        FleetMovementState::Impulsing => current_projected_position(fleet),
        FleetMovementState::Stopped | FleetMovementState::Unknown => fleet.system_position,
        FleetMovementState::Warping => None,
    }
}

fn current_projected_position(fleet: &Fleet) -> Option<Vector3> {
    let position = fleet.system_position?;
    let age = fleet.observed_at.elapsed().as_secs_f32();
    Some(vector_add(position, vector_scale(fleet_velocity(fleet), age)))
}

fn position_mode(movement_state: FleetMovementState) -> &'static str {
    match movement_state {
        FleetMovementState::Impulsing => "projected",
        FleetMovementState::Stopped | FleetMovementState::Unknown => "fixed",
        FleetMovementState::Warping => "unavailable",
    }
}

fn fleet_velocity(fleet: &Fleet) -> Vector3 {
    if fleet.movement_state != FleetMovementState::Impulsing {
        return Vector3::default();
    }

    let Some(speed) = impulse_speed(fleet) else {
        return Vector3::default();
    };
    let Some(direction) = fleet.travel_direction else {
        return Vector3::default();
    };
    let direction_length = vector_length(direction);
    if direction_length <= INTERCEPT_EPSILON {
        return Vector3::default();
    }

    vector_scale(direction, speed / direction_length)
}

fn impulse_speed(fleet: &Fleet) -> Option<f32> {
    fleet
        .max_impulse_speed
        .filter(|speed| speed.is_finite() && *speed > INTERCEPT_EPSILON)
}

fn intercept_time(
    own_position: Vector3,
    own_speed: f32,
    target_position: Vector3,
    target_velocity: Vector3,
) -> Option<f32> {
    if !own_speed.is_finite() || own_speed <= INTERCEPT_EPSILON {
        return None;
    }

    let relative_position = vector_sub(target_position, own_position);
    let a = vector_dot(target_velocity, target_velocity) - own_speed * own_speed;
    let b = 2.0 * vector_dot(relative_position, target_velocity);
    let c = vector_dot(relative_position, relative_position);

    if c <= INTERCEPT_EPSILON {
        return Some(0.0);
    }

    if a.abs() <= INTERCEPT_EPSILON {
        if b.abs() <= INTERCEPT_EPSILON {
            return None;
        }
        return positive_time(-c / b);
    }

    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }

    let sqrt_discriminant = discriminant.sqrt();
    let t1 = (-b - sqrt_discriminant) / (2.0 * a);
    let t2 = (-b + sqrt_discriminant) / (2.0 * a);

    [t1, t2]
        .into_iter()
        .filter_map(positive_time)
        .min_by(|left, right| left.total_cmp(right))
}

fn positive_time(time_seconds: f32) -> Option<f32> {
    if time_seconds.is_finite() && time_seconds >= 0.0 {
        Some(time_seconds)
    } else {
        None
    }
}

fn vector_add(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 {
        x: left.x + right.x,
        y: left.y + right.y,
        z: left.z + right.z,
    }
}

fn vector_sub(left: Vector3, right: Vector3) -> Vector3 {
    Vector3 {
        x: left.x - right.x,
        y: left.y - right.y,
        z: left.z - right.z,
    }
}

fn vector_scale(vector: Vector3, scale: f32) -> Vector3 {
    Vector3 {
        x: vector.x * scale,
        y: vector.y * scale,
        z: vector.z * scale,
    }
}

fn vector_dot(left: Vector3, right: Vector3) -> f32 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn vector_length(vector: Vector3) -> f32 {
    vector_dot(vector, vector).sqrt()
}

fn format_vector3(vector: Vector3) -> String {
    format!("({:.2}, {:.2}, {:.2})", vector.x, vector.y, vector.z)
}

/// Convert an enter-system event into owned fleets and route it through the view gate.
fn process_enter_system(address: *mut Il2CppObject, fleets: *mut Il2CppList<*mut Il2CppObject>) {
    let Some(system_id) = node_address_system(address) else {
        trace!(target: LOG_TARGET, "Fleet enter event ignored without valid system");
        return;
    };

    let fleets = unsafe { list_objects(fleets) }
        .into_iter()
        .filter_map(|fleet| inspect_fleet_in_system(fleet, Some(system_id)))
        .collect::<Vec<_>>();

    route_fleet_event(PendingFleetEvent::EnterSystem { system_id, fleets });
}

/// Invalidate own fleets and remove exited fleets from the viewed-system store.
fn process_exit_system(address: *mut Il2CppObject, fleets: *mut Il2CppList<*mut Il2CppObject>) {
    let fleet_refs = unsafe { list_objects(fleets) }
        .into_iter()
        .filter_map(inspect_fleet_ref)
        .collect::<Vec<_>>();
    let changes = invalidate_own_fleet_refs(&fleet_refs);
    if !changes.is_empty() {
        log_fleet_changes(
            &format!("Own fleet store update: reason=exit_system, invalidated={}", changes.len()),
            &changes,
        );
    }

    let Some(system_id) = node_address_system(address) else {
        trace!(target: LOG_TARGET, "Fleet exit event ignored without valid system");
        return;
    };

    if navigation_view::current_viewed_system_id() == Some(system_id) {
        process_remove_refs("fleet_exit_system", system_id, fleet_refs);
    } else {
        trace!(
            target: LOG_TARGET,
            "Fleet exit event ignored: reason=system_mismatch, event_system={}, viewed_system={}",
            format_optional_i64(Some(system_id)),
            format_optional_i64(navigation_view::current_viewed_system_id()),
        );
    }
}

/// Apply an already-owned enter batch to the store and log the resulting changes.
fn process_enter_fleets(system_id: i64, fleets: Vec<Fleet>) -> bool {
    match store_enter_fleets(system_id, fleets) {
        StoreEnterResult::Replaced { stored, changes } => {
            log_fleet_changes(
                &format!("Fleet store replace: reason=enter_system, system={system_id}, stored={stored}"),
                &changes,
            );
        }
        StoreEnterResult::Upserted {
            added,
            updated,
            unchanged,
            total,
            changes,
        } => {
            let summary = format!(
                "Fleet store upsert: reason=enter_system, system={system_id}, added={added}, updated={updated}, unchanged={unchanged}, total={total}"
            );
            if changes.is_empty() {
                trace_fleet_changes(&summary, &changes);
            } else {
                log_fleet_changes(&summary, &changes);
            }
        }
    }

    true
}

/// Convert an update event into owned fleets and route it through the view gate.
fn process_fleets_updated(reason: &'static str, fleets: *mut Il2CppList<*mut Il2CppObject>) {
    let fleets = unsafe { list_objects(fleets) }
        .into_iter()
        .filter_map(inspect_fleet)
        .collect::<Vec<_>>();

    route_fleet_event(PendingFleetEvent::Update { reason, fleets });
}

fn process_single_fleet_updated(reason: &'static str, fleet: *mut Il2CppObject) {
    let Some(fleet) = inspect_fleet(fleet) else {
        trace!(target: LOG_TARGET, "Fleet update ignored: reason={reason}, fleet=unreadable");
        return;
    };

    route_fleet_event(PendingFleetEvent::Update { reason, fleets: vec![fleet] });
}

fn process_single_fleet_disposed(fleet: *mut Il2CppObject) {
    let Some(fleet) = inspect_fleet_ref(fleet) else {
        trace!(target: LOG_TARGET, "Fleet dispose ignored: fleet=unreadable");
        return;
    };

    route_fleet_event(PendingFleetEvent::Dispose { fleets: vec![fleet] });
}

/// Convert a single player-fleet update into an owned snapshot.
fn process_player_fleet_updated(fleet: *mut Il2CppObject) {
    if fleet.is_null() {
        trace!(target: LOG_TARGET, "Player fleet update ignored: fleet=null");
        return;
    }

    let Some(fleet) = inspect_fleet(fleet) else {
        trace!(target: LOG_TARGET, "Player fleet update ignored: unreadable fleet");
        return;
    };

    let changes = store_own_fleets(&[fleet]);
    if !changes.is_empty() {
        log_fleet_changes(
            &format!("Own fleet store update: reason=player_fleet_updated, changed={}", changes.len()),
            &changes,
        );
    }
}

/// Read ended courses for diagnostics only.
fn process_course_end(courses: *mut Il2CppList<*mut Il2CppObject>) {
    for course in unsafe { list_objects(courses) } {
        let snapshot = inspect_course_end(course);
        let Some(fleet_id) = snapshot.fleet_id else {
            warn!(target: LOG_TARGET, "Course end ignored: course without readable fleet ID");
            continue;
        };

        // FleetDeployedData.CurrentState is the authoritative movement source. This hook observes
        // course-end events for trace diagnostics and does not feed the store.
        trace!(
            target: LOG_TARGET,
            "Course end observed: fleet_id={fleet_id}, position={}",
            format_optional_vector3(snapshot.position),
        );
    }
}

/// Apply an owned update batch to the store and log the resulting changes.
fn process_update_fleets(reason: &str, system_id: i64, fleets: Vec<Fleet>) -> bool {
    let StoreUpdateResult {
        inserted,
        updated,
        unchanged,
        ignored,
        total,
        changes,
        ignored_fleet_ids,
    } = store_update_fleets(system_id, fleets);
    let summary = format!(
        "Fleet store update: reason={reason}, system={system_id}, inserted={inserted}, updated={updated}, unchanged={unchanged}, ignored={ignored}, total={total}"
    );

    if ignored > 0 {
        trace!(
            target: LOG_TARGET,
            "Fleet update ignored: reason={reason}, system={system_id}, ids={}",
            format_fleet_ids(&ignored_fleet_ids),
        );
    }

    if reason == "fleet_update"
        || reason == "fleet_added"
        || reason == "fleet_state_change"
        || changes.is_empty()
        || is_movement_only_update(&changes)
    {
        trace_fleet_changes(&summary, &changes);
    } else {
        log_fleet_changes(&summary, &changes);
    }

    inserted > 0 || updated > 0 || unchanged > 0
}

/// Convert a disposed event into owned fleet references and route it through the view gate.
fn process_fleets_disposed(fleets: *mut Il2CppList<*mut Il2CppObject>) {
    let fleet_refs = unsafe { list_objects(fleets) }
        .into_iter()
        .filter_map(inspect_fleet_ref)
        .collect::<Vec<_>>();

    route_fleet_event(PendingFleetEvent::Dispose { fleets: fleet_refs });
}

/// Remove disposed fleet references from the viewed store.
fn process_dispose_refs(system_id: i64, fleet_refs: Vec<FleetRef>) -> bool {
    process_remove_refs("fleet_disposed", system_id, fleet_refs)
}

/// Remove fleet references from the viewed store.
fn process_remove_refs(reason: &str, system_id: i64, fleet_refs: Vec<FleetRef>) -> bool {
    let StoreRemoveResult {
        vanished,
        ignored,
        total,
        changes,
        ignored_fleet_ids,
    } = store_remove_fleets(system_id, fleet_refs);
    let summary = format!(
        "Fleet store remove: reason={reason}, system={system_id}, vanished={vanished}, ignored={ignored}, total={total}"
    );

    if ignored > 0 {
        trace!(
            target: LOG_TARGET,
            "Fleet remove ignored: reason={reason}, system={system_id}, ids={}",
            format_fleet_ids(&ignored_fleet_ids),
        );
    }

    trace_fleet_changes(&summary, &changes);

    vanished > 0
}

/// Update the shared viewed system and flush pending fleet events for it.
fn set_viewed_system(system_id: Option<i64>) {
    let Some(system_id) = system_id else {
        return;
    };

    if !navigation_view::set_viewed_system(Some(system_id)) {
        return;
    }

    debug!(target: LOG_TARGET, "Viewed system changed: system={system_id}");

    flush_pending_fleet_events(system_id);
}

/// Clear only the shared viewed-system marker.
///
/// Non-system views, such as the galaxy view, must stop actions from using the previous system context. The fleet
/// snapshot store is intentionally kept until a concrete system view replaces it with another system's data.
fn clear_viewed_system(reason: &str) {
    if !navigation_view::clear_viewed_system() {
        return;
    }

    debug!(target: LOG_TARGET, "Viewed system marker cleared: reason={reason}");
}

/// Send an event directly to the store, or queue it until a viewed system is known.
fn route_fleet_event(event: PendingFleetEvent) -> bool {
    process_global_own_fleet_event(&event);

    let Some(event) = viewed_store_event(event) else {
        return false;
    };

    match navigation_view::current_viewed_system_id() {
        Some(system_id) => process_pending_fleet_event(system_id, event),
        None => {
            queue_pending_fleet_event(event);
            false
        }
    }
}

fn viewed_store_event(event: PendingFleetEvent) -> Option<PendingFleetEvent> {
    match event {
        PendingFleetEvent::EnterSystem { system_id, fleets } => {
            let fleets = fleets
                .into_iter()
                .filter(|fleet| fleet.kind != FleetKind::Own)
                .collect::<Vec<_>>();
            (!fleets.is_empty()).then_some(PendingFleetEvent::EnterSystem { system_id, fleets })
        }
        PendingFleetEvent::Update { reason, fleets } => {
            let fleets = fleets
                .into_iter()
                .filter(|fleet| fleet.kind != FleetKind::Own)
                .collect::<Vec<_>>();
            (!fleets.is_empty()).then_some(PendingFleetEvent::Update { reason, fleets })
        }
        PendingFleetEvent::Dispose { fleets } => Some(PendingFleetEvent::Dispose { fleets }),
    }
}

/// Keep own-fleet snapshots globally, independent of the currently viewed system.
fn process_global_own_fleet_event(event: &PendingFleetEvent) {
    match event {
        PendingFleetEvent::EnterSystem { system_id, fleets } => {
            let changes = store_own_fleets(fleets);
            if !changes.is_empty() {
                log_fleet_changes(
                    &format!(
                        "Own fleet store update: reason=enter_system, system={system_id}, changed={}",
                        changes.len()
                    ),
                    &changes,
                );
            }
        }
        PendingFleetEvent::Update { reason, fleets } => {
            let changes = store_own_fleets(fleets);
            if !changes.is_empty() {
                trace_fleet_changes(
                    &format!("Own fleet store update: reason={reason}, changed={}", changes.len()),
                    &changes,
                );
            }
        }
        PendingFleetEvent::Dispose { fleets } => {
            let changes = invalidate_own_fleet_refs(fleets);
            if !changes.is_empty() {
                log_fleet_changes(
                    &format!("Own fleet store update: reason=fleet_disposed, invalidated={}", changes.len()),
                    &changes,
                );
            }
        }
    }
}

/// Drain pending events outside the queue lock and process them for the viewed system.
fn flush_pending_fleet_events(system_id: i64) {
    let events = drain_pending_fleet_events();

    if events.is_empty() {
        return;
    }

    let mut processed = 0;
    let mut dropped = 0;
    for event in events {
        if process_pending_fleet_event(system_id, event) {
            processed += 1;
        } else {
            dropped += 1;
        }
    }

    trace!(
        target: LOG_TARGET,
        "Pending fleet events flushed: system={system_id}, processed={processed}, dropped={dropped}",
    );
}

/// Process one queued or live event against the current viewed-system gate.
fn process_pending_fleet_event(system_id: i64, event: PendingFleetEvent) -> bool {
    match event {
        PendingFleetEvent::EnterSystem { system_id: event_system_id, fleets } => {
            if event_system_id == system_id {
                process_enter_fleets(system_id, fleets)
            } else {
                trace!(
                    target: LOG_TARGET,
                    "Fleet event ignored: reason=system_mismatch, event=enter_system, system={}, viewed_system={system_id}",
                    format_optional_i64(Some(event_system_id)),
                );
                false
            }
        }
        PendingFleetEvent::Update { reason, fleets } => {
            let result = process_update_fleets(reason, system_id, fleets);
            if !result {
                trace!(
                    target: LOG_TARGET,
                    "Fleet event ignored: reason=no_matching_fleets, event={reason}, viewed_system={system_id}",
                );
            }
            result
        }
        PendingFleetEvent::Dispose { fleets } => {
            let result = process_dispose_refs(system_id, fleets);
            if !result {
                trace!(
                    target: LOG_TARGET,
                    "Fleet event ignored: reason=no_matching_fleets, event=fleet_disposed, viewed_system={system_id}",
                );
            }
            result
        }
    }
}

/// Read a fleet with its own address-derived system, when available.
fn inspect_fleet(fleet: *mut Il2CppObject) -> Option<Fleet> {
    inspect_fleet_in_system(fleet, fleet_address_system(fleet))
}

/// Copy all stable fleet data into an owned snapshot.
fn inspect_fleet_in_system(fleet: *mut Il2CppObject, system_id: Option<i64>) -> Option<Fleet> {
    let id = fleet_id(fleet)?;
    let observed_at = std::time::Instant::now();
    let local_player = is_local_player(fleet);
    let fleet_type = fleet_type(fleet);
    let hull_type = fleet_hull_type(fleet);
    let hull_name = fleet_hull_name(fleet);

    Some(Fleet {
        id,
        observed_at,
        system_id,
        kind: classify_fleet(local_player, fleet_type, hull_type),
        combat_class: classify_combat_class(hull_name.as_deref(), hull_type),
        fleet_type,
        hull_type,
        hull_name,
        local_player,
        system_position: fleet_system_position(fleet),
        strength: fleet_strength(fleet),
        level: fleet_level(fleet),
        mining: fleet_is_mining(fleet),
        max_impulse_speed: fleet_max_impulse_speed(fleet),
        max_warp_speed: fleet_max_warp_speed(fleet),
        travel_direction: fleet_travel_direction(fleet),
        time_since_last_update: fleet_time_since_last_update(fleet),
        movement_state: fleet_movement_state(fleet),
    })
}

/// Read only the fields needed to remove a fleet from the store.
fn inspect_fleet_ref(fleet: *mut Il2CppObject) -> Option<FleetRef> {
    Some(FleetRef {
        id: fleet_id(fleet)?,
        system_id: fleet_address_system(fleet),
    })
}

#[derive(Debug)]
struct CourseEndSnapshot {
    fleet_id: Option<i64>,
    position: Option<Vector3>,
}

/// Copy course-end details for trace diagnostics.
fn inspect_course_end(course: *mut Il2CppObject) -> CourseEndSnapshot {
    CourseEndSnapshot {
        fleet_id: course_fleet_id(course),
        position: course_system_position(course),
    }
}

// ---- IL2CPP access ---------------------------------------------------------

/// Copy non-null object pointers from an IL2CPP `List<T>`.
unsafe fn list_objects(list: *mut Il2CppList<*mut Il2CppObject>) -> Vec<*mut Il2CppObject> {
    if list.is_null() {
        return Vec::new();
    }

    let list = unsafe { &*list };
    let mut objects = Vec::with_capacity(list.len());
    for index in 0..list.len() {
        if let Some(object) = unsafe { list.get(index) }
            && !object.is_null()
        {
            objects.push(object);
        }
    }
    objects
}

/// Read `FleetDeployedData.ID`.
fn fleet_id(fleet: *mut Il2CppObject) -> Option<i64> {
    invoke::i64(GET_FLEET_ID_FN.load(Relaxed), fleet, "FleetDeployedData.get_ID")
}

/// Read `FleetDeployedData.SystemPosition`.
fn fleet_system_position(fleet: *mut Il2CppObject) -> Option<Vector3> {
    invoke::value(
        GET_FLEET_SYSTEM_POSITION_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_SystemPosition",
    )
}

/// Read `FleetDeployedData.Address`.
fn fleet_address(fleet: *mut Il2CppObject) -> Option<*mut Il2CppObject> {
    invoke::object(GET_FLEET_ADDRESS_FN.load(Relaxed), fleet, "FleetDeployedData.get_Address")
}

/// Read a fleet's address system ID.
fn fleet_address_system(fleet: *mut Il2CppObject) -> Option<i64> {
    node_address_system(fleet_address(fleet)?)
}

/// Read whether this fleet belongs to the local player.
fn is_local_player(fleet: *mut Il2CppObject) -> bool {
    invoke::bool(
        GET_FLEET_IS_LOCAL_PLAYER_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_IsLocalPlayer",
    )
    .unwrap_or(false)
}

/// Read the raw deployed fleet type.
fn fleet_type(fleet: *mut Il2CppObject) -> Option<i32> {
    invoke::i32(GET_FLEET_TYPE_FN.load(Relaxed), fleet, "FleetDeployedData.get_FleetType")
}

/// Read fleet strength.
fn fleet_strength(fleet: *mut Il2CppObject) -> Option<i32> {
    invoke::i32(GET_FLEET_STRENGTH_FN.load(Relaxed), fleet, "FleetDeployedData.get_Strength")
}

/// Read fleet level.
fn fleet_level(fleet: *mut Il2CppObject) -> Option<i32> {
    invoke::i32(GET_FLEET_LEVEL_FN.load(Relaxed), fleet, "FleetDeployedData.get_Level")
}

/// Read whether the fleet is mining.
fn fleet_is_mining(fleet: *mut Il2CppObject) -> Option<bool> {
    invoke::bool(GET_FLEET_IS_MINING_FN.load(Relaxed), fleet, "FleetDeployedData.get_IsMining")
}

/// Read maximum impulse speed.
fn fleet_max_impulse_speed(fleet: *mut Il2CppObject) -> Option<f32> {
    invoke::f32(
        GET_FLEET_MAX_IMPULSE_SPEED_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_MaxImpulseSpeed",
    )
}

/// Read maximum warp speed.
fn fleet_max_warp_speed(fleet: *mut Il2CppObject) -> Option<f32> {
    invoke::f32(
        GET_FLEET_MAX_WARP_SPEED_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_MaxWarpSpeed",
    )
}

/// Read current travel direction.
fn fleet_travel_direction(fleet: *mut Il2CppObject) -> Option<Vector3> {
    invoke::value(
        GET_FLEET_TRAVEL_DIRECTION_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_TravelDirection",
    )
}

/// Read age of the current fleet data.
fn fleet_time_since_last_update(fleet: *mut Il2CppObject) -> Option<f32> {
    invoke::f32(
        GET_FLEET_TIME_SINCE_LAST_UPDATE_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_TimeSinceLastUpdate",
    )
}

/// Read and map the fleet's deployed movement state.
fn fleet_movement_state(fleet: *mut Il2CppObject) -> FleetMovementState {
    classify_movement_state(invoke::i32(
        GET_FLEET_CURRENT_STATE_FN.load(Relaxed),
        fleet,
        "FleetDeployedData.get_CurrentState",
    ))
}

fn classify_movement_state(raw_state: Option<i32>) -> FleetMovementState {
    let Some(raw_state) = raw_state else {
        return FleetMovementState::Unknown;
    };

    match raw_state {
        DEPLOYED_STATE_IDLE | DEPLOYED_STATE_BATTLING | DEPLOYED_STATE_DOCKED | DEPLOYED_STATE_PRE_BATTLE => {
            FleetMovementState::Stopped
        }
        DEPLOYED_STATE_MOVING => FleetMovementState::Impulsing,
        // Recall moves the fleet home, but the enum does not say whether by impulse or warp,
        // so treat it like a warp: position is unreliable and the fleet is left out of intercepts.
        DEPLOYED_STATE_WARPING | DEPLOYED_STATE_RECALL => FleetMovementState::Warping,
        _ => FleetMovementState::Unknown,
    }
}

/// Read the fleet ID associated with a course.
fn course_fleet_id(course: *mut Il2CppObject) -> Option<i64> {
    invoke::i64(GET_COURSE_FLEET_ID_FN.load(Relaxed), course, "CourseData.get_FleetID")
}

/// Read the current course system position.
fn course_system_position(course: *mut Il2CppObject) -> Option<Vector3> {
    invoke::value(
        GET_COURSE_SYSTEM_POSITION_FN.load(Relaxed),
        course,
        "CourseData.get_SystemPosition",
    )
}

/// Read the fleet hull object.
fn fleet_hull(fleet: *mut Il2CppObject) -> Option<*mut Il2CppObject> {
    invoke::object(GET_FLEET_HULL_FN.load(Relaxed), fleet, "FleetDeployedData.get_Hull")
}

/// Read the raw hull type.
fn fleet_hull_type(fleet: *mut Il2CppObject) -> Option<i32> {
    let hull = fleet_hull(fleet)?;
    invoke::i32(GET_HULL_TYPE_FN.load(Relaxed), hull, "HullSpec.get_Type")
}

/// Read the hull name.
fn fleet_hull_name(fleet: *mut Il2CppObject) -> Option<String> {
    let hull = fleet_hull(fleet)?;
    invoke::string(GET_HULL_NAME_FN.load(Relaxed), hull, "HullSpec.get_Name")
}

/// Read a valid system ID from a `NodeAddress`.
fn node_address_system(address: *mut Il2CppObject) -> Option<i64> {
    valid_system_id(invoke::i64(
        GET_NODE_ADDRESS_SYSTEM_FN.load(Relaxed),
        address,
        "NodeAddress.get_System",
    ))
}

/// Reject negative sentinel system IDs.
fn valid_system_id(system_id: Option<i64>) -> Option<i64> {
    system_id.filter(|system_id| *system_id >= 0)
}

fn format_optional_vector3(value: Option<Vector3>) -> String {
    value.map(format_vector3).unwrap_or_else(|| "unknown".to_string())
}

// ---- Installation ----------------------------------------------------------

/// Install accessors and event hooks for fleet scanning.
pub fn install(api: &Il2CppApi) {
    if !compatibility::is_enabled(manifest::FLEET_SCANNER) {
        return;
    }
    fleet_bar::install(api);
    target_viewer::install(api);
    target_viewer::subscribe_target_id(on_target_viewer_show);
    if is_ready() {
        return;
    }

    install_model_accessors(api);
    install_node_address_accessors(api);
    install_change_view_data_accessors(api);
    install_navigation_selection(api);
    install_fleet_data_system_hooks(api);
    install_fleet_event_container_hooks(api);
    install_course_event_hook(api);
    trace!(target: LOG_TARGET, "Fleet scanner install finished");
}

fn is_ready() -> bool {
    fleet_model_accessors_ready()
        && node_address_accessors_ready()
        && course_accessors_ready()
        && !field_missing(&OFFSET_CHANGE_VIEW_DATA_ADDRESS)
        && navigation_selection_ready()
        && fleet_data_system_hooks_ready()
        && fleet_event_container_hooks_ready()
        && deployment_events_ready()
}

fn fleet_model_accessors_ready() -> bool {
    all_pointers_ready(fleet_model_accessor_slots())
}

fn fleet_model_accessor_slots() -> [&'static AtomicPtr<MethodInfo>; 16] {
    [
        &GET_FLEET_ID_FN,
        &GET_FLEET_SYSTEM_POSITION_FN,
        &GET_FLEET_ADDRESS_FN,
        &GET_FLEET_IS_LOCAL_PLAYER_FN,
        &GET_FLEET_TYPE_FN,
        &GET_FLEET_STRENGTH_FN,
        &GET_FLEET_LEVEL_FN,
        &GET_FLEET_IS_MINING_FN,
        &GET_FLEET_HULL_FN,
        &GET_FLEET_MAX_IMPULSE_SPEED_FN,
        &GET_FLEET_MAX_WARP_SPEED_FN,
        &GET_FLEET_TRAVEL_DIRECTION_FN,
        &GET_FLEET_TIME_SINCE_LAST_UPDATE_FN,
        &GET_FLEET_CURRENT_STATE_FN,
        &GET_HULL_TYPE_FN,
        &GET_HULL_NAME_FN,
    ]
}

fn node_address_accessors_ready() -> bool {
    all_pointers_ready([&GET_NODE_ADDRESS_SYSTEM_FN])
}

fn course_accessors_ready() -> bool {
    all_pointers_ready([&GET_COURSE_FLEET_ID_FN, &GET_COURSE_SYSTEM_POSITION_FN])
}

fn navigation_selection_ready() -> bool {
    all_pointers_ready([&NAVIGATION_MANAGER_CAN_SELECT_POI_METHOD, &NAVIGATION_MANAGER_SELECT_POI_METHOD])
        && all_pointers_ready([&ORIG_CHANGE_VIEW, &ORIG_NAVIGATION_MANAGER_ENABLE, &ORIG_NAVIGATION_MANAGER_DISABLE])
}

fn fleet_data_system_hooks_ready() -> bool {
    all_pointers_ready([
        &ORIG_FLEET_DATA_ADDED,
        &ORIG_FLEET_DATA_UPDATED,
        &ORIG_FLEET_DATA_DISPOSED,
        &ORIG_FLEET_DATA_ENTER_SYSTEM,
        &ORIG_FLEET_DATA_EXIT_SYSTEM,
    ])
}

fn fleet_event_container_hooks_ready() -> bool {
    all_pointers_ready(fleet_event_container_hook_slots())
}

fn fleet_event_container_hook_slots() -> [&'static AtomicPtr<()>; 5] {
    [
        &ORIG_EVENT_FLEET_ADDED,
        &ORIG_EVENT_FLEET_DISPOSED,
        &ORIG_EVENT_FLEET_UPDATED,
        &ORIG_PLAYER_FLEET_UPDATED,
        &ORIG_EVENT_FLEET_STATE_CHANGED,
    ]
}

fn deployment_events_ready() -> bool {
    all_pointers_ready([&ORIG_COURSE_END])
}

fn all_pointers_ready<T, const N: usize>(pointers: [&AtomicPtr<T>; N]) -> bool {
    pointers.into_iter().all(|pointer| !pointer.load(Relaxed).is_null())
}

/// Resolve all FleetDeployedData and HullSpec getters used for snapshots.
fn install_model_accessors(api: &Il2CppApi) {
    if let Some(fleet_class) = resolver::resolve_prime_model_class(api, "FleetDeployedData") {
        resolve_fn_if_missing(api, fleet_class, "get_ID", 0, &GET_FLEET_ID_FN);
        resolve_fn_if_missing(api, fleet_class, "get_SystemPosition", 0, &GET_FLEET_SYSTEM_POSITION_FN);
        resolve_fn_if_missing(api, fleet_class, "get_Address", 0, &GET_FLEET_ADDRESS_FN);
        resolve_fn_if_missing(api, fleet_class, "get_IsLocalPlayer", 0, &GET_FLEET_IS_LOCAL_PLAYER_FN);
        resolve_fn_if_missing(api, fleet_class, "get_FleetType", 0, &GET_FLEET_TYPE_FN);
        resolve_fn_if_missing(api, fleet_class, "get_Strength", 0, &GET_FLEET_STRENGTH_FN);
        resolve_fn_if_missing(api, fleet_class, "get_Level", 0, &GET_FLEET_LEVEL_FN);
        resolve_fn_if_missing(api, fleet_class, "get_IsMining", 0, &GET_FLEET_IS_MINING_FN);
        resolve_fn_if_missing(api, fleet_class, "get_Hull", 0, &GET_FLEET_HULL_FN);
        resolve_fn_if_missing(api, fleet_class, "get_MaxImpulseSpeed", 0, &GET_FLEET_MAX_IMPULSE_SPEED_FN);
        resolve_fn_if_missing(api, fleet_class, "get_MaxWarpSpeed", 0, &GET_FLEET_MAX_WARP_SPEED_FN);
        resolve_fn_if_missing(api, fleet_class, "get_TravelDirection", 0, &GET_FLEET_TRAVEL_DIRECTION_FN);
        resolve_fn_if_missing(
            api,
            fleet_class,
            "get_TimeSinceLastUpdate",
            0,
            &GET_FLEET_TIME_SINCE_LAST_UPDATE_FN,
        );
        resolve_fn_if_missing(api, fleet_class, "get_CurrentState", 0, &GET_FLEET_CURRENT_STATE_FN);
    } else {
        warn!(target: LOG_TARGET, "FleetDeployedData class not found");
    }

    if let Some(hull_class) = resolver::resolve_prime_model_class(api, "HullSpec") {
        resolve_fn_if_missing(api, hull_class, "get_Type", 0, &GET_HULL_TYPE_FN);
        resolve_fn_if_missing(api, hull_class, "get_Name", 0, &GET_HULL_NAME_FN);
    } else {
        warn!(target: LOG_TARGET, "HullSpec class not found");
    }

    if let Some(course_class) = resolver::resolve_prime_model_class(api, "CourseData") {
        resolve_fn_if_missing(api, course_class, "get_FleetID", 0, &GET_COURSE_FLEET_ID_FN);
        resolve_fn_if_missing(api, course_class, "get_SystemPosition", 0, &GET_COURSE_SYSTEM_POSITION_FN);
    } else {
        warn!(target: LOG_TARGET, "CourseData class not found");
    }
}

/// Resolve NodeAddress access needed for system IDs.
fn install_node_address_accessors(api: &Il2CppApi) {
    if let Some(address_class) = resolver::resolve_prime_model_class(api, "NodeAddress") {
        resolve_fn_if_missing(api, address_class, "get_System", 0, &GET_NODE_ADDRESS_SYSTEM_FN);
    } else {
        warn!(target: LOG_TARGET, "NodeAddress class not found");
    }
}

/// Resolve ChangeViewData fields used by the navigation view hook.
fn install_change_view_data_accessors(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Navigation", "ChangeViewData")
    else {
        warn!(target: LOG_TARGET, "ChangeViewData class not found");
        return;
    };

    resolve_field_if_missing(api, class, "address", &OFFSET_CHANGE_VIEW_DATA_ADDRESS);
}

/// Resolve and track NavigationManager access for POI selection.
fn install_navigation_selection(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Navigation", "NavigationManager")
    else {
        warn!(target: LOG_TARGET, "NavigationManager class not found");
        return;
    };

    resolve_fn_if_missing(api, class, "CanSelectPoi", 1, &NAVIGATION_MANAGER_CAN_SELECT_POI_METHOD);
    resolve_fn_if_missing(api, class, "SelectPoi", 1, &NAVIGATION_MANAGER_SELECT_POI_METHOD);
    install_hook(api, class, "ChangeView", 2, hook_change_view as *const (), &ORIG_CHANGE_VIEW);
    install_hook(
        api,
        class,
        "OnEnable",
        0,
        hook_navigation_manager_enable as *const (),
        &ORIG_NAVIGATION_MANAGER_ENABLE,
    );
    install_hook(
        api,
        class,
        "OnDisable",
        0,
        hook_navigation_manager_disable as *const (),
        &ORIG_NAVIGATION_MANAGER_DISABLE,
    );
}

/// Hook FleetDataSystem instance handlers, which are large enough to survive MSVC inlining of event wrappers.
fn install_fleet_data_system_hooks(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.Core.Systems", "FleetDataSystem")
    else {
        warn!(target: LOG_TARGET, "FleetDataSystem class not found");
        return;
    };

    install_hook(
        api,
        class,
        "OnFleetsAddedEvent",
        1,
        hook_fleet_data_added as *const (),
        &ORIG_FLEET_DATA_ADDED,
    );
    install_hook(
        api,
        class,
        "OnFleetsUpdatedEvent",
        1,
        hook_fleet_data_updated as *const (),
        &ORIG_FLEET_DATA_UPDATED,
    );
    install_hook(
        api,
        class,
        "OnFleetsDisposedEvent",
        1,
        hook_fleet_data_disposed as *const (),
        &ORIG_FLEET_DATA_DISPOSED,
    );
    install_hook(
        api,
        class,
        "OnFleetsEnterSystemEvent",
        2,
        hook_fleet_data_enter_system as *const (),
        &ORIG_FLEET_DATA_ENTER_SYSTEM,
    );
    install_hook(
        api,
        class,
        "OnFleetsExitSystemEvent",
        2,
        hook_fleet_data_exit_system as *const (),
        &ORIG_FLEET_DATA_EXIT_SYSTEM,
    );
}

/// Hook a non-inline event-container method that carries direct player-fleet snapshots.
fn install_fleet_event_container_hooks(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_prime_class(api, "Digit.PrimeServer.Services", "FleetEventContainer") else {
        warn!(target: LOG_TARGET, "FleetEventContainer class not found");
        return;
    };

    install_hook(
        api,
        class,
        "AddFleetAdded",
        1,
        hook_event_fleet_added as *const (),
        &ORIG_EVENT_FLEET_ADDED,
    );
    install_hook(
        api,
        class,
        "AddFleetDisposed",
        1,
        hook_event_fleet_disposed as *const (),
        &ORIG_EVENT_FLEET_DISPOSED,
    );
    install_hook(
        api,
        class,
        "AddFleetUpdated",
        1,
        hook_event_fleet_updated as *const (),
        &ORIG_EVENT_FLEET_UPDATED,
    );
    install_hook(
        api,
        class,
        "AddPlayerFleetUpdated",
        1,
        hook_player_fleet_updated as *const (),
        &ORIG_PLAYER_FLEET_UPDATED,
    );
    install_hook(
        api,
        class,
        "AddFleetStateChanged",
        1,
        hook_event_fleet_state_changed as *const (),
        &ORIG_EVENT_FLEET_STATE_CHANGED,
    );
}

/// Hook course events for diagnostics.
fn install_course_event_hook(api: &Il2CppApi) {
    let Some(events_class) = resolver::resolve_prime_class(api, "Digit.PrimeServer.Events", "DeploymentEvents") else {
        warn!(target: LOG_TARGET, "DeploymentEvents class not found");
        return;
    };

    install_hook(
        api,
        events_class,
        "TriggerCourseEndEvent",
        1,
        hook_course_end as *const (),
        &ORIG_COURSE_END,
    );
}

/// Install one resolved hook and store its original trampoline.
fn install_hook(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    hook: *const (),
    original: &AtomicPtr<()>,
) {
    tracker::install_resolved_hook_if_missing(
        api,
        class,
        method_name,
        param_count,
        &format!("FleetScanner.{method_name}"),
        hook,
        original,
    );
}

/// Resolve one method into an atomic MethodInfo pointer.
fn resolve_fn_if_missing(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    target: &AtomicPtr<MethodInfo>,
) {
    if !method_missing(target) {
        return;
    }

    resolver::resolve_method_into(api, class, method_name, param_count, target);
}

fn method_missing(target: &AtomicPtr<MethodInfo>) -> bool {
    target.load(Relaxed).is_null()
}

fn field_missing(target: &AtomicUsize) -> bool {
    target.load(Relaxed) == 0
}

fn resolve_field_if_missing(api: &Il2CppApi, class: *mut Il2CppClass, field_name: &str, target: &AtomicUsize) {
    if !field_missing(target) {
        return;
    }

    resolver::resolve_field_offset_into(api, class, field_name, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ptr::NonNull;
    use std::sync::atomic::AtomicPtr;
    use std::time::{Duration, Instant};

    use crate::hooks::navigation_view::TEST_LOCK;

    struct PointerStateGuard<'a, T> {
        previous: Vec<(&'a AtomicPtr<T>, *mut T)>,
    }

    impl<'a, T> PointerStateGuard<'a, T> {
        fn set<const N: usize>(pointers: [&'a AtomicPtr<T>; N], value: *mut T) -> Self {
            let previous = pointers
                .into_iter()
                .map(|pointer| (pointer, pointer.swap(value, Relaxed)))
                .collect();
            Self { previous }
        }
    }

    impl<T> Drop for PointerStateGuard<'_, T> {
        fn drop(&mut self) {
            for (pointer, previous) in &self.previous {
                pointer.store(*previous, Relaxed);
            }
        }
    }

    fn reset_store() {
        *FLEET_STORE.lock().unwrap() = None;
        OWN_FLEET_STORE.lock().unwrap().clear();
        let _ = navigation_view::clear_viewed_system();
        PENDING_FLEET_EVENTS.lock().unwrap().clear();
    }

    fn fleet(id: i64, kind: FleetKind) -> Fleet {
        fleet_in_system(id, kind, Some(7))
    }

    fn fleet_in_system(id: i64, kind: FleetKind, system_id: Option<i64>) -> Fleet {
        Fleet {
            id,
            observed_at: Instant::now(),
            system_id,
            kind,
            combat_class: CombatClass::Unknown,
            fleet_type: None,
            hull_type: None,
            hull_name: None,
            local_player: kind == FleetKind::Own,
            system_position: Some(Vector3 { x: 1.0, y: 2.0, z: 3.0 }),
            strength: Some(100),
            level: Some(22),
            mining: Some(false),
            max_impulse_speed: None,
            max_warp_speed: None,
            travel_direction: None,
            time_since_last_update: None,
            movement_state: FleetMovementState::Unknown,
        }
    }

    fn fleet_with_motion(
        id: i64,
        kind: FleetKind,
        position: Vector3,
        speed: Option<f32>,
        direction: Option<Vector3>,
        observed_age: Option<Duration>,
    ) -> Fleet {
        Fleet {
            observed_at: Instant::now() - observed_age.unwrap_or_default(),
            system_position: Some(position),
            max_impulse_speed: speed,
            travel_direction: direction,
            movement_state: FleetMovementState::Impulsing,
            ..fleet(id, kind)
        }
    }

    fn fleet_with_state(id: i64, kind: FleetKind, movement_state: FleetMovementState) -> Fleet {
        Fleet { movement_state, ..fleet(id, kind) }
    }

    /// Sample fleet at (10,0,20) impulsing toward (3,0,4) at speed 6, observed 2s ago.
    ///
    /// Shared by the projection tests, which need an identical motion profile and differ only in fleet kind.
    fn moving_sample_fleet(kind: FleetKind) -> Fleet {
        fleet_with_motion(
            1,
            kind,
            Vector3 { x: 10.0, y: 0.0, z: 20.0 },
            Some(6.0),
            Some(Vector3 { x: 3.0, y: 0.0, z: 4.0 }),
            Some(Duration::from_secs(2)),
        )
    }

    /// Own fleet at the origin, stationary, with impulse speed 10. Reference point for the intercept tests.
    fn origin_own_fleet() -> Fleet {
        fleet_with_motion(1, FleetKind::Own, Vector3::default(), Some(10.0), None, None)
    }

    /// Hostile at (100,0,0) impulsing along +x at speed 5. Shared by the intercept selection tests.
    fn hostile_moving_east(id: i64) -> Fleet {
        fleet_with_motion(
            id,
            FleetKind::Hostile,
            Vector3 { x: 100.0, y: 0.0, z: 0.0 },
            Some(5.0),
            Some(Vector3 { x: 1.0, y: 0.0, z: 0.0 }),
            None,
        )
    }

    /// Lock the viewed-system store and run an assertion closure against it.
    fn with_store(check: impl FnOnce(&FleetStore)) {
        let guard = FLEET_STORE.lock().unwrap_or_else(|e| e.into_inner());
        check(guard.as_ref().expect("expected an initialized store"));
    }

    /// Assert which fleet IDs are present and which are absent in the viewed-system store.
    fn assert_members(stored: &FleetStore, present: &[i64], absent: &[i64]) {
        for id in present {
            assert!(stored.fleets.contains_key(id), "expected fleet {id} present");
        }
        for id in absent {
            assert!(!stored.fleets.contains_key(id), "expected fleet {id} absent");
        }
    }

    fn assert_vector_near(actual: Vector3, expected: Vector3) {
        const EPSILON: f32 = 0.01;

        assert!(
            (actual.x - expected.x).abs() <= EPSILON,
            "x: actual={}, expected={}",
            actual.x,
            expected.x
        );
        assert!(
            (actual.y - expected.y).abs() <= EPSILON,
            "y: actual={}, expected={}",
            actual.y,
            expected.y
        );
        assert!(
            (actual.z - expected.z).abs() <= EPSILON,
            "z: actual={}, expected={}",
            actual.z,
            expected.z
        );
    }

    fn sorted_actions(changes: &[FleetStoreChange]) -> Vec<(i64, FleetStoreAction)> {
        let mut actions = changes
            .iter()
            .map(|change| (change.fleet.id, change.action))
            .collect::<Vec<_>>();
        actions.sort_by_key(|(id, _)| *id);
        actions
    }

    fn change(action: FleetStoreAction, fleet: Fleet) -> FleetStoreChange {
        FleetStoreChange {
            action,
            fleet,
            changed_fields: Vec::new(),
        }
    }

    fn fleet_ref(id: i64, system_id: Option<i64>) -> FleetRef {
        FleetRef { id, system_id }
    }

    #[test]
    fn fleet_model_readiness_rejects_missing_current_state_accessor() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        let resolved = NonNull::<MethodInfo>::dangling().as_ptr();
        let _state_guard = PointerStateGuard::set(fleet_model_accessor_slots(), resolved);

        assert!(fleet_model_accessors_ready());

        GET_FLEET_CURRENT_STATE_FN.store(std::ptr::null_mut(), Relaxed);

        assert!(!fleet_model_accessors_ready());
    }

    #[test]
    fn fleet_event_readiness_rejects_each_missing_required_hook() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        let installed = NonNull::<()>::dangling().as_ptr();
        let hooks = fleet_event_container_hook_slots();
        let _state_guard = PointerStateGuard::set(hooks, installed);

        assert!(fleet_event_container_hooks_ready());

        for hook in &hooks {
            hook.store(std::ptr::null_mut(), Relaxed);
            assert!(!fleet_event_container_hooks_ready());
            hook.store(installed, Relaxed);
        }
    }

    #[test]
    fn intercept_time_accounts_for_target_velocity() {
        let time = intercept_time(
            Vector3::default(),
            10.0,
            Vector3 { x: 100.0, y: 0.0, z: 0.0 },
            Vector3 { x: 5.0, y: 0.0, z: 0.0 },
        );

        assert_eq!(time, Some(20.0));
    }

    #[test]
    fn projected_position_uses_observed_age_and_normalized_direction() {
        let fleet = moving_sample_fleet(FleetKind::Hostile);

        assert_vector_near(
            current_projected_position(&fleet).expect("expected current position"),
            Vector3 { x: 17.2, y: 0.0, z: 29.6 },
        );
    }

    #[test]
    fn position_projects_only_while_impulsing() {
        let fleet = moving_sample_fleet(FleetKind::Own);

        assert_vector_near(
            current_position_for_intercept(&fleet).expect("expected projected position"),
            Vector3 { x: 17.2, y: 0.0, z: 29.6 },
        );

        let stopped = Fleet {
            movement_state: FleetMovementState::Stopped,
            ..fleet.clone()
        };
        assert_eq!(
            current_position_for_intercept(&stopped),
            Some(Vector3 { x: 10.0, y: 0.0, z: 20.0 }),
        );
    }

    #[test]
    fn warping_position_is_not_available_for_system_intercept() {
        let fleet = fleet_with_state(1, FleetKind::Own, FleetMovementState::Warping);

        assert_eq!(current_position_for_intercept(&fleet), None);
    }

    #[test]
    fn unknown_position_uses_stored_position_without_projection() {
        let fleet = Fleet {
            movement_state: FleetMovementState::Unknown,
            ..moving_sample_fleet(FleetKind::Hostile)
        };

        assert_eq!(
            current_position_for_intercept(&fleet),
            Some(Vector3 { x: 10.0, y: 0.0, z: 20.0 }),
        );
    }

    #[test]
    fn select_fastest_intercept_picks_lowest_calculated_time() {
        let own_fleet = origin_own_fleet();
        let hostiles = vec![
            hostile_moving_east(2),
            fleet_with_motion(3, FleetKind::Hostile, Vector3 { x: 90.0, y: 0.0, z: 0.0 }, None, None, None),
        ];

        let selection = select_fastest_intercept(&own_fleet, &hostiles).expect("expected intercept target");

        assert_eq!(selection.hostile.id, 3);
        assert_eq!(selection.time_seconds, 9.0);
        assert_eq!(selection.intercept_position, Vector3 { x: 90.0, y: 0.0, z: 0.0 },);
    }

    #[test]
    fn select_fastest_intercept_skips_hostiles_without_position() {
        let own_fleet = origin_own_fleet();
        let mut hostile = fleet(2, FleetKind::Hostile);
        hostile.system_position = None;

        assert!(select_fastest_intercept(&own_fleet, &[hostile]).is_none());
    }

    #[test]
    fn select_fastest_intercept_skips_warping_hostiles() {
        let own_fleet = origin_own_fleet();
        let hostile = Fleet {
            movement_state: FleetMovementState::Warping,
            ..hostile_moving_east(2)
        };

        assert!(select_fastest_intercept(&own_fleet, &[hostile]).is_none());
    }

    #[test]
    fn classifies_deployed_movement_states() {
        assert_eq!(classify_movement_state(None), FleetMovementState::Unknown);
        assert_eq!(classify_movement_state(Some(DEPLOYED_STATE_IDLE)), FleetMovementState::Stopped);
        assert_eq!(
            classify_movement_state(Some(DEPLOYED_STATE_MOVING)),
            FleetMovementState::Impulsing
        );
        assert_eq!(
            classify_movement_state(Some(DEPLOYED_STATE_WARPING)),
            FleetMovementState::Warping
        );
        assert_eq!(
            classify_movement_state(Some(DEPLOYED_STATE_RECALL)),
            FleetMovementState::Warping
        );
        assert_eq!(
            classify_movement_state(Some(DEPLOYED_STATE_DOCKED)),
            FleetMovementState::Stopped
        );
        assert_eq!(classify_movement_state(Some(99)), FleetMovementState::Unknown);
    }

    #[test]
    fn classifies_known_fleet_types() {
        assert_eq!(classify_fleet(true, Some(DEPLOYED_FLEET_TYPE_PLAYER), None), FleetKind::Own);
        assert_eq!(classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_PLAYER), None), FleetKind::Player);
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_HOSTILE), Some(HULL_TYPE_ARMADA_TARGET)),
            FleetKind::Armada
        );
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_HOSTILE), Some(0)),
            FleetKind::Hostile
        );
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_NPC_INSTANTIATED), None),
            FleetKind::Npc
        );
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_SENTINEL), None),
            FleetKind::Sentinel
        );
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_ALLIANCE), None),
            FleetKind::Alliance
        );
        assert_eq!(
            classify_fleet(false, Some(DEPLOYED_FLEET_TYPE_CHALLENGE), None),
            FleetKind::Challenge
        );
        assert_eq!(classify_fleet(false, Some(99), None), FleetKind::Other(99));
        assert_eq!(classify_fleet(false, None, None), FleetKind::Unknown);
    }

    #[test]
    fn valid_system_id_rejects_negative_values() {
        assert_eq!(valid_system_id(Some(-1)), None);
        assert_eq!(valid_system_id(Some(0)), Some(0));
        assert_eq!(valid_system_id(Some(897954743)), Some(897954743));
        assert_eq!(valid_system_id(None), None);
    }

    #[test]
    fn classifies_combat_class_from_hull_name() {
        assert_eq!(
            classify_combat_class(Some("Hull_L21_Explorer_Mar"), None),
            CombatClass::Explorer
        );
        assert_eq!(
            classify_combat_class(Some("Hull_L23_Destroyer_Mar"), None),
            CombatClass::Destroyer
        );
        assert_eq!(
            classify_combat_class(Some("Hull_L16_Battleship_Mar"), None),
            CombatClass::Battleship
        );
        assert_eq!(classify_combat_class(Some("SurveyShip"), None), CombatClass::Survey);
    }

    #[test]
    fn classifies_combat_class_from_hull_type_fallback() {
        assert_eq!(classify_combat_class(None, Some(0)), CombatClass::Destroyer);
        assert_eq!(classify_combat_class(None, Some(1)), CombatClass::Survey);
        assert_eq!(classify_combat_class(None, Some(2)), CombatClass::Explorer);
        assert_eq!(classify_combat_class(None, Some(3)), CombatClass::Battleship);
        assert_eq!(classify_combat_class(None, Some(99)), CombatClass::Unknown);
        assert_eq!(classify_combat_class(None, None), CombatClass::Unknown);
    }

    #[test]
    fn store_enter_fleets_stores_owned_entries_by_id() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let result = store_enter_fleets(7, vec![fleet(1, FleetKind::Player), fleet(2, FleetKind::Hostile)]);

        let StoreEnterResult::Replaced { stored, changes } = result else {
            panic!("expected replace result");
        };
        assert_eq!(stored, 2);
        assert_eq!(
            sorted_actions(&changes),
            vec![(1, FleetStoreAction::Inserted), (2, FleetStoreAction::Inserted)]
        );
        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_eq!(s.fleets.len(), 2);
            assert_eq!(s.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(s.fleets.get(&2).unwrap().kind, FleetKind::Hostile);
        });

        reset_store();
    }

    #[test]
    fn store_enter_fleets_excludes_own_fleets_from_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let result = store_enter_fleets(7, vec![fleet(1, FleetKind::Own), fleet(2, FleetKind::Hostile)]);

        let StoreEnterResult::Replaced { stored, changes } = result else {
            panic!("expected replace result");
        };
        assert_eq!(stored, 1);
        assert_eq!(sorted_actions(&changes), vec![(2, FleetStoreAction::Inserted)]);
        with_store(|s| assert_members(s, &[2], &[1]));

        reset_store();
    }

    #[test]
    fn store_enter_fleets_deduplicates_by_latest_id() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let result = store_enter_fleets(7, vec![fleet(1, FleetKind::Player), fleet(1, FleetKind::Hostile)]);

        let StoreEnterResult::Replaced { stored, changes } = result else {
            panic!("expected replace result");
        };
        assert_eq!(stored, 1);
        assert_eq!(sorted_actions(&changes), vec![(1, FleetStoreAction::Inserted)]);
        assert_eq!(changes.first().unwrap().fleet.kind, FleetKind::Hostile);
        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_eq!(s.fleets.get(&1).unwrap().kind, FleetKind::Hostile);
        });

        reset_store();
    }

    #[test]
    fn formats_fleet_ids_sorted_for_trace_logs() {
        assert_eq!(format_fleet_ids(&[]), "[]");
        assert_eq!(format_fleet_ids(&[3, 1, 2]), "[1, 2, 3]");
    }

    #[test]
    fn viewed_system_tracks_navigation_view_events() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(42));
        assert_eq!(navigation_view::current_viewed_system_id(), Some(42));

        set_viewed_system(None);
        assert_eq!(navigation_view::current_viewed_system_id(), Some(42));

        set_viewed_system(Some(7));
        clear_viewed_system("test");
        assert_eq!(navigation_view::current_viewed_system_id(), None);

        reset_store();
    }

    #[test]
    fn store_enter_fleets_replaces_system_and_fleets_together() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player)]);
        let result = store_enter_fleets(8, vec![fleet_in_system(2, FleetKind::Hostile, Some(8))]);

        let StoreEnterResult::Replaced { stored, changes } = result else {
            panic!("expected replace result");
        };
        assert_eq!(stored, 1);
        assert_eq!(sorted_actions(&changes), vec![(2, FleetStoreAction::Inserted)]);
        with_store(|s| {
            assert_eq!(s.system_id, Some(8));
            assert_eq!(s.fleets.len(), 1);
            assert_members(s, &[2], &[1]);
        });

        reset_store();
    }

    #[test]
    fn store_enter_fleets_upserts_same_system_without_reset() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player), fleet(2, FleetKind::Hostile)]);
        let result = store_enter_fleets(7, vec![fleet(2, FleetKind::Armada), fleet(3, FleetKind::Npc)]);

        let StoreEnterResult::Upserted {
            added,
            updated,
            unchanged,
            total,
            changes,
        } = result
        else {
            panic!("expected upsert result");
        };
        assert_eq!(added, 1);
        assert_eq!(updated, 1);
        assert_eq!(unchanged, 0);
        assert_eq!(total, 3);
        assert_eq!(
            sorted_actions(&changes),
            vec![(2, FleetStoreAction::Updated), (3, FleetStoreAction::Inserted)]
        );
        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_eq!(s.fleets.len(), 3);
            assert_eq!(s.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(s.fleets.get(&2).unwrap().kind, FleetKind::Armada);
            assert_eq!(s.fleets.get(&3).unwrap().kind, FleetKind::Npc);
        });

        reset_store();
    }

    #[test]
    fn store_enter_fleets_upsert_ignores_unchanged_existing_fleet() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player)]);
        let result = store_enter_fleets(7, vec![fleet(1, FleetKind::Player)]);

        let StoreEnterResult::Upserted {
            added,
            updated,
            unchanged,
            total,
            changes,
        } = result
        else {
            panic!("expected upsert result");
        };
        assert_eq!(added, 0);
        assert_eq!(updated, 0);
        assert_eq!(unchanged, 1);
        assert_eq!(total, 1);
        assert!(changes.is_empty());

        reset_store();
    }

    #[test]
    fn store_enter_fleets_upsert_tracks_changed_existing_fleet() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player)]);
        let mut changed = fleet(1, FleetKind::Player);
        changed.strength = Some(200);
        let result = store_enter_fleets(7, vec![changed]);

        let StoreEnterResult::Upserted {
            added,
            updated,
            unchanged,
            total,
            changes,
        } = result
        else {
            panic!("expected upsert result");
        };
        assert_eq!(added, 0);
        assert_eq!(updated, 1);
        assert_eq!(unchanged, 0);
        assert_eq!(total, 1);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].action, FleetStoreAction::Updated);
        assert_eq!(changes[0].changed_fields[0].name, "strength");

        reset_store();
    }

    #[test]
    fn store_update_fleets_updates_existing_and_inserts_matching_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player), fleet(2, FleetKind::Hostile)]);
        let result = store_update_fleets(
            7,
            vec![
                fleet(2, FleetKind::Armada),
                fleet(3, FleetKind::Npc),
                fleet_in_system(4, FleetKind::Hostile, Some(8)),
                fleet_in_system(5, FleetKind::Hostile, None),
            ],
        );

        assert_eq!(result.inserted, 1);
        assert_eq!(result.updated, 1);
        assert_eq!(result.ignored, 2);
        assert_eq!(result.ignored_fleet_ids, vec![4, 5]);
        assert_eq!(result.total, 3);
        assert_eq!(
            sorted_actions(&result.changes),
            vec![(2, FleetStoreAction::Updated), (3, FleetStoreAction::Inserted)]
        );
        with_store(|s| {
            assert_eq!(s.fleets.len(), 3);
            assert_eq!(s.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(s.fleets.get(&2).unwrap().kind, FleetKind::Armada);
            assert_eq!(s.fleets.get(&3).unwrap().kind, FleetKind::Npc);
            assert_members(s, &[], &[4, 5]);
        });

        reset_store();
    }

    #[test]
    fn store_update_fleets_excludes_own_fleets_from_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let result = store_update_fleets(7, vec![fleet(1, FleetKind::Own), fleet(2, FleetKind::Hostile)]);

        assert_eq!(result.inserted, 1);
        assert_eq!(result.ignored, 0);
        assert_eq!(sorted_actions(&result.changes), vec![(2, FleetStoreAction::Inserted)]);
        with_store(|s| assert_members(s, &[2], &[1]));

        reset_store();
    }

    #[test]
    fn own_fleet_store_accepts_only_own_fleets() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let changes = store_own_fleets(&[fleet(1, FleetKind::Own), fleet(2, FleetKind::Hostile)]);

        assert_eq!(sorted_actions(&changes), vec![(1, FleetStoreAction::Inserted)]);
        assert!(own_fleet(1).is_some());
        assert!(own_fleet(2).is_none());

        reset_store();
    }

    #[test]
    fn own_fleet_store_updates_independent_of_viewed_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![fleet_in_system(1, FleetKind::Own, Some(99))],
        });

        assert!(PENDING_FLEET_EVENTS.lock().unwrap().is_empty());
        assert_eq!(own_fleet(1).unwrap().system_id, Some(99));

        reset_store();
    }

    #[test]
    fn own_only_update_is_not_queued_for_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        assert!(!route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![fleet_in_system(1, FleetKind::Own, Some(99))],
        }));

        assert!(own_fleet(1).is_some());
        assert!(PENDING_FLEET_EVENTS.lock().unwrap().is_empty());

        reset_store();
    }

    #[test]
    fn mixed_update_queues_only_non_own_fleets_for_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        assert!(!route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![
                fleet_in_system(1, FleetKind::Own, Some(99)),
                fleet_in_system(2, FleetKind::Hostile, Some(99)),
            ],
        }));

        assert!(own_fleet(1).is_some());
        {
            let queue = PENDING_FLEET_EVENTS.lock().unwrap();
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().fleet_count(), 1);
        }

        reset_store();
    }

    #[test]
    fn own_fleet_invalidation_clears_live_system_fields_only() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        let mut own = fleet(1, FleetKind::Own);
        own.hull_name = Some("Franklin".to_string());
        own.max_impulse_speed = Some(10.0);
        own.travel_direction = Some(Vector3 { x: 1.0, y: 0.0, z: 0.0 });
        own.movement_state = FleetMovementState::Impulsing;
        store_own_fleets(&[own]);

        let changes = invalidate_own_fleet_refs(&[fleet_ref(1, Some(7))]);
        let stored = own_fleet(1).unwrap();

        assert_eq!(changes.len(), 1);
        assert_eq!(stored.system_id, None);
        assert_eq!(stored.system_position, None);
        assert_eq!(stored.travel_direction, None);
        assert_eq!(stored.movement_state, FleetMovementState::Unknown);
        assert_eq!(stored.hull_name.as_deref(), Some("Franklin"));
        assert_eq!(stored.max_impulse_speed, Some(10.0));

        reset_store();
    }

    #[test]
    fn own_fleet_and_hostile_fleets_are_read_separately() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_own_fleets(&[fleet(1, FleetKind::Own)]);
        store_enter_fleets(7, vec![fleet(2, FleetKind::Hostile), fleet(3, FleetKind::Player)]);

        let own = own_fleet(1).expect("expected own fleet");
        let hostiles = hostile_fleets();

        assert_eq!(own.id, 1);
        assert_eq!(hostiles.len(), 1);
        assert_eq!(hostiles[0].id, 2);

        reset_store();
    }

    #[test]
    fn store_update_fleets_updates_known_id_without_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(2, FleetKind::Hostile)]);
        let result = store_update_fleets(7, vec![fleet_in_system(2, FleetKind::Armada, None)]);

        assert_eq!(result.inserted, 0);
        assert_eq!(result.updated, 1);
        assert_eq!(result.ignored, 0);
        assert_eq!(result.total, 1);
        with_store(|s| {
            let fleet = s.fleets.get(&2).unwrap();
            assert_eq!(fleet.kind, FleetKind::Armada);
            assert_eq!(fleet.system_id, Some(7));
        });

        reset_store();
    }

    #[test]
    fn store_update_fleets_tracks_field_diffs() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(2, FleetKind::Hostile)]);
        let mut changed = fleet(2, FleetKind::Hostile);
        changed.strength = Some(200);

        let result = store_update_fleets(7, vec![changed]);

        assert_eq!(result.inserted, 0);
        assert_eq!(result.updated, 1);
        assert_eq!(result.unchanged, 0);
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].changed_fields.len(), 1);
        assert_eq!(result.changes[0].changed_fields[0].name, "strength");
        assert_eq!(result.changes[0].changed_fields[0].old_value, "100");
        assert_eq!(result.changes[0].changed_fields[0].new_value, "200");

        reset_store();
    }

    #[test]
    fn store_update_fleets_tracks_position_diffs() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(2, FleetKind::Hostile)]);
        let mut changed = fleet(2, FleetKind::Hostile);
        changed.system_position = Some(Vector3 { x: 9.0, y: 8.0, z: 7.0 });

        let result = store_update_fleets(7, vec![changed]);

        assert_eq!(result.inserted, 0);
        assert_eq!(result.updated, 1);
        assert_eq!(result.unchanged, 0);
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].changed_fields.len(), 1);
        assert_eq!(result.changes[0].changed_fields[0].name, "position");
        assert_eq!(result.changes[0].changed_fields[0].old_value, "(1.00, 2.00, 3.00)");
        assert_eq!(result.changes[0].changed_fields[0].new_value, "(9.00, 8.00, 7.00)");
        with_store(|s| {
            assert_eq!(
                s.fleets.get(&2).unwrap().system_position,
                Some(Vector3 { x: 9.0, y: 8.0, z: 7.0 })
            );
        });

        reset_store();
    }

    #[test]
    fn store_remove_fleets_removes_existing_entries_only() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player), fleet(2, FleetKind::Hostile)]);
        let result = store_remove_fleets(7, vec![fleet_ref(2, None), fleet_ref(3, Some(7))]);

        assert_eq!(result.vanished, 1);
        assert_eq!(result.ignored, 1);
        assert_eq!(result.ignored_fleet_ids, vec![3]);
        assert_eq!(result.total, 1);
        assert_eq!(sorted_actions(&result.changes), vec![(2, FleetStoreAction::Vanished)]);
        with_store(|s| {
            assert_eq!(s.fleets.len(), 1);
            assert_members(s, &[1], &[2]);
        });

        reset_store();
    }

    #[test]
    fn enter_event_is_queued_without_viewed_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        route_fleet_event(PendingFleetEvent::EnterSystem {
            system_id: 7,
            fleets: vec![fleet(1, FleetKind::Hostile)],
        });

        assert_eq!(PENDING_FLEET_EVENTS.lock().unwrap().len(), 1);
        assert!(FLEET_STORE.lock().unwrap().is_none());

        reset_store();
    }

    #[test]
    fn pending_enter_is_processed_for_matching_viewed_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        queue_pending_fleet_event(PendingFleetEvent::EnterSystem {
            system_id: 7,
            fleets: vec![fleet(1, FleetKind::Hostile)],
        });
        set_viewed_system(Some(7));

        assert!(PENDING_FLEET_EVENTS.lock().unwrap().is_empty());
        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_members(s, &[1], &[]);
        });

        reset_store();
    }

    #[test]
    fn pending_enter_is_dropped_for_other_viewed_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        queue_pending_fleet_event(PendingFleetEvent::EnterSystem {
            system_id: 8,
            fleets: vec![fleet_in_system(1, FleetKind::Hostile, Some(8))],
        });
        set_viewed_system(Some(7));

        assert!(PENDING_FLEET_EVENTS.lock().unwrap().is_empty());
        assert!(FLEET_STORE.lock().unwrap().is_none());

        reset_store();
    }

    #[test]
    fn pending_queue_is_capped_and_drops_oldest_events() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        for system_id in 0..=max_pending_fleet_events() {
            queue_pending_fleet_event(PendingFleetEvent::EnterSystem {
                system_id: system_id as i64,
                fleets: Vec::new(),
            });
        }

        let queue = PENDING_FLEET_EVENTS.lock().unwrap();
        assert_eq!(queue.len(), max_pending_fleet_events());
        assert_eq!(queue.front().unwrap().system_id(), Some(1));
        assert_eq!(queue.back().unwrap().system_id(), Some(max_pending_fleet_events() as i64));
        drop(queue);

        reset_store();
    }

    #[test]
    fn view_clear_preserves_store_and_pending_queue() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(7));
        store_enter_fleets(7, vec![fleet(1, FleetKind::Hostile)]);
        queue_pending_fleet_event(PendingFleetEvent::EnterSystem {
            system_id: 8,
            fleets: vec![fleet_in_system(2, FleetKind::Hostile, Some(8))],
        });

        clear_viewed_system("test");

        assert_eq!(navigation_view::current_viewed_system_id(), None);
        assert_eq!(PENDING_FLEET_EVENTS.lock().unwrap().len(), 1);
        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_members(s, &[1], &[]);
        });

        reset_store();
    }

    #[test]
    fn fleet_exit_removes_only_exited_fleets_from_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(7));
        store_enter_fleets(7, vec![fleet(1, FleetKind::Hostile), fleet(2, FleetKind::Hostile)]);

        assert!(process_remove_refs("fleet_exit_system", 7, vec![fleet_ref(1, Some(7))]));

        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_members(s, &[2], &[1]);
        });

        reset_store();
    }

    #[test]
    fn update_known_fleet_without_system_is_applied_for_viewed_store() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(7));
        store_enter_fleets(7, vec![fleet(1, FleetKind::Hostile)]);
        route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![fleet_in_system(1, FleetKind::Armada, None)],
        });

        with_store(|s| {
            let fleet = s.fleets.get(&1).unwrap();
            assert_eq!(fleet.kind, FleetKind::Armada);
            assert_eq!(fleet.system_id, Some(7));
        });

        reset_store();
    }

    #[test]
    fn update_unknown_fleet_is_inserted_when_system_matches_view() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(7));
        route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![fleet(1, FleetKind::Hostile)],
        });

        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert_members(s, &[1], &[]);
        });

        reset_store();
    }

    #[test]
    fn update_unknown_fleet_without_matching_system_is_ignored() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        set_viewed_system(Some(7));
        route_fleet_event(PendingFleetEvent::Update {
            reason: "fleet_update",
            fleets: vec![fleet_in_system(1, FleetKind::Hostile, None), fleet_in_system(2, FleetKind::Hostile, Some(8))],
        });

        with_store(|s| {
            assert_eq!(s.system_id, Some(7));
            assert!(s.fleets.is_empty());
        });

        reset_store();
    }
}
