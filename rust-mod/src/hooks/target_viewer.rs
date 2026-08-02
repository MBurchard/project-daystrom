//! Shared observation of the PreScan target viewer.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering::Relaxed};
use std::time::Instant;

use log::{trace, warn};

use crate::hook::safety::HookInfo;
use crate::il2cpp::api::Il2CppApi;
use crate::il2cpp::resolver;
use crate::il2cpp::types::*;

use super::tracker;

const LOG_TARGET: &str = "TargetViewer";

/// Original trampoline for `PreScanTargetWidget.ShowWithFleet(FleetPlayerData)`.
static ORIG_SHOW_WITH_FLEET: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Original trampoline for `BattleTargetData.get_TargetID()`.
static ORIG_GET_TARGET_ID: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// Last target emitted for the current viewer display.
static LAST_TARGET_ID: AtomicI64 = AtomicI64::new(0);
static LAST_TARGET_LOG_MS: AtomicI64 = AtomicI64::new(0);
static TARGET_LOG_TIMER_START: OnceLock<Instant> = OnceLock::new();

static HOOK_INFO: HookInfo = HookInfo::new(LOG_TARGET);

const TARGET_LOG_DEDUPE_MS: i64 = 500;

type ShowWithFleetFn = unsafe extern "C" fn(*mut Il2CppObject, *mut Il2CppObject);
type GetTargetIdFn = unsafe extern "C" fn(*mut Il2CppObject) -> i64;
type TargetViewerCallback = fn(TargetViewerEvent);

#[derive(Clone, Copy)]
struct TargetViewerSubscriber {
    id: usize,
    callback: TargetViewerCallback,
}

static SHOW_WITH_FLEET_SUBSCRIBERS: Mutex<Vec<TargetViewerSubscriber>> = Mutex::new(Vec::new());
static TARGET_ID_SUBSCRIBERS: Mutex<Vec<TargetViewerSubscriber>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetViewerEvent {
    pub(crate) prescan: *mut Il2CppObject,
    pub(crate) fleet_id: Option<i64>,
}

pub(crate) fn subscribe_show_with_fleet(callback: TargetViewerCallback) {
    subscribe(&SHOW_WITH_FLEET_SUBSCRIBERS, callback, "ShowWithFleet");
}

pub(crate) fn subscribe_target_id(callback: TargetViewerCallback) {
    subscribe(&TARGET_ID_SUBSCRIBERS, callback, "TargetID");
}

/// Install the shared PreScan target viewer hook.
pub(crate) fn install(api: &Il2CppApi) {
    if ORIG_SHOW_WITH_FLEET.load(Relaxed).is_null() {
        install_show_with_fleet_accessors(api);
    }
    if ORIG_GET_TARGET_ID.load(Relaxed).is_null() {
        install_target_accessors(api);
    }
}

fn install_show_with_fleet_accessors(api: &Il2CppApi) {
    if let Some(class) = resolver::resolve_class(api, "Assembly-CSharp", "Digit.Prime.Combat", "PreScanTargetWidget") {
        install_show_with_fleet_hook(api, class);
    } else {
        warn!(target: LOG_TARGET, "PreScanTargetWidget class not found");
    }
}

fn install_show_with_fleet_hook(api: &Il2CppApi, class: *mut Il2CppClass) {
    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "ShowWithFleet",
        1,
        "TargetViewer.ShowWithFleet",
        hook_show_with_fleet as *const (),
        &ORIG_SHOW_WITH_FLEET,
    );
}

fn install_target_accessors(api: &Il2CppApi) {
    if let Some(class) = resolver::resolve_prime_model_class(api, "BattleTargetData") {
        install_target_id_hook(api, class);
    } else {
        warn!(target: LOG_TARGET, "BattleTargetData class not found");
    }
}

fn install_target_id_hook(api: &Il2CppApi, class: *mut Il2CppClass) {
    tracker::install_resolved_hook_if_missing(
        api,
        class,
        "get_TargetID",
        0,
        "TargetViewer.BattleTargetData.get_TargetID",
        hook_get_target_id as *const (),
        &ORIG_GET_TARGET_ID,
    );
}

extern "C" fn hook_show_with_fleet(this: *mut Il2CppObject, fleet: *mut Il2CppObject) {
    let orig = ORIG_SHOW_WITH_FLEET.load(Relaxed);
    if !orig.is_null() {
        let original: ShowWithFleetFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(this, fleet) };
    }

    HOOK_INFO.run(|| {
        let event = TargetViewerEvent { prescan: this, fleet_id: None };
        trace!(target: LOG_TARGET, "Target viewer shown");
        emit(&SHOW_WITH_FLEET_SUBSCRIBERS, event);
    });
}

extern "C" fn hook_get_target_id(this: *mut Il2CppObject) -> i64 {
    let orig = ORIG_GET_TARGET_ID.load(Relaxed);
    let target_id = if orig.is_null() {
        0
    } else {
        let original: GetTargetIdFn = unsafe { std::mem::transmute(orig) };
        unsafe { original(this) }
    };

    if target_id > 0 && should_emit_target(target_id) {
        HOOK_INFO.run(|| {
            let event = TargetViewerEvent {
                prescan: std::ptr::null_mut(),
                fleet_id: Some(target_id),
            };
            trace!(target: LOG_TARGET, "Target ID read: fleet_id={target_id}");
            emit(&TARGET_ID_SUBSCRIBERS, event);
        });
    }

    target_id
}

fn subscribe(target: &Mutex<Vec<TargetViewerSubscriber>>, callback: TargetViewerCallback, label: &str) {
    let mut subscribers = target.lock().unwrap_or_else(|error| error.into_inner());
    let id = callback as usize;
    if subscribers.iter().any(|subscriber| subscriber.id == id) {
        return;
    }

    subscribers.push(TargetViewerSubscriber { id, callback });
    trace!(
        target: LOG_TARGET,
        "{label} subscriber registered: subscribers={}",
        subscribers.len(),
    );
}

fn emit(target: &Mutex<Vec<TargetViewerSubscriber>>, event: TargetViewerEvent) {
    let subscribers = target.lock().unwrap_or_else(|error| error.into_inner()).clone();

    for subscriber in subscribers {
        (subscriber.callback)(event);
    }
}

fn should_emit_target(target_id: i64) -> bool {
    let now_ms = monotonic_ms();
    let last_id = LAST_TARGET_ID.load(Relaxed);
    let last_ms = LAST_TARGET_LOG_MS.load(Relaxed);

    if target_id == last_id && now_ms.saturating_sub(last_ms) < TARGET_LOG_DEDUPE_MS {
        return false;
    }

    LAST_TARGET_ID.store(target_id, Relaxed);
    LAST_TARGET_LOG_MS.store(now_ms, Relaxed);
    true
}

fn monotonic_ms() -> i64 {
    let start = TARGET_LOG_TIMER_START.get_or_init(Instant::now);
    start.elapsed().as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::AtomicUsize;

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    static FIRST_CALLBACK_COUNT: AtomicUsize = AtomicUsize::new(0);
    static SECOND_CALLBACK_COUNT: AtomicUsize = AtomicUsize::new(0);

    fn first_callback(_: TargetViewerEvent) {
        FIRST_CALLBACK_COUNT.fetch_add(1, Relaxed);
    }

    fn second_callback(_: TargetViewerEvent) {
        SECOND_CALLBACK_COUNT.fetch_add(1, Relaxed);
    }

    fn reset_counts() {
        FIRST_CALLBACK_COUNT.store(0, Relaxed);
        SECOND_CALLBACK_COUNT.store(0, Relaxed);
    }

    fn event() -> TargetViewerEvent {
        TargetViewerEvent {
            prescan: std::ptr::null_mut(),
            fleet_id: Some(7),
        }
    }

    #[test]
    fn subscribe_ignores_same_callback_twice() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_counts();
        let subscribers = Mutex::new(Vec::new());

        subscribe(&subscribers, first_callback, "test");
        subscribe(&subscribers, first_callback, "test");
        emit(&subscribers, event());

        assert_eq!(subscribers.lock().unwrap().len(), 1);
        assert_eq!(FIRST_CALLBACK_COUNT.load(Relaxed), 1);
    }

    #[test]
    fn emit_calls_all_subscribers() {
        let _guard = TEST_LOCK.lock().unwrap();
        reset_counts();
        let subscribers = Mutex::new(Vec::new());

        subscribe(&subscribers, first_callback, "test");
        subscribe(&subscribers, second_callback, "test");
        emit(&subscribers, event());

        assert_eq!(subscribers.lock().unwrap().len(), 2);
        assert_eq!(FIRST_CALLBACK_COUNT.load(Relaxed), 1);
        assert_eq!(SECOND_CALLBACK_COUNT.load(Relaxed), 1);
    }
}
