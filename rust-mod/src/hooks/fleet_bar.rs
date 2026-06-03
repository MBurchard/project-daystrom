//! Fleet bar state access.
//!
//! Tracks `FleetBarViewController` and exposes the currently selected fleet from `FleetBarContext`.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::warn;

use crate::hooks::tracker::{self, instance_tracker};
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

const LOG_TARGET: &str = "FleetBar";

instance_tracker!(fleet_bar);

/// Whether fleet bar lifecycle hooks have been installed successfully.
static TRACKER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// MethodInfo for `FleetBarViewController.get_CanvasContext()`.
static GET_CONTEXT_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetBarContext.get_CurrentFleet()`.
static GET_CURRENT_FLEET_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetBarContext.get_CurrentIndex()`.
static GET_CURRENT_INDEX_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetPlayerData.get_Id()`.
static GET_FLEET_ID_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetPlayerData.get_CurrentState()`.
static GET_FLEET_STATE_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetPlayerData.get_Hull()`.
static GET_FLEET_HULL_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `FleetPlayerData.get_Address()`.
static GET_FLEET_ADDRESS_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `HullSpec.get_Name()`.
static GET_HULL_NAME_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// MethodInfo for `NodeAddress.get_System()`.
static GET_NODE_ADDRESS_SYSTEM_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// `FleetPlayerData.LocationData`.
static OFFSET_FLEET_LOCATION_DATA: AtomicUsize = AtomicUsize::new(0);

// ---- Public API -----------------------------------------------------------

/// Details for the fleet selected in the bottom fleet bar.
pub(crate) struct SelectedFleet {
    pub(crate) index: Option<i32>,
    pub(crate) id: Option<i64>,
    pub(crate) system_id: Option<i64>,
    pub(crate) state: Option<i32>,
    pub(crate) hull_name: Option<String>,
    pub(crate) location_data: Option<*mut Il2CppObject>,
}

impl fmt::Display for SelectedFleet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "selected fleet: index={}, id={}, system={}, state={}, hull={}",
            format_optional_i32(self.index),
            format_optional_i64(self.id),
            format_optional_i64(self.system_id),
            format_optional_i32(self.state),
            format_optional_str(self.hull_name.as_deref()),
        )
    }
}

/// Install fleet bar tracking and accessors.
pub(crate) fn install(api: &Il2CppApi) {
    if is_ready() {
        return;
    }

    if !TRACKER_INSTALLED.load(Relaxed) || method_missing(&GET_CONTEXT_METHOD) {
        if let Some(class) =
            resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "FleetBarViewController")
        {
            install_tracker_once(api, class);
            resolve_method_if_missing(api, class, "get_CanvasContext", 0, &GET_CONTEXT_METHOD, "Fleet bar context");
        } else {
            warn!(target: LOG_TARGET, "FleetBarViewController class not found");
        }
    }

    if method_missing(&GET_CURRENT_FLEET_METHOD) || method_missing(&GET_CURRENT_INDEX_METHOD) {
        if let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.HUD", "FleetBarContext") {
            resolve_method_if_missing(api, class, "get_CurrentFleet", 0, &GET_CURRENT_FLEET_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_CurrentIndex", 0, &GET_CURRENT_INDEX_METHOD, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "FleetBarContext class not found");
        }
    }

    if method_missing(&GET_FLEET_ID_METHOD)
        || method_missing(&GET_FLEET_STATE_METHOD)
        || method_missing(&GET_FLEET_HULL_METHOD)
        || method_missing(&GET_FLEET_ADDRESS_METHOD)
        || field_missing(&OFFSET_FLEET_LOCATION_DATA)
    {
        if let Some(class) = resolver::resolve_prime_model_class(api, "FleetPlayerData") {
            resolve_method_if_missing(api, class, "get_Id", 0, &GET_FLEET_ID_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_CurrentState", 0, &GET_FLEET_STATE_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_Hull", 0, &GET_FLEET_HULL_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_Address", 0, &GET_FLEET_ADDRESS_METHOD, "Selected fleet");
            resolve_field_if_missing(api, class, "LocationData", &OFFSET_FLEET_LOCATION_DATA, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "FleetPlayerData class not found");
        }
    }

    if method_missing(&GET_HULL_NAME_METHOD) {
        if let Some(class) = resolver::resolve_prime_model_class(api, "HullSpec") {
            resolve_method_if_missing(api, class, "get_Name", 0, &GET_HULL_NAME_METHOD, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "HullSpec class not found");
        }
    }

    if method_missing(&GET_NODE_ADDRESS_SYSTEM_METHOD) {
        if let Some(class) = resolver::resolve_prime_model_class(api, "NodeAddress") {
            resolve_method_if_missing(api, class, "get_System", 0, &GET_NODE_ADDRESS_SYSTEM_METHOD, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "NodeAddress class not found");
        }
    }
}

/// Fleet selected in the bottom fleet bar.
pub(crate) fn selected_fleet() -> Option<SelectedFleet> {
    let controller = fleet_bar::get();
    if controller.is_null() {
        return None;
    }

    let context = fleet_bar_context(controller)?;
    let index = invoke::i32(
        GET_CURRENT_INDEX_METHOD.load(Relaxed),
        context,
        "FleetBarContext.get_CurrentIndex",
    );
    let fleet = invoke::object(
        GET_CURRENT_FLEET_METHOD.load(Relaxed),
        context,
        "FleetBarContext.get_CurrentFleet",
    )?;

    Some(SelectedFleet {
        index,
        id: fleet_id(fleet),
        system_id: fleet_system_id(fleet),
        state: fleet_state(fleet),
        hull_name: fleet_hull_name(fleet),
        location_data: fleet_location_data(fleet),
    })
}

// ---- Fleet access ---------------------------------------------------------

fn fleet_bar_context(controller: *mut ()) -> Option<*mut Il2CppObject> {
    invoke::object(
        GET_CONTEXT_METHOD.load(Relaxed),
        controller as *mut Il2CppObject,
        "FleetBarViewController.get_CanvasContext",
    )
}

fn fleet_id(fleet: *mut Il2CppObject) -> Option<i64> {
    invoke::i64(GET_FLEET_ID_METHOD.load(Relaxed), fleet, "FleetPlayerData.get_Id")
}

fn fleet_state(fleet: *mut Il2CppObject) -> Option<i32> {
    invoke::i32(GET_FLEET_STATE_METHOD.load(Relaxed), fleet, "FleetPlayerData.get_CurrentState")
}

fn fleet_hull_name(fleet: *mut Il2CppObject) -> Option<String> {
    let hull = invoke::object(GET_FLEET_HULL_METHOD.load(Relaxed), fleet, "FleetPlayerData.get_Hull")?;
    invoke::string(GET_HULL_NAME_METHOD.load(Relaxed), hull, "HullSpec.get_Name")
}

fn fleet_system_id(fleet: *mut Il2CppObject) -> Option<i64> {
    let address = invoke::object(GET_FLEET_ADDRESS_METHOD.load(Relaxed), fleet, "FleetPlayerData.get_Address")?;
    valid_system_id(invoke::i64(
        GET_NODE_ADDRESS_SYSTEM_METHOD.load(Relaxed),
        address,
        "NodeAddress.get_System",
    ))
}

fn fleet_location_data(fleet: *mut Il2CppObject) -> Option<*mut Il2CppObject> {
    let offset = OFFSET_FLEET_LOCATION_DATA.load(Relaxed);
    if offset == 0 {
        return None;
    }

    let location_data = unsafe { tracker::read_ptr(fleet as *const (), offset) } as *mut Il2CppObject;
    (!location_data.is_null()).then_some(location_data)
}

// ---- Formatting -----------------------------------------------------------

fn format_optional_i32(value: Option<i32>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

fn format_optional_i64(value: Option<i64>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

fn format_optional_str(value: Option<&str>) -> String {
    value.unwrap_or("unknown").to_string()
}

fn valid_system_id(system_id: Option<i64>) -> Option<i64> {
    system_id.filter(|system_id| *system_id >= 0)
}

// ---- Installation state ---------------------------------------------------

fn is_ready() -> bool {
    TRACKER_INSTALLED.load(Relaxed)
        && !method_missing(&GET_CONTEXT_METHOD)
        && !method_missing(&GET_CURRENT_FLEET_METHOD)
        && !method_missing(&GET_CURRENT_INDEX_METHOD)
        && !method_missing(&GET_FLEET_ID_METHOD)
        && !method_missing(&GET_FLEET_STATE_METHOD)
        && !method_missing(&GET_FLEET_HULL_METHOD)
        && !method_missing(&GET_HULL_NAME_METHOD)
        && !method_missing(&GET_FLEET_ADDRESS_METHOD)
        && !method_missing(&GET_NODE_ADDRESS_SYSTEM_METHOD)
        && !field_missing(&OFFSET_FLEET_LOCATION_DATA)
}

fn install_tracker_once(api: &Il2CppApi, class: *mut Il2CppClass) {
    if !TRACKER_INSTALLED.load(Relaxed) && fleet_bar::install(api, class, LOG_TARGET) {
        TRACKER_INSTALLED.store(true, Relaxed);
    }
}

// ---- Resolution helpers ---------------------------------------------------

fn method_missing(target: &AtomicPtr<MethodInfo>) -> bool {
    target.load(Relaxed).is_null()
}

fn field_missing(target: &AtomicUsize) -> bool {
    target.load(Relaxed) == 0
}

fn resolve_method_if_missing(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    target: &AtomicPtr<MethodInfo>,
    feature: &str,
) {
    if !method_missing(target) {
        return;
    }

    if !resolver::resolve_method_into(api, class, method_name, param_count, target) {
        warn!(target: LOG_TARGET, "{feature} unavailable: method {method_name} not resolved");
    }
}

fn resolve_field_if_missing(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    field_name: &str,
    target: &AtomicUsize,
    feature: &str,
) {
    if !field_missing(target) {
        return;
    }

    if !resolver::resolve_field_offset_into(api, class, field_name, target) {
        warn!(target: LOG_TARGET, "{feature} unavailable: field {field_name} not resolved");
    }
}
