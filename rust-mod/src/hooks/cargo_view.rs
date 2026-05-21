//! Cargo auto-open hook.
//!
//! When a target viewer is shown, this hook optionally opens the cargo view normally revealed through the in-game
//! cargo button.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::{debug, warn};

use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- Dynamically resolved field offsets -----------------------------------

/// `PreScanTargetWidget._battleTargetData`.
static OFFSET_BATTLE_TARGET_DATA: AtomicUsize = AtomicUsize::new(0);

/// `PreScanTargetWidget._rewardsButtonWidget`.
static OFFSET_REWARDS_BUTTON_WIDGET: AtomicUsize = AtomicUsize::new(0);

/// `BattleTargetData.TargetFleetDeployedData`.
static OFFSET_TARGET_FLEET_DEPLOYED_DATA: AtomicUsize = AtomicUsize::new(0);

// ---- Dynamically resolved functions ---------------------------------------

/// Original trampoline for `PreScanTargetWidget.ShowWithFleet(FleetPlayerData)`.
static ORIG_SHOW_WITH_FLEET: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// `RewardsButtonWidget._rewardsController`.
static OFFSET_REWARDS_CONTROLLER: AtomicUsize = AtomicUsize::new(0);

/// Method info for `VisibilityController.Show(bool)`.
static VISIBILITY_SHOW_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `FleetDeployedData.get_FleetType()`.
static GET_FLEET_TYPE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `FleetDeployedData.get_Hull()`.
static GET_HULL_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Method info for `HullSpec.get_Type()`.
static GET_HULL_TYPE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

// ---- Game enum values ------------------------------------------------------

const DEPLOYED_FLEET_TYPE_PLAYER: i32 = 1;
const DEPLOYED_FLEET_TYPE_MARAUDER: i32 = 2;
const HULL_TYPE_ARMADA_TARGET: i32 = 5;

/// Assemblies to search for PrimeServer model classes.
const MODEL_ASSEMBLIES: &[&str] = &["Digit.Client.PrimeLib.Runtime", "Assembly-CSharp", "Assembly-CSharp-firstpass"];

// ---- Type aliases ----------------------------------------------------------

type ShowWithFleetFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CargoTargetKind {
    Station,
    Player,
    Hostile,
    Armada,
    Other,
}

// ---- Hook -----------------------------------------------------------------

extern "C" fn hook_show_with_fleet(this: *mut Il2CppObject, fleet: *mut Il2CppObject) {
    let orig = ORIG_SHOW_WITH_FLEET.load(Relaxed);
    if !orig.is_null() {
        let f: ShowWithFleetFn = unsafe { std::mem::transmute(orig) };
        unsafe { f(this, fleet) };
    }

    maybe_open_cargo(this);
}

fn maybe_open_cargo(prescan: *mut Il2CppObject) {
    if prescan.is_null() || !crate::settings::cargo_view_enabled() {
        return;
    }

    let Some(kind) = classify_target(prescan) else {
        return;
    };
    if !should_open_for(kind) {
        return;
    }

    let rewards_button_offset = OFFSET_REWARDS_BUTTON_WIDGET.load(Relaxed);
    let rewards_controller_offset = OFFSET_REWARDS_CONTROLLER.load(Relaxed);
    let show_ptr = VISIBILITY_SHOW_FN.load(Relaxed);
    if rewards_button_offset == 0 || rewards_controller_offset == 0 || show_ptr.is_null() {
        return;
    }

    let rewards_button = unsafe { tracker::read_ptr(prescan as *const (), rewards_button_offset) };
    if rewards_button.is_null() {
        return;
    }

    let rewards_controller = unsafe { tracker::read_ptr(rewards_button as *const (), rewards_controller_offset) };
    if rewards_controller.is_null() {
        return;
    }

    debug!(target: "CargoView", "Opening cargo view for {kind:?}");
    invoke::void_bool(
        show_ptr,
        rewards_controller as *mut Il2CppObject,
        true,
        "VisibilityController.Show",
    );
}

fn should_open_for(kind: CargoTargetKind) -> bool {
    match kind {
        CargoTargetKind::Station => crate::settings::show_cargo_for_stations(),
        CargoTargetKind::Player => crate::settings::show_cargo_for_players(),
        CargoTargetKind::Hostile => crate::settings::show_cargo_for_hostiles(),
        CargoTargetKind::Armada => crate::settings::show_cargo_for_armadas(),
        CargoTargetKind::Other => false,
    }
}

fn classify_target(prescan: *mut Il2CppObject) -> Option<CargoTargetKind> {
    let battle_target_offset = OFFSET_BATTLE_TARGET_DATA.load(Relaxed);
    let target_fleet_offset = OFFSET_TARGET_FLEET_DEPLOYED_DATA.load(Relaxed);
    if battle_target_offset == 0 || target_fleet_offset == 0 {
        return None;
    }

    let battle_target = unsafe { tracker::read_ptr(prescan as *const (), battle_target_offset) };
    if battle_target.is_null() {
        return None;
    }

    let target_fleet = unsafe { tracker::read_ptr(battle_target as *const (), target_fleet_offset) };
    if target_fleet.is_null() {
        return Some(CargoTargetKind::Station);
    }

    match get_fleet_type(target_fleet as *mut Il2CppObject)? {
        DEPLOYED_FLEET_TYPE_PLAYER => Some(CargoTargetKind::Player),
        DEPLOYED_FLEET_TYPE_MARAUDER => {
            if get_hull_type(target_fleet as *mut Il2CppObject) == Some(HULL_TYPE_ARMADA_TARGET) {
                Some(CargoTargetKind::Armada)
            } else {
                Some(CargoTargetKind::Hostile)
            }
        }
        _ => Some(CargoTargetKind::Other),
    }
}

fn get_fleet_type(fleet: *mut Il2CppObject) -> Option<i32> {
    invoke::i32(GET_FLEET_TYPE_FN.load(Relaxed), fleet, "FleetDeployedData.get_FleetType")
}

fn get_hull_type(fleet: *mut Il2CppObject) -> Option<i32> {
    let hull = invoke::object(GET_HULL_FN.load(Relaxed), fleet, "FleetDeployedData.get_Hull")?;
    invoke::i32(GET_HULL_TYPE_FN.load(Relaxed), hull, "HullSpec.get_Type")
}

// ---- Installation ---------------------------------------------------------

/// Install cargo auto-open hooks.
pub fn install(api: &Il2CppApi) {
    let Some(prescan_class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "PreScanTargetWidget")
    else {
        warn!(target: "CargoView", "PreScanTargetWidget class not found");
        return;
    };

    resolve_offset(api, prescan_class, "_battleTargetData", &OFFSET_BATTLE_TARGET_DATA);
    resolve_offset(api, prescan_class, "_rewardsButtonWidget", &OFFSET_REWARDS_BUTTON_WIDGET);

    tracker::install_resolved_hook(
        api,
        prescan_class,
        "ShowWithFleet",
        1,
        "CargoViewShowWithFleet",
        hook_show_with_fleet as *const (),
        |orig| ORIG_SHOW_WITH_FLEET.store(orig as *mut (), Relaxed),
    );

    if let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "RewardsButtonWidget") {
        resolve_offset(api, class, "_rewardsController", &OFFSET_REWARDS_CONTROLLER);
    } else {
        warn!(target: "CargoView", "RewardsButtonWidget class not found");
    }

    if let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Client.UI", "VisibilityController") {
        resolve_fn(api, class, "Show", 1, &VISIBILITY_SHOW_FN);
    } else {
        warn!(target: "CargoView", "VisibilityController class not found");
    }

    if let Some(class) = resolve_model_class(api, "BattleTargetData") {
        resolve_offset(api, class, "TargetFleetDeployedData", &OFFSET_TARGET_FLEET_DEPLOYED_DATA);
    } else {
        warn!(target: "CargoView", "BattleTargetData class not found");
    }

    if let Some(class) = resolve_model_class(api, "FleetDeployedData") {
        resolve_fn(api, class, "get_FleetType", 0, &GET_FLEET_TYPE_FN);
        resolve_fn(api, class, "get_Hull", 0, &GET_HULL_FN);
    } else {
        warn!(target: "CargoView", "FleetDeployedData class not found");
    }

    if let Some(class) = resolve_model_class(api, "HullSpec") {
        resolve_fn(api, class, "get_Type", 0, &GET_HULL_TYPE_FN);
    } else {
        warn!(target: "CargoView", "HullSpec class not found");
    }
}

fn resolve_model_class(api: &Il2CppApi, class_name: &str) -> Option<*mut Il2CppClass> {
    for assembly in MODEL_ASSEMBLIES {
        if let Some(class) = resolver::resolve_class(api, assembly, "Digit.PrimeServer.Models", class_name) {
            debug!(target: "CargoView", "{class_name} class found in assembly '{assembly}'");
            return Some(class);
        }
    }
    None
}

fn resolve_offset(api: &Il2CppApi, class: *mut Il2CppClass, field_name: &str, target: &AtomicUsize) {
    resolver::resolve_field_offset_into(api, class, field_name, target);
}

fn resolve_fn(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    param_count: i32,
    target: &AtomicPtr<MethodInfo>,
) {
    resolver::resolve_method_into(api, class, method_name, param_count, target);
}
