//! System view zoom extension.
//!
//! Hooks `NavigationZoom.SetDepth()` to extend the zoom-out limit when entering a system.
//! Uses the game's own `OverrideZoomLimits()` API to push the maximum beyond the default radius.
//!
//! `SetViewParameters` delegates to `SetDepth` on all platforms, making `SetDepth` the more robust hook target.
//! On Windows, MSVC additionally inlines `SetViewParameters` at all call sites (verified: zero CALL references in
//! GameAssembly.dll), but the inlined copies are still called `SetDepth`.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering::Relaxed};

use log::{debug, warn};

use crate::hook::safety::HookInfo;
use crate::hooks::navigation_view;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

// ---- NodeDepth enum values ------------------------------------------------

/// `NodeDepth.SolarSystem` — the only depth we care about for zoom overrides.
const NODE_DEPTH_SOLAR_SYSTEM: i32 = 2;

/// Minimum value for smax so the user can always zoom out far enough.
const SMAX_FLOOR: f32 = 3000.0;

// ---- Dynamically resolved field offsets -----------------------------------

static OFFSET_MINIMUM: AtomicUsize = AtomicUsize::new(0);
static OFFSET_MIDDLE: AtomicUsize = AtomicUsize::new(0);
static OFFSET_MAXIMUM: AtomicUsize = AtomicUsize::new(0);
static OFFSET_VIEW_RADIUS: AtomicUsize = AtomicUsize::new(0);
static OFFSET_DEPTH: AtomicUsize = AtomicUsize::new(0);
static OFFSET_ACTUAL_DISTANCE: AtomicUsize = AtomicUsize::new(0);
static OFFSET_FAR_RATIO_NORMAL: AtomicUsize = AtomicUsize::new(0);
static OFFSET_FAR_RATIO_EXTENDED: AtomicUsize = AtomicUsize::new(0);
static OFFSET_DEFAULT_ZOOM_RATIO: AtomicUsize = AtomicUsize::new(0);

// ---- State ----------------------------------------------------------------

/// Original function pointer for `NavigationZoom.SetDepth(NodeDepth)`.
static ORIG_SET_DEPTH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `NavigationZoom.OverrideZoomLimits(float, float)`.
static OVERRIDE_ZOOM_LIMITS_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Resolved method info for `NavigationZoom.set_Distance(float)`.
static SET_DISTANCE_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());

/// Cached NavigationZoom instance for live settings updates.
/// FIXME: This cached Unity object can become stale after navigation lifecycle changes. Prefer lifecycle clearing
/// or a current-instance lookup before live settings updates if the dump exposes one.
static CACHED_NAV_ZOOM: AtomicPtr<Il2CppObject> = AtomicPtr::new(std::ptr::null_mut());

/// Latest system zoom setting, updated from the main-thread settings executor.
static CURRENT_SYSTEM_ZOOM: AtomicU32 = AtomicU32::new(1000);

/// Latest ship names visibility setting, updated from the main-thread settings executor.
static CURRENT_SHIP_NAMES_VISIBLE: AtomicU32 = AtomicU32::new(1800);

/// Per-hook error tracking and deactivation state.
static HOOK_INFO: HookInfo = HookInfo::new("SystemZoom");

/// Whether the first diagnostic log has been emitted.
static LOGGED_FIRST: AtomicBool = AtomicBool::new(false);

// ---- Type aliases ---------------------------------------------------------

type SetDepthFn = unsafe extern "C" fn(*mut Il2CppObject, i32);

// ---- Diagnostic helpers ---------------------------------------------------

/// Read a named float field, returning 0.0 if the offset is unresolved.
fn read_field(this: *mut Il2CppObject, offset: &AtomicUsize) -> f32 {
    let off = offset.load(Relaxed);
    if off == 0 {
        return 0.0;
    }
    unsafe { tracker::read_f32(this as *const (), off) }
}

/// Read the _depth field (i32 enum).
fn read_depth(this: *mut Il2CppObject) -> i32 {
    let off = OFFSET_DEPTH.load(Relaxed);
    if off == 0 {
        return -1;
    }
    unsafe { tracker::read_i32(this as *const (), off) }
}

/// Map NodeDepth enum value to a human-readable name.
fn depth_name(depth: i32) -> &'static str {
    match depth {
        1 => "Galaxy",
        2 => "SolarSystem",
        4 => "PlanetSystem",
        8 => "Starbase",
        _ => "Unknown",
    }
}

/// Log the current zoom state of a NavigationZoom instance.
fn log_zoom_state(this: *mut Il2CppObject, context: &str) {
    // FIXME: These diagnostic reads are direct field reads. If this hook becomes unstable after game updates,
    // prefer getters/properties from the dump or remove nonessential diagnostics.
    let depth = read_depth(this);
    let minimum = read_field(this, &OFFSET_MINIMUM);
    let middle = read_field(this, &OFFSET_MIDDLE);
    let maximum = read_field(this, &OFFSET_MAXIMUM);
    let distance = read_field(this, &OFFSET_ACTUAL_DISTANCE);
    let view_radius = read_field(this, &OFFSET_VIEW_RADIUS);
    let far_normal = read_field(this, &OFFSET_FAR_RATIO_NORMAL);
    let far_extended = read_field(this, &OFFSET_FAR_RATIO_EXTENDED);
    let default_ratio = read_field(this, &OFFSET_DEFAULT_ZOOM_RATIO);

    debug!(
        target: "SystemZoom",
        "{context} depth={} min={minimum:.1} mid={middle:.1} max={maximum:.1} \
         dist={distance:.1} radius={view_radius:.1} \
         farNormal={far_normal:.3} farExtended={far_extended:.3} \
         defaultRatio={default_ratio:.3}",
        depth_name(depth),
    );
}

// ---- Zoom application -----------------------------------------------------

/// Calculate the internal zoom limit (smax) from the ship names visibility setting.
///
/// Uses the empirically determined formula: `smax = (visibility - 200) / 0.4`.
/// The result is clamped to at least [`SMAX_FLOOR`], so the game's zoom range is always usable.
fn calculate_smax(visibility: u32) -> f32 {
    let visibility = visibility as f32;
    let smax = (visibility - 200.0) / 0.4;
    smax.max(SMAX_FLOOR)
}

/// Extend the zoom-out limit on a NavigationZoom instance.
///
/// Sets the internal maximum high enough for ship name rendering.
fn apply_zoom_limit(this: *mut Il2CppObject, ship_names_visible: u32) {
    let override_ptr = OVERRIDE_ZOOM_LIMITS_FN.load(Relaxed);
    let min_offset = OFFSET_MINIMUM.load(Relaxed);
    if override_ptr.is_null() || this.is_null() || min_offset == 0 {
        return;
    }
    let minimum = read_field(this, &OFFSET_MINIMUM);
    let smax = calculate_smax(ship_names_visible);
    invoke::void_f32_f32(override_ptr, this, minimum, smax, "NavigationZoom.OverrideZoomLimits");
}

/// Set the camera distance on a NavigationZoom instance via the game's property setter.
fn apply_distance(this: *mut Il2CppObject, distance: f32) {
    let ptr = SET_DISTANCE_FN.load(Relaxed);
    if ptr.is_null() || this.is_null() {
        return;
    }
    invoke::void_f32(ptr, this, distance, "NavigationZoom.set_Distance");
}

pub(crate) fn on_settings_changed_value(system_zoom: u32, ship_names_visible: u32) {
    CURRENT_SYSTEM_ZOOM.store(system_zoom, Relaxed);
    CURRENT_SHIP_NAMES_VISIBLE.store(ship_names_visible, Relaxed);

    HOOK_INFO.run(|| apply_settings_update(system_zoom, ship_names_visible));
}

fn apply_settings_update(system_zoom: u32, ship_names_visible: u32) {
    let this = CACHED_NAV_ZOOM.load(Relaxed);
    if this.is_null() {
        return;
    }

    // The shared navigation view state is the source of truth for live system zoom updates.
    if !navigation_view::is_viewing_system() {
        return;
    }

    // Keep the cached NavigationZoom instance honest; it can become stale across navigation lifecycle changes.
    let depth = read_depth(this);
    if depth != NODE_DEPTH_SOLAR_SYSTEM {
        return;
    }

    apply_zoom_limit(this, ship_names_visible);
    let zoom = system_zoom as f32;
    apply_distance(this, zoom);
    let smax = calculate_smax(ship_names_visible);
    debug!(target: "SystemZoom", "Live update: distance={zoom:.0}, smax={smax:.0}");
}

// ---- Hook -----------------------------------------------------------------

/// Hook for `NavigationZoom.SetDepth(NodeDepth value)`.
///
/// Called when the navigation depth changes (system, galaxy, planet, starbase).
/// The inlined callers of `SetViewParameters` call `SetDepth` after setting up the view parameters, so this fires
/// reliably on all platforms.
extern "C" fn hook_set_depth(this: *mut Il2CppObject, depth: i32) {
    // Always call the original first.
    let orig_ptr = ORIG_SET_DEPTH.load(Relaxed);
    if !orig_ptr.is_null() {
        let original: SetDepthFn = unsafe { std::mem::transmute(orig_ptr) };
        unsafe { original(this, depth) };
    }

    HOOK_INFO.run(|| {
        let is_first = !LOGGED_FIRST.swap(true, Relaxed);
        let is_system = depth == NODE_DEPTH_SOLAR_SYSTEM;

        if is_first || is_system {
            debug!(
                target: "SystemZoom",
                "SetDepth(depth={})",
                depth_name(depth),
            );
            log_zoom_state(this, "  Before override:");
        }

        // Cache the instance, extend zoom limit, and set initial distance.
        if is_system {
            CACHED_NAV_ZOOM.store(this, Relaxed);
            let ship_names_visible = CURRENT_SHIP_NAMES_VISIBLE.load(Relaxed);
            apply_zoom_limit(this, ship_names_visible);
            let zoom = CURRENT_SYSTEM_ZOOM.load(Relaxed) as f32;
            apply_distance(this, zoom);
            if is_first {
                log_zoom_state(this, "  After override:");
            }
        }
    });
}

// ---- Field resolution helper ----------------------------------------------

/// Resolve a field offset and store it.
fn resolve_field(api: &Il2CppApi, class: *mut Il2CppClass, field_name: &str, target: &AtomicUsize) {
    resolver::resolve_field_offset_into(api, class, field_name, target);
}

// ---- Installation ---------------------------------------------------------

/// Install system zoom hooks.
///
/// Hooks `SetDepth` to extend the zoom-out limit for system views and resolves `OverrideZoomLimits` as a callable
/// method.
pub fn install(api: &Il2CppApi) {
    let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Navigation", "NavigationZoom")
    else {
        warn!(target: "SystemZoom", "NavigationZoom class not found");
        return;
    };

    // Resolve all field offsets for diagnostic logging.
    resolve_field(api, class, "_minimum", &OFFSET_MINIMUM);
    resolve_field(api, class, "_middle", &OFFSET_MIDDLE);
    resolve_field(api, class, "_maximum", &OFFSET_MAXIMUM);
    resolve_field(api, class, "_viewRadius", &OFFSET_VIEW_RADIUS);
    resolve_field(api, class, "_depth", &OFFSET_DEPTH);
    resolve_field(api, class, "_actualDistance", &OFFSET_ACTUAL_DISTANCE);
    resolve_field(api, class, "_farRatioSystemNormal", &OFFSET_FAR_RATIO_NORMAL);
    resolve_field(api, class, "_farRatioSystemExtended", &OFFSET_FAR_RATIO_EXTENDED);
    resolve_field(api, class, "_systemDefaultZoomRatio", &OFFSET_DEFAULT_ZOOM_RATIO);

    // Hook SetDepth (called when the navigation depth changes).
    // SetViewParameters is inlined by MSVC on Windows, but its inlined copies are still called SetDepth.
    tracker::install_resolved_hook(
        api,
        class,
        "SetDepth",
        1,
        "SystemZoom.SetDepth",
        hook_set_depth as *const (),
        |orig| ORIG_SET_DEPTH.store(orig as *mut (), Relaxed),
    );

    // Resolve OverrideZoomLimits (called from the SetDepth hook).
    resolver::resolve_method_into(api, class, "OverrideZoomLimits", 2, &OVERRIDE_ZOOM_LIMITS_FN);

    // Resolve set_Distance (called for initial zoom and live slider updates, not hooked).
    resolver::resolve_method_into(api, class, "set_Distance", 1, &SET_DISTANCE_FN);
}
