//! Fleet scanner hooks for the currently viewed system.
//!
//! Deployment events are converted to owned snapshots immediately. Runtime Unity/IL2CPP pointers are never stored
//! beyond the read operation. Events observed before a concrete system view is known are held in a bounded pending
//! queue and flushed once the viewed system changes.

use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};

use log::{debug, trace, warn};

use crate::hook::safety::HookInfo;
use crate::hooks::navigation_view;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

mod model;
mod store;

use model::*;
use store::*;

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

/// `HullSpec.get_Type()`.
static GET_HULL_TYPE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `HullSpec.get_Name()`.
static GET_HULL_NAME_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `NodeAddress.get_System()`.
static GET_NODE_ADDRESS_SYSTEM_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

// ---- Original trampolines --------------------------------------------------

static ORIG_FLEETS_ENTER_SYSTEM: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEETS_EXIT_SYSTEM: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEETS_DISPOSED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEETS_UPDATED: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_FLEET_STATE_CHANGE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_DID_CHANGE_VIEW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_LEAVE_NAVIGATION_VIEW: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Assemblies to search for PrimeServer model classes.
const MODEL_ASSEMBLIES: &[&str] = &["Digit.Client.PrimeLib.Runtime", "Assembly-CSharp", "Assembly-CSharp-firstpass"];

static HOOK_INFO: HookInfo = HookInfo::new("FleetScanner");

// ---- Type aliases ----------------------------------------------------------

type FleetsSystemFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppList<*mut Il2CppObject>, *const MethodInfo);
type FleetEventFn = unsafe extern "C" fn(*mut Il2CppList<*mut Il2CppObject>, *const MethodInfo);
type ViewChangedFn = unsafe extern "C" fn(*mut Il2CppObject, *const MethodInfo);
type NoParamEventFn = unsafe extern "C" fn(*const MethodInfo);

// ---- Hooks ----------------------------------------------------------------

/// Observe fleet batches that enter a system and copy them into owned snapshots.
extern "C" fn hook_fleets_enter_system(
    address: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    let orig = ORIG_FLEETS_ENTER_SYSTEM.load(Relaxed);
    if !orig.is_null() {
        let original: FleetsSystemFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(address, fleets, method_info) };
    }

    HOOK_INFO.run(|| process_enter_system(address, fleets));
}

/// Observe fleet batches that leave a system and clear the viewed store when relevant.
extern "C" fn hook_fleets_exit_system(
    address: *mut Il2CppObject,
    fleets: *mut Il2CppList<*mut Il2CppObject>,
    method_info: *const MethodInfo,
) {
    let orig = ORIG_FLEETS_EXIT_SYSTEM.load(Relaxed);
    if !orig.is_null() {
        let original: FleetsSystemFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(address, fleets, method_info) };
    }

    HOOK_INFO.run(|| process_exit_system(address));
}

/// Observe disposed fleets and remove matching owned snapshots.
extern "C" fn hook_fleets_disposed(fleets: *mut Il2CppList<*mut Il2CppObject>, method_info: *const MethodInfo) {
    let orig = ORIG_FLEETS_DISPOSED.load(Relaxed);
    if !orig.is_null() {
        let original: FleetEventFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(fleets, method_info) };
    }

    HOOK_INFO.run(|| process_fleets_disposed(fleets));
}

/// Observe regular fleet update batches.
extern "C" fn hook_fleets_updated(fleets: *mut Il2CppList<*mut Il2CppObject>, method_info: *const MethodInfo) {
    let orig = ORIG_FLEETS_UPDATED.load(Relaxed);
    if !orig.is_null() {
        let original: FleetEventFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(fleets, method_info) };
    }

    HOOK_INFO.run(|| process_fleets_updated("fleet_update", fleets));
}

/// Observe fleet state-change batches, which use the same owned update path.
extern "C" fn hook_fleet_state_change(fleets: *mut Il2CppList<*mut Il2CppObject>, method_info: *const MethodInfo) {
    let orig = ORIG_FLEET_STATE_CHANGE.load(Relaxed);
    if !orig.is_null() {
        let original: FleetEventFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(fleets, method_info) };
    }

    HOOK_INFO.run(|| process_fleets_updated("fleet_state_change", fleets));
}

/// Observe concrete navigation view changes and set the viewed system.
extern "C" fn hook_did_change_view(address: *mut Il2CppObject, method_info: *const MethodInfo) {
    let orig = ORIG_DID_CHANGE_VIEW.load(Relaxed);
    if !orig.is_null() {
        let original: ViewChangedFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(address, method_info) };
    }

    HOOK_INFO.run(|| set_viewed_system(node_address_system(address)));
}

/// Observe leaving the navigation view and clear the viewed system.
extern "C" fn hook_leave_navigation_view(method_info: *const MethodInfo) {
    let orig = ORIG_LEAVE_NAVIGATION_VIEW.load(Relaxed);
    if !orig.is_null() {
        let original: NoParamEventFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(method_info) };
    }

    HOOK_INFO.run(|| clear_viewed_system("leave_navigation_view"));
}

// ---- Processing ------------------------------------------------------------

/// Convert an enter-system event into owned fleets and route it through the view gate.
fn process_enter_system(address: *mut Il2CppObject, fleets: *mut Il2CppList<*mut Il2CppObject>) {
    let Some(system_id) = node_address_system(address) else {
        trace!(target: "FleetScanner", "Fleet enter event ignored without valid system");
        return;
    };

    let fleets = unsafe { list_objects(fleets) }
        .into_iter()
        .filter_map(|fleet| inspect_fleet_in_system(fleet, Some(system_id)))
        .collect::<Vec<_>>();

    route_fleet_event(PendingFleetEvent::EnterSystem { system_id, fleets });
}

/// Clear the store only when the exit event belongs to the currently viewed system.
fn process_exit_system(address: *mut Il2CppObject) {
    let Some(system_id) = node_address_system(address) else {
        trace!(target: "FleetScanner", "Fleet exit event ignored without valid system");
        return;
    };

    if navigation_view::current_viewed_system_id() == Some(system_id) {
        reset_fleet_store("exit_system");
    } else {
        trace!(
            target: "FleetScanner",
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

/// Apply an owned update batch to the store and decide whether it is debug-worthy.
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
            target: "FleetScanner",
            "Fleet update ignored: reason={reason}, system={system_id}, ids={}",
            format_fleet_ids(&ignored_fleet_ids),
        );
    }

    if changes.is_empty()
        || inserted == 0 && updated == 0
        || is_movement_only_update(&changes)
        || is_player_only_update(&changes)
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

/// Remove owned fleet references from the viewed store.
fn process_dispose_refs(system_id: i64, fleet_refs: Vec<FleetRef>) -> bool {
    let StoreRemoveResult {
        vanished,
        ignored,
        total,
        changes,
        ignored_fleet_ids,
    } = store_remove_fleets(system_id, fleet_refs);
    let summary = format!(
        "Fleet store remove: reason=fleet_disposed, system={system_id}, vanished={vanished}, ignored={ignored}, total={total}"
    );

    if ignored > 0 {
        trace!(
            target: "FleetScanner",
            "Fleet dispose ignored: reason=not_in_store, system={system_id}, ids={}",
            format_fleet_ids(&ignored_fleet_ids),
        );
    }

    if vanished == 0 {
        trace_fleet_changes(&summary, &changes);
    } else if total == 0 {
        debug!(target: "FleetScanner", "{summary}");
    } else {
        log_fleet_changes(&summary, &changes);
    }

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

    debug!(target: "FleetScanner", "Viewed system changed: system={system_id}");

    flush_pending_fleet_events(system_id);
}

/// Clear the shared viewed system and drop the active store.
fn clear_viewed_system(reason: &str) {
    if !navigation_view::clear_viewed_system() {
        return;
    }

    debug!(target: "FleetScanner", "Viewed system cleared: reason={reason}");
    reset_fleet_store("view_left");
}

/// Send an event directly to the store, or queue it until a viewed system is known.
fn route_fleet_event(event: PendingFleetEvent) -> bool {
    match navigation_view::current_viewed_system_id() {
        Some(system_id) => process_pending_fleet_event(system_id, event),
        None => {
            queue_pending_fleet_event(event);
            false
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

    debug!(
        target: "FleetScanner",
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
                    target: "FleetScanner",
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
                    target: "FleetScanner",
                    "Fleet event ignored: reason=no_matching_fleets, event={reason}, viewed_system={system_id}",
                );
            }
            result
        }
        PendingFleetEvent::Dispose { fleets } => {
            let result = process_dispose_refs(system_id, fleets);
            if !result {
                trace!(
                    target: "FleetScanner",
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
    let local_player = is_local_player(fleet);
    let fleet_type = fleet_type(fleet);
    let hull_type = fleet_hull_type(fleet);
    let hull_name = fleet_hull_name(fleet);

    Some(Fleet {
        id,
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
    })
}

/// Read only the fields needed to remove a fleet from the store.
fn inspect_fleet_ref(fleet: *mut Il2CppObject) -> Option<FleetRef> {
    Some(FleetRef {
        id: fleet_id(fleet)?,
        system_id: fleet_address_system(fleet),
    })
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

// ---- Installation ----------------------------------------------------------

/// Install accessors and event hooks for fleet scanning.
pub fn install(api: &Il2CppApi) {
    install_model_accessors(api);
    install_node_address_accessors(api);
    install_navigation_view_hooks(api);
    install_event_hooks(api);
    trace!(target: "FleetScanner", "Fleet scanner install finished");
}

/// Resolve all FleetDeployedData and HullSpec getters used for snapshots.
fn install_model_accessors(api: &Il2CppApi) {
    if let Some(fleet_class) = resolve_model_class(api, "FleetDeployedData") {
        resolve_fn(api, fleet_class, "get_ID", 0, &GET_FLEET_ID_FN);
        resolve_fn(api, fleet_class, "get_SystemPosition", 0, &GET_FLEET_SYSTEM_POSITION_FN);
        resolve_fn(api, fleet_class, "get_Address", 0, &GET_FLEET_ADDRESS_FN);
        resolve_fn(api, fleet_class, "get_IsLocalPlayer", 0, &GET_FLEET_IS_LOCAL_PLAYER_FN);
        resolve_fn(api, fleet_class, "get_FleetType", 0, &GET_FLEET_TYPE_FN);
        resolve_fn(api, fleet_class, "get_Strength", 0, &GET_FLEET_STRENGTH_FN);
        resolve_fn(api, fleet_class, "get_Level", 0, &GET_FLEET_LEVEL_FN);
        resolve_fn(api, fleet_class, "get_IsMining", 0, &GET_FLEET_IS_MINING_FN);
        resolve_fn(api, fleet_class, "get_Hull", 0, &GET_FLEET_HULL_FN);
        resolve_fn(api, fleet_class, "get_MaxImpulseSpeed", 0, &GET_FLEET_MAX_IMPULSE_SPEED_FN);
        resolve_fn(api, fleet_class, "get_MaxWarpSpeed", 0, &GET_FLEET_MAX_WARP_SPEED_FN);
        resolve_fn(api, fleet_class, "get_TravelDirection", 0, &GET_FLEET_TRAVEL_DIRECTION_FN);
        resolve_fn(
            api,
            fleet_class,
            "get_TimeSinceLastUpdate",
            0,
            &GET_FLEET_TIME_SINCE_LAST_UPDATE_FN,
        );
    } else {
        warn!(target: "FleetScanner", "FleetDeployedData class not found");
    }

    if let Some(hull_class) = resolve_model_class(api, "HullSpec") {
        resolve_fn(api, hull_class, "get_Type", 0, &GET_HULL_TYPE_FN);
        resolve_fn(api, hull_class, "get_Name", 0, &GET_HULL_NAME_FN);
    } else {
        warn!(target: "FleetScanner", "HullSpec class not found");
    }
}

/// Resolve NodeAddress access needed for system IDs.
fn install_node_address_accessors(api: &Il2CppApi) {
    if let Some(address_class) = resolve_model_class(api, "NodeAddress") {
        resolve_fn(api, address_class, "get_System", 0, &GET_NODE_ADDRESS_SYSTEM_FN);
    } else {
        warn!(target: "FleetScanner", "NodeAddress class not found");
    }
}

/// Hook navigation view events that provide the viewed system lifecycle.
fn install_navigation_view_hooks(api: &Il2CppApi) {
    let Some(events_class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Navigation", "NavigationCameraEvents")
    else {
        warn!(target: "FleetScanner", "NavigationCameraEvents class not found");
        return;
    };

    trace!(target: "FleetScanner", "NavigationCameraEvents class found in assembly 'Assembly-CSharp'");

    install_hook(
        api,
        events_class,
        "TriggerDidChangeViewEvent",
        1,
        hook_did_change_view as *const (),
        &ORIG_DID_CHANGE_VIEW,
    );
    install_hook(
        api,
        events_class,
        "TriggerLeaveNavigationViewEvent",
        0,
        hook_leave_navigation_view as *const (),
        &ORIG_LEAVE_NAVIGATION_VIEW,
    );
}

/// Hook deployment events that provide fleet lifecycle changes.
fn install_event_hooks(api: &Il2CppApi) {
    let Some(events_class) = resolve_events_class(api) else {
        warn!(target: "FleetScanner", "DeploymentEvents class not found");
        return;
    };

    install_hook(
        api,
        events_class,
        "TriggerFleetsEnterSystemEvent",
        2,
        hook_fleets_enter_system as *const (),
        &ORIG_FLEETS_ENTER_SYSTEM,
    );
    install_hook(
        api,
        events_class,
        "TriggerFleetsExitSystemEvent",
        2,
        hook_fleets_exit_system as *const (),
        &ORIG_FLEETS_EXIT_SYSTEM,
    );
    install_hook(
        api,
        events_class,
        "TriggerFleetsDisposedEvent",
        1,
        hook_fleets_disposed as *const (),
        &ORIG_FLEETS_DISPOSED,
    );
    install_hook(
        api,
        events_class,
        "TriggerFleetsUpdatedEvent",
        1,
        hook_fleets_updated as *const (),
        &ORIG_FLEETS_UPDATED,
    );
    install_hook(
        api,
        events_class,
        "TriggerFleetStateChangeEvent",
        1,
        hook_fleet_state_change as *const (),
        &ORIG_FLEET_STATE_CHANGE,
    );
}

/// Resolve the DeploymentEvents class from known model assemblies.
fn resolve_events_class(api: &Il2CppApi) -> Option<*mut Il2CppClass> {
    for assembly in MODEL_ASSEMBLIES {
        if let Some(class) = resolver::resolve_class(api, assembly, "Digit.PrimeServer.Events", "DeploymentEvents") {
            trace!(target: "FleetScanner", "DeploymentEvents class found in assembly '{assembly}'");
            return Some(class);
        }
    }
    None
}

/// Resolve a PrimeServer model class from known model assemblies.
fn resolve_model_class(api: &Il2CppApi, class_name: &str) -> Option<*mut Il2CppClass> {
    for assembly in MODEL_ASSEMBLIES {
        if let Some(class) = resolver::resolve_class(api, assembly, "Digit.PrimeServer.Models", class_name) {
            trace!(target: "FleetScanner", "{class_name} class found in assembly '{assembly}'");
            return Some(class);
        }
    }
    None
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
    tracker::install_resolved_hook(
        api,
        class,
        method_name,
        param_count,
        &format!("FleetScanner.{method_name}"),
        hook,
        |orig| original.store(orig as *mut (), Relaxed),
    );
}

/// Resolve one method into an atomic MethodInfo pointer.
fn resolve_fn(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    target: &AtomicPtr<MethodInfo>,
) {
    resolver::resolve_method_into(api, class, method_name, param_count, target);
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::hooks::navigation_view::TEST_LOCK;

    fn reset_store() {
        *FLEET_STORE.lock().unwrap() = None;
        let _ = navigation_view::clear_viewed_system();
        PENDING_FLEET_EVENTS.lock().unwrap().clear();
    }

    fn fleet(id: i64, kind: FleetKind) -> Fleet {
        fleet_in_system(id, kind, Some(7))
    }

    fn fleet_in_system(id: i64, kind: FleetKind, system_id: Option<i64>) -> Fleet {
        Fleet {
            id,
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
        }
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

    fn change_with_fields(action: FleetStoreAction, fleet: Fleet, fields: &[&'static str]) -> FleetStoreChange {
        FleetStoreChange {
            action,
            fleet,
            changed_fields: fields
                .iter()
                .map(|field| FleetFieldChange {
                    name: field,
                    old_value: "old".to_string(),
                    new_value: "new".to_string(),
                })
                .collect(),
        }
    }

    fn fleet_ref(id: i64, system_id: Option<i64>) -> FleetRef {
        FleetRef { id, system_id }
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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert_eq!(stored.fleets.len(), 2);
            assert_eq!(stored.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(stored.fleets.get(&2).unwrap().kind, FleetKind::Hostile);
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert_eq!(stored.fleets.get(&1).unwrap().kind, FleetKind::Hostile);
        }

        reset_store();
    }

    #[test]
    fn reset_fleet_store_clears_fleets_and_system() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_store();

        store_enter_fleets(7, vec![fleet(1, FleetKind::Player)]);

        reset_fleet_store("test");

        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, None);
            assert!(stored.fleets.is_empty());
        }

        reset_store();
    }

    #[test]
    fn player_only_updates_are_trace_noise() {
        assert!(is_player_only_update(&[change(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Player),
        )]));
        assert!(!is_player_only_update(&[change(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Own),
        )]));
        assert!(!is_player_only_update(&[change(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Hostile),
        )]));
        assert!(!is_player_only_update(&[change(
            FleetStoreAction::Inserted,
            fleet(1, FleetKind::Player),
        )]));
        assert!(!is_player_only_update(&[
            change(FleetStoreAction::Updated, fleet(1, FleetKind::Player)),
            change(FleetStoreAction::Updated, fleet(2, FleetKind::Hostile)),
        ]));
        assert!(!is_player_only_update(&[]));
    }

    #[test]
    fn movement_only_updates_are_trace_noise() {
        assert!(is_movement_only_update(&[change_with_fields(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Hostile),
            &["position", "direction", "age"],
        )]));
        assert!(!is_movement_only_update(&[change_with_fields(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Hostile),
            &["position", "strength"],
        )]));
        assert!(!is_movement_only_update(&[change_with_fields(
            FleetStoreAction::Inserted,
            fleet(1, FleetKind::Hostile),
            &["position"],
        )]));
        assert!(!is_movement_only_update(&[change(
            FleetStoreAction::Updated,
            fleet(1, FleetKind::Hostile),
        )]));
        assert!(!is_movement_only_update(&[]));
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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(8));
            assert_eq!(stored.fleets.len(), 1);
            assert!(stored.fleets.contains_key(&2));
            assert!(!stored.fleets.contains_key(&1));
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert_eq!(stored.fleets.len(), 3);
            assert_eq!(stored.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(stored.fleets.get(&2).unwrap().kind, FleetKind::Armada);
            assert_eq!(stored.fleets.get(&3).unwrap().kind, FleetKind::Npc);
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.fleets.len(), 3);
            assert_eq!(stored.fleets.get(&1).unwrap().kind, FleetKind::Player);
            assert_eq!(stored.fleets.get(&2).unwrap().kind, FleetKind::Armada);
            assert_eq!(stored.fleets.get(&3).unwrap().kind, FleetKind::Npc);
            assert!(!stored.fleets.contains_key(&4));
            assert!(!stored.fleets.contains_key(&5));
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            let fleet = stored.fleets.get(&2).unwrap();
            assert_eq!(fleet.kind, FleetKind::Armada);
            assert_eq!(fleet.system_id, Some(7));
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(
                stored.fleets.get(&2).unwrap().system_position,
                Some(Vector3 { x: 9.0, y: 8.0, z: 7.0 })
            );
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.fleets.len(), 1);
            assert!(stored.fleets.contains_key(&1));
            assert!(!stored.fleets.contains_key(&2));
        }

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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert!(stored.fleets.contains_key(&1));
        }

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
    fn view_clear_resets_store_but_keeps_pending_queue() {
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
        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, None);
            assert!(stored.fleets.is_empty());
        }

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

        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            let fleet = stored.fleets.get(&1).unwrap();
            assert_eq!(fleet.kind, FleetKind::Armada);
            assert_eq!(fleet.system_id, Some(7));
        }

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

        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert!(stored.fleets.contains_key(&1));
        }

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

        {
            let guard = FLEET_STORE.lock().unwrap();
            let stored = guard.as_ref().unwrap();
            assert_eq!(stored.system_id, Some(7));
            assert!(stored.fleets.is_empty());
        }

        reset_store();
    }
}
