//! Extend selected in-game slider maxima without changing their game-provided defaults.
//!
//! The adjustment runs when `InventoryUseRowWidget` binds its concrete `InventoryForPopup`. It updates the
//! `_maxItemsToUse` field before the game binds the slider, avoiding platform-dependent inlining of the property's
//! setter and the internal maximum calculation. Alliance donations are identified by `IsDonationUse`; Standard
//! Recruit purchases use their stable bundle ID. Configured values only increase the maximum supplied by the game.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering::Relaxed};

use log::debug;

use crate::hook::safety::HookInfo;
use crate::hooks::tracker;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::compatibility;
use crate::il2cpp::compatibility_manifest as manifest;
use crate::il2cpp::invoke;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

const LOG_TARGET: &str = "SliderLimits";

/// Standard Recruit bundle ID, verified through an in-game debug session on v1851.
const STANDARD_RECRUIT_BUNDLE_ID: i64 = 145_512_548;
const STANDARD_RECRUIT_MAX: u32 = 150;

// ---- State ----------------------------------------------------------------

/// Original function pointer for `InventoryUseRowWidget.OnDidBindContext()`.
static ORIGINAL_ON_DID_BIND_CONTEXT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

static OFFSET_WIDGET_CONTEXT: AtomicUsize = AtomicUsize::new(0);
static OFFSET_MAX_ITEMS_TO_USE: AtomicUsize = AtomicUsize::new(0);
static OFFSET_ACTION_TARGET: AtomicUsize = AtomicUsize::new(0);
static OFFSET_IS_DONATION_USE: AtomicUsize = AtomicUsize::new(0);
static OFFSET_IS_CHEST_PURCHASE: AtomicUsize = AtomicUsize::new(0);

static GET_BUNDLE_ID_FN: AtomicPtr<MethodInfo> = AtomicPtr::new(std::ptr::null_mut());
static BUNDLE_CLASS: AtomicPtr<Il2CppClass> = AtomicPtr::new(std::ptr::null_mut());

static HOOK_INFO: HookInfo = HookInfo::new("SliderLimits");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SliderKind {
    AllianceDonation,
    StandardRecruit,
}

// ---- Hook -----------------------------------------------------------------

type OnDidBindContextFn = unsafe extern "C" fn(*mut Il2CppObject);

extern "C" fn hook_on_did_bind_context(this: *mut Il2CppObject) {
    HOOK_INFO.run(|| extend_bound_slider(this));

    let original_ptr = ORIGINAL_ON_DID_BIND_CONTEXT.load(Relaxed);
    if !original_ptr.is_null() {
        let original: OnDidBindContextFn = unsafe { std::mem::transmute(original_ptr) };
        unsafe { original(this) };
    }
}

fn extend_bound_slider(widget: *mut Il2CppObject) {
    let Some(api) = crate::hooks::il2cpp_init::IL2CPP_API.get() else {
        return;
    };
    let Some(inventory_item) = read_object_field(widget, &OFFSET_WIDGET_CONTEXT) else {
        return;
    };
    let Some(game_maximum) = read_i64_field(inventory_item, &OFFSET_MAX_ITEMS_TO_USE) else {
        return;
    };

    let slider_kind = classify_inventory_item(api, inventory_item);
    let settings = crate::settings::slider_limits();
    let (label, configured) = match slider_kind {
        Some(SliderKind::AllianceDonation) => ("Alliance Donation", settings.alliance_donation_max),
        Some(SliderKind::StandardRecruit) => (
            "Standard Recruit",
            settings.standard_recruit_max.map(|maximum| maximum.min(STANDARD_RECRUIT_MAX)),
        ),
        None => return,
    };

    let maximum = extend_maximum(game_maximum, configured);
    if maximum <= game_maximum {
        return;
    }

    if write_i64_field(inventory_item, &OFFSET_MAX_ITEMS_TO_USE, maximum) {
        debug!(target: LOG_TARGET, "Extending {label} slider maximum: {game_maximum} -> {maximum}");
    }
}

fn classify_inventory_item(api: &Il2CppApi, inventory_item: *mut Il2CppObject) -> Option<SliderKind> {
    if read_bool_field(inventory_item, &OFFSET_IS_DONATION_USE).unwrap_or(false) {
        return Some(SliderKind::AllianceDonation);
    }
    if !read_bool_field(inventory_item, &OFFSET_IS_CHEST_PURCHASE).unwrap_or(false) {
        return None;
    }

    let bundle = read_object_field(inventory_item, &OFFSET_ACTION_TARGET)?;
    let bundle_class = BUNDLE_CLASS.load(Relaxed);
    let actual_class = unsafe { (api.object_get_class)(bundle) };
    if !is_exact_class(actual_class, bundle_class) {
        debug!(target: LOG_TARGET, "Ignoring chest slider with non-Bundle action target");
        return None;
    }

    let bundle_id = invoke::i64(GET_BUNDLE_ID_FN.load(Relaxed), bundle, "Bundle.get_BundleId");

    classify_chest_slider(bundle_id)
}

fn classify_chest_slider(bundle_id: Option<i64>) -> Option<SliderKind> {
    (bundle_id == Some(STANDARD_RECRUIT_BUNDLE_ID)).then_some(SliderKind::StandardRecruit)
}

fn is_exact_class(actual: *mut Il2CppClass, expected: *mut Il2CppClass) -> bool {
    !expected.is_null() && actual == expected
}

fn extend_maximum(game_maximum: i64, configured: Option<u32>) -> i64 {
    configured.map_or(game_maximum, |maximum| game_maximum.max(i64::from(maximum)))
}

fn read_object_field(base: *mut Il2CppObject, offset: &AtomicUsize) -> Option<*mut Il2CppObject> {
    let offset = offset.load(Relaxed);
    if base.is_null() || offset == 0 {
        return None;
    }

    let value = unsafe { tracker::read_ptr(base.cast(), offset) }.cast::<Il2CppObject>();
    (!value.is_null()).then_some(value)
}

fn read_bool_field(base: *mut Il2CppObject, offset: &AtomicUsize) -> Option<bool> {
    let offset = offset.load(Relaxed);
    if base.is_null() || offset == 0 {
        return None;
    }

    Some(unsafe { *((base as *const u8).add(offset) as *const bool) })
}

fn read_i64_field(base: *mut Il2CppObject, offset: &AtomicUsize) -> Option<i64> {
    let offset = offset.load(Relaxed);
    if base.is_null() || offset == 0 {
        return None;
    }

    Some(unsafe { *((base as *const u8).add(offset) as *const i64) })
}

fn write_i64_field(base: *mut Il2CppObject, offset: &AtomicUsize, value: i64) -> bool {
    let offset = offset.load(Relaxed);
    if base.is_null() || offset == 0 {
        return false;
    }

    unsafe { *((base as *mut u8).add(offset) as *mut i64) = value };
    true
}

// ---- Installation ---------------------------------------------------------

/// Install the independently idempotent slider-limit hook.
pub fn install(api: &Il2CppApi) {
    if !compatibility::is_enabled(manifest::SLIDER_LIMITS) || !ORIGINAL_ON_DID_BIND_CONTEXT.load(Relaxed).is_null() {
        return;
    }

    let Some(widget_class) = resolver::resolve_prime_class(api, "Digit.Client.UI", "Widget") else {
        return;
    };
    if !resolver::resolve_field_offset_into(api, widget_class, "m_untypedContext", &OFFSET_WIDGET_CONTEXT) {
        return;
    }

    let Some(inventory_item_class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Inventories", "InventoryForPopup")
    else {
        return;
    };
    if !resolve_inventory_item_fields(api, inventory_item_class) {
        return;
    }

    resolve_identifier_methods(api);

    let Some(row_class) =
        resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Inventories", "InventoryUseRowWidget")
    else {
        return;
    };
    tracker::install_resolved_hook_if_missing(
        api,
        row_class,
        "OnDidBindContext",
        0,
        "SliderLimits.BindInventoryRow",
        hook_on_did_bind_context as *const (),
        &ORIGINAL_ON_DID_BIND_CONTEXT,
    );
}

fn resolve_inventory_item_fields(api: &Il2CppApi, class: *mut Il2CppClass) -> bool {
    resolver::resolve_field_offset_into(api, class, "_maxItemsToUse", &OFFSET_MAX_ITEMS_TO_USE)
        && resolver::resolve_field_offset_into(api, class, "<IsDonationUse>k__BackingField", &OFFSET_IS_DONATION_USE)
        && resolver::resolve_field_offset_into(
            api,
            class,
            "<IsChestPurchase>k__BackingField",
            &OFFSET_IS_CHEST_PURCHASE,
        )
        && resolver::resolve_field_offset_into(api, class, "<ActionTarget>k__BackingField", &OFFSET_ACTION_TARGET)
}

fn resolve_identifier_methods(api: &Il2CppApi) {
    if let Some(bundle_class) = resolver::resolve_prime_class(api, "Digit.PrimePlatform.Content", "Bundle") {
        BUNDLE_CLASS.store(bundle_class, Relaxed);
        try_resolve_method_into(api, bundle_class, "get_BundleId", &GET_BUNDLE_ID_FN);
    }
}

fn try_resolve_method_into(
    api: &Il2CppApi,
    class: *mut Il2CppClass,
    method_name: &str,
    target: &AtomicPtr<MethodInfo>,
) -> bool {
    if !target.load(Relaxed).is_null() {
        return true;
    }

    let Some(method) = resolver::try_resolve_method(api, class, method_name, 0) else {
        return false;
    };
    target.store(method as *mut MethodInfo, Relaxed);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_maximum_only_extends_game_value() {
        assert_eq!(extend_maximum(50, None), 50);
        assert_eq!(extend_maximum(50, Some(40)), 50);
        assert_eq!(extend_maximum(50, Some(500)), 500);
        assert_eq!(extend_maximum(100, Some(50)), 100);
    }

    #[test]
    fn standard_recruit_requires_matching_bundle() {
        assert_eq!(
            classify_chest_slider(Some(STANDARD_RECRUIT_BUNDLE_ID)),
            Some(SliderKind::StandardRecruit)
        );
        assert_eq!(classify_chest_slider(Some(475_431_388)), None);
        assert_eq!(classify_chest_slider(None), None);
    }

    #[test]
    fn exact_class_guard_rejects_null_and_foreign_classes() {
        let mut actual = 0_u8;
        let mut foreign = 0_u8;
        let actual = (&mut actual as *mut u8).cast::<Il2CppClass>();
        let foreign = (&mut foreign as *mut u8).cast::<Il2CppClass>();

        assert!(is_exact_class(actual, actual));
        assert!(!is_exact_class(actual, foreign));
        assert!(!is_exact_class(actual, std::ptr::null_mut()));
        assert!(!is_exact_class(std::ptr::null_mut(), actual));
    }

    #[test]
    fn field_helpers_ignore_null_and_unresolved_offsets() {
        let offset = AtomicUsize::new(0);
        assert_eq!(read_object_field(std::ptr::null_mut(), &offset), None);
        assert_eq!(read_bool_field(std::ptr::null_mut(), &offset), None);
        assert_eq!(read_i64_field(std::ptr::null_mut(), &offset), None);
        assert!(!write_i64_field(std::ptr::null_mut(), &offset, 500));
    }
}
