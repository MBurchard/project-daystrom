//! Fleet bar state access.
//!
//! Tracks `FleetBarViewController` and exposes the currently selected fleet from `FleetBarContext`.

use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering::Relaxed};

use log::warn;

use crate::hooks::tracker::instance_tracker;
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

/// MethodInfo for `HullSpec.get_Name()`.
static GET_HULL_NAME_METHOD: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

// ---- Public API -----------------------------------------------------------

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
    {
        if let Some(class) = resolve_model_class(api, "FleetPlayerData") {
            resolve_method_if_missing(api, class, "get_Id", 0, &GET_FLEET_ID_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_CurrentState", 0, &GET_FLEET_STATE_METHOD, "Selected fleet");
            resolve_method_if_missing(api, class, "get_Hull", 0, &GET_FLEET_HULL_METHOD, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "FleetPlayerData class not found");
        }
    }

    if method_missing(&GET_HULL_NAME_METHOD) {
        if let Some(class) = resolve_model_class(api, "HullSpec") {
            resolve_method_if_missing(api, class, "get_Name", 0, &GET_HULL_NAME_METHOD, "Selected fleet");
        } else {
            warn!(target: LOG_TARGET, "HullSpec class not found");
        }
    }
}

/// Human-readable snapshot of the fleet selected in the bottom fleet bar.
pub(crate) fn describe_selected_fleet() -> String {
    let controller = fleet_bar::get();
    if controller.is_null() {
        return "selected fleet unavailable: fleet bar controller unavailable".to_string();
    }

    let Some(context) = fleet_bar_context(controller) else {
        return "selected fleet unavailable: fleet bar context unavailable".to_string();
    };

    let index = invoke::i32(
        GET_CURRENT_INDEX_METHOD.load(Relaxed),
        context,
        "FleetBarContext.get_CurrentIndex",
    );

    let Some(fleet) = invoke::object(
        GET_CURRENT_FLEET_METHOD.load(Relaxed),
        context,
        "FleetBarContext.get_CurrentFleet",
    ) else {
        return format!(
            "selected fleet unavailable: no current fleet, index={}",
            format_optional_i32(index)
        );
    };

    format!(
        "selected fleet: index={}, id={}, state={}, hull={}",
        format_optional_i32(index),
        format_optional_i64(fleet_id(fleet)),
        format_optional_i32(fleet_state(fleet)),
        format_optional_str(fleet_hull_name(fleet).as_deref()),
    )
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
}

fn install_tracker_once(api: &Il2CppApi, class: *mut Il2CppClass) {
    if !TRACKER_INSTALLED.load(Relaxed) && fleet_bar::install(api, class, LOG_TARGET) {
        TRACKER_INSTALLED.store(true, Relaxed);
    }
}

// ---- Resolution helpers ---------------------------------------------------

/// Resolve model classes across assemblies used by different game builds.
fn resolve_model_class(api: &Il2CppApi, class_name: &str) -> Option<*mut Il2CppClass> {
    for assembly in ["Digit.Client.PrimeLib.Runtime", "Assembly-CSharp", "Assembly-CSharp-firstpass"] {
        if let Some(class) = resolver::resolve_class(api, assembly, "Digit.PrimeServer.Models", class_name) {
            return Some(class);
        }
    }
    None
}

fn method_missing(target: &AtomicPtr<MethodInfo>) -> bool {
    target.load(Relaxed).is_null()
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
