//! Backend-owned discovery state for Project Daystrom application updates.
//!
//! The updater endpoint, scheduling, version comparison, and notification decisions remain in
//! Rust. The frontend receives only display data and can request a check or dismiss one version
//! for the lifetime of the current process.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use semver::Version;
use serde::Serialize;
use tauri::Emitter;
use tauri_plugin_updater::{Update, Updater, UpdaterExt};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::ui_error::UiErrorCode;
use crate::use_log;

use_log!("DaystromUpdate");

mod install;
mod rollback;
mod rollback_cache;

pub(crate) use install::PendingInstallResult;
pub use install::install_daystrom_update;
pub use rollback::{get_cached_daystrom_rollback_status, restore_previous_daystrom_version};

/// Environment variable that overrides the update manifest only in debug builds.
#[cfg(debug_assertions)]
const DEBUG_UPDATE_ENDPOINT: &str = "DAYSTROM_UPDATE_ENDPOINT";

/// Environment variable that accelerates periodic checks only in debug builds.
#[cfg(debug_assertions)]
const DEBUG_UPDATE_INTERVAL_SECONDS: &str = "DAYSTROM_UPDATE_INTERVAL_SECONDS";

/// Delay between automatic checks after the initial startup check.
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Minimum age of a newly published feature release before production discovery exposes it.
const MINOR_UPDATE_DELAY_SECONDS: i64 = 12 * 60 * 60;

/// Minimum age of a newly published major release before production discovery exposes it.
const MAJOR_UPDATE_DELAY_SECONDS: i64 = 24 * 60 * 60;

/// Maximum duration of one manifest request before discovery fails visibly.
const UPDATE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of Unicode characters exposed from remote release notes.
const MAX_RELEASE_NOTES_CHARACTERS: usize = 2_000;

/// Maximum number of lines exposed from remote release notes.
const MAX_RELEASE_NOTES_LINES: usize = 20;

/// Whether the permanent update monitor has already been started.
static MONITOR_STARTED: AtomicBool = AtomicBool::new(false);

/// Prevent overlapping startup, periodic, and manual update requests.
static CHECK_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Last available version observed in this process, used as the notification baseline.
static SEEN_VERSION: Mutex<Option<String>> = Mutex::new(None);

/// Current application-update status exposed to the frontend.
static STATE: Mutex<DaystromUpdateStatus> = Mutex::new(DaystromUpdateStatus {
    phase: DaystromUpdatePhase::Idle,
    version: None,
    notes: None,
    download_progress: None,
    error: None,
    dismissed: false,
    can_install: false,
    busy: false,
});

/// Current phase of Daystrom's application-update discovery.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DaystromUpdatePhase {
    /// No update check has completed yet.
    Idle,
    /// A startup or user-requested check is running.
    Checking,
    /// The installed Daystrom version is current.
    UpToDate,
    /// A newer release is described by the configured manifest.
    Available,
    /// The selected release is being confirmed against the remote manifest.
    Confirming,
    /// The installed release is being verified or downloaded for rollback.
    RetainingRollback,
    /// The verified updater package is being downloaded.
    Downloading,
    /// The verified updater package is ready and the application is shutting down to install it.
    Installing,
    /// The latest visible update check failed.
    Failed,
}

impl DaystromUpdatePhase {
    /// Return whether this phase represents active update work.
    const fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Confirming | Self::RetainingRollback | Self::Downloading | Self::Installing
        )
    }
}

/// Display-safe snapshot of Project Daystrom's update state.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DaystromUpdateStatus {
    /// Current discovery phase.
    pub phase: DaystromUpdatePhase,
    /// Available application version, if one was found.
    pub version: Option<String>,
    /// Optional release notes from the configured update manifest.
    pub notes: Option<String>,
    /// Download completion percentage when the remote server reports a total size.
    pub download_progress: Option<u8>,
    /// Stable failure code for the latest visible check.
    pub error: Option<UiErrorCode>,
    /// Whether the available-version banner is dismissed for this process.
    pub dismissed: bool,
    /// Whether this build may install the currently available update.
    pub can_install: bool,
    /// Whether the current visible update phase represents active work.
    pub busy: bool,
}

impl Default for DaystromUpdateStatus {
    fn default() -> Self {
        Self {
            phase: DaystromUpdatePhase::Idle,
            version: None,
            notes: None,
            download_progress: None,
            error: None,
            dismissed: false,
            can_install: false,
            busy: false,
        }
    }
}

/// Reason an update check was started.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CheckTrigger {
    /// Initial check after normal application startup.
    Startup,
    /// Silent six-hour background check.
    Periodic,
    /// Explicit user request from the main window.
    Manual,
}

/// Display metadata retained from an available update.
struct AvailableUpdate {
    /// Version announced by the manifest.
    version: String,
    /// Optional release notes announced by the manifest.
    notes: Option<String>,
}

/// Production rollout decision derived from SemVer and the release line's rollout anchor.
#[derive(Debug, PartialEq)]
enum RolloutDecision {
    /// The release has completed its minimum waiting period.
    Eligible,
    /// The release remains hidden until this instant.
    Deferred(OffsetDateTime),
    /// Publication finalization has not added the required rollout anchor yet.
    MissingPublicationDate,
}

impl From<Update> for AvailableUpdate {
    fn from(update: Update) -> Self {
        Self {
            version: update.version,
            notes: normalize_release_notes(update.body.as_deref()),
        }
    }
}

/// Normalize untrusted manifest notes before exposing them to the frontend.
fn normalize_release_notes(notes: Option<&str>) -> Option<String> {
    let normalized = notes?.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::new();
    let mut characters = 0;
    let mut lines = 1;
    let mut truncated = false;

    for character in normalized.chars() {
        let character = match character {
            '\t' => ' ',
            '\n' if lines >= MAX_RELEASE_NOTES_LINES => {
                truncated = true;
                break;
            }
            '\n' => {
                lines += 1;
                '\n'
            }
            character if is_disallowed_release_note_character(character) => continue,
            character => character,
        };

        if characters >= MAX_RELEASE_NOTES_CHARACTERS {
            truncated = true;
            break;
        }
        output.push(character);
        characters += 1;
    }

    let mut output = output.trim().to_string();
    if output.is_empty() {
        return None;
    }
    if truncated {
        if output.chars().count() >= MAX_RELEASE_NOTES_CHARACTERS {
            output.pop();
        }
        output.push('…');
    }
    Some(output)
}

/// Reject control and bidirectional formatting characters from remote display text.
fn is_disallowed_release_note_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// Guard that releases the global in-flight marker on every return path.
struct CheckGuard;

impl Drop for CheckGuard {
    fn drop(&mut self) {
        CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// Start application-update discovery for the lifetime of this backend process.
///
/// Performs one startup check and then checks silently every six hours. Repeated calls are ignored.
pub fn start(app: tauri::AppHandle) {
    if MONITOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    rollback_cache::reconcile_after_start(&app);
    rollback::initialize(&app);
    rollback::resume_mod_restore(&app, crate::game::is_game_running());

    let interval = update_check_interval();
    tauri::async_runtime::spawn(async move {
        run_check(&app, CheckTrigger::Startup).await;
        loop {
            tokio::time::sleep(interval).await;
            run_check(&app, CheckTrigger::Periodic).await;
        }
    });
}

/// Finish an outstanding bundled-mod restore after the process monitor observes STFC exiting.
pub(crate) fn resume_pending_mod_restore(app: &tauri::AppHandle) {
    rollback::resume_mod_restore(app, false);
}

/// Mark an outstanding bundled-mod restore complete after explicit mod preparation succeeded.
pub(crate) fn complete_pending_mod_restore(app: &tauri::AppHandle) {
    rollback::complete_mod_restore(app);
}

/// Resolve the periodic interval, honouring a positive debug-only seconds override.
#[cfg(debug_assertions)]
fn update_check_interval() -> Duration {
    let Ok(value) = std::env::var(DEBUG_UPDATE_INTERVAL_SECONDS) else {
        return UPDATE_CHECK_INTERVAL;
    };
    let Some(interval) = parse_debug_interval(&value) else {
        log_warn!("Ignoring invalid {DEBUG_UPDATE_INTERVAL_SECONDS}={value:?}; expected positive seconds");
        return UPDATE_CHECK_INTERVAL;
    };
    log_info!("Using debug update interval of {} seconds", interval.as_secs());
    interval
}

/// Return the fixed production interval without consulting process environment variables.
#[cfg(not(debug_assertions))]
fn update_check_interval() -> Duration {
    UPDATE_CHECK_INTERVAL
}

/// Parse a positive debug interval in seconds.
#[cfg(debug_assertions)]
fn parse_debug_interval(value: &str) -> Option<Duration> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
}

/// Return the latest cached Daystrom update status without starting network activity.
#[tauri::command]
pub fn get_cached_daystrom_update_status() -> DaystromUpdateStatus {
    STATE.lock().unwrap().clone()
}

/// Run an immediate Daystrom update check requested by the user.
#[tauri::command]
pub async fn check_for_daystrom_update(app: tauri::AppHandle) {
    run_check(&app, CheckTrigger::Manual).await;
}

/// Hide the currently available update for the lifetime of this Daystrom process.
#[tauri::command]
pub fn dismiss_daystrom_update(app: tauri::AppHandle) {
    update_state(&app, |status| {
        if status.phase == DaystromUpdatePhase::Available {
            status.dismissed = true;
        }
    });
}

/// Execute one update request and publish its result according to its trigger.
async fn run_check(app: &tauri::AppHandle, trigger: CheckTrigger) {
    if install::is_in_progress() {
        log_debug!("Skipping {trigger:?} update check while installation is in progress");
        return;
    }

    if trigger == CheckTrigger::Manual {
        update_state(app, |status| {
            status.phase = DaystromUpdatePhase::Checking;
            status.download_progress = None;
            status.error = None;
            status.dismissed = false;
            status.can_install = false;
        });
    }

    let Some(_guard) = acquire_check(trigger).await else {
        return;
    };

    if trigger != CheckTrigger::Periodic {
        update_state(app, |status| {
            status.phase = DaystromUpdatePhase::Checking;
            status.download_progress = None;
            status.error = None;
            status.can_install = false;
        });
    }

    let endpoints = effective_update_endpoint_description(app);
    log_debug!("Checking for Daystrom updates at {endpoints} ({trigger:?})");
    let result = check_remote(app).await;
    if install::is_in_progress() {
        log_debug!("Discarding completed {trigger:?} update check because installation has started");
        return;
    }
    match result {
        Ok(Some(update)) => match rollout_decision(app, &update, OffsetDateTime::now_utc()) {
            Ok(RolloutDecision::Eligible) => apply_available_update(app, update.into(), trigger),
            Ok(RolloutDecision::Deferred(available_at)) => {
                update_state(app, |status| *status = up_to_date_status());
                log_info!("Daystrom update {} remains staged until {available_at}", update.version);
            }
            Ok(RolloutDecision::MissingPublicationDate) => {
                update_state(app, |status| *status = up_to_date_status());
                log_warn!(
                    "Daystrom update {} has no finalized rollout time and remains hidden",
                    update.version
                );
            }
            Err(error) => {
                log_warn!("Could not evaluate Daystrom update rollout: {error}");
                update_state(app, |status| {
                    if let Some(next) = failure_status(status, trigger, installation_allowed()) {
                        *status = next;
                    }
                });
            }
        },
        Ok(None) => {
            update_state(app, |status| *status = up_to_date_status());
            log_debug!("Daystrom is up to date");
        }
        Err(error) => {
            log_warn!("Daystrom update check at {endpoints} failed: {error}");
            update_state(app, |status| {
                if let Some(next) = failure_status(status, trigger, installation_allowed()) {
                    *status = next;
                }
            });
        }
    }
}

/// Decide whether a discovered release has completed its production waiting period.
fn rollout_decision(app: &tauri::AppHandle, update: &Update, now: OffsetDateTime) -> Result<RolloutDecision, String> {
    #[cfg(debug_assertions)]
    if std::env::var_os(DEBUG_UPDATE_ENDPOINT).is_some() {
        return Ok(RolloutDecision::Eligible);
    }

    rollout_decision_for(&app.package_info().version, &update.version, update.date, now)
}

/// Pure rollout decision shared by production discovery and unit tests.
fn rollout_decision_for(
    current: &Version,
    target: &str,
    rollout_at: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> Result<RolloutDecision, String> {
    let Some(rollout_at) = rollout_at else {
        return Ok(RolloutDecision::MissingPublicationDate);
    };
    let target = Version::parse(target).map_err(|error| format!("invalid update version {target}: {error}"))?;
    let delay_seconds = if target.major != current.major {
        MAJOR_UPDATE_DELAY_SECONDS
    } else if target.minor != current.minor {
        MINOR_UPDATE_DELAY_SECONDS
    } else {
        0
    };
    let available_at = rollout_at + time::Duration::seconds(delay_seconds);
    Ok(if now >= available_at {
        RolloutDecision::Eligible
    } else {
        RolloutDecision::Deferred(available_at)
    })
}

/// Acquire the single update-check slot, waiting for a user-requested check when necessary.
async fn acquire_check(trigger: CheckTrigger) -> Option<CheckGuard> {
    loop {
        if CHECK_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return Some(CheckGuard);
        }
        if trigger != CheckTrigger::Manual {
            log_debug!("Skipping overlapping {trigger:?} update check");
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Build an updater from the fixed production configuration and optional debug-only endpoint.
fn build_updater(app: &tauri::AppHandle) -> Result<Updater, String> {
    let mut builder = app.updater_builder();

    #[cfg(debug_assertions)]
    if let Ok(endpoint) = std::env::var(DEBUG_UPDATE_ENDPOINT) {
        let endpoint = endpoint
            .parse::<tauri::Url>()
            .map_err(|error| format!("Invalid {DEBUG_UPDATE_ENDPOINT}: {error}"))?;
        log_info!("Using debug update endpoint {endpoint}");
        builder = builder
            .endpoints(vec![endpoint])
            .map_err(|error| format!("Invalid debug update endpoint: {error}"))?;
    }

    builder
        .timeout(UPDATE_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())
}

/// Return the manifest endpoints used by the current build for diagnostic logging.
fn effective_update_endpoints(app: &tauri::AppHandle) -> Vec<String> {
    #[cfg(debug_assertions)]
    if let Ok(endpoint) = std::env::var(DEBUG_UPDATE_ENDPOINT) {
        return vec![endpoint];
    }

    configured_update_endpoints(app.config().plugins.0.get("updater"))
}

/// Describe the effective manifest endpoints for diagnostics and actionable failures.
fn effective_update_endpoint_description(app: &tauri::AppHandle) -> String {
    let endpoints = effective_update_endpoints(app).join(", ");
    if endpoints.is_empty() {
        "<no endpoint configured>".to_string()
    } else {
        endpoints
    }
}

/// Return whether this build may install updates from its effective manifest endpoint.
fn installation_allowed() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os(DEBUG_UPDATE_ENDPOINT).is_some()
    }

    #[cfg(not(debug_assertions))]
    {
        true
    }
}

/// Extract updater endpoints from Tauri's plugin configuration.
fn configured_update_endpoints(updater: Option<&serde_json::Value>) -> Vec<String> {
    updater
        .and_then(|config| config.get("endpoints"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}

/// Query the configured manifest and return a newer release when available.
async fn check_remote(app: &tauri::AppHandle) -> Result<Option<Update>, String> {
    build_updater(app)?.check().await.map_err(|error| error.to_string())
}

/// Store an available update and optionally notify about a newly discovered periodic result.
fn apply_available_update(app: &tauri::AppHandle, update: AvailableUpdate, trigger: CheckTrigger) {
    if should_suppress_rejected_update(trigger, rollback_cache::is_rejected_version(app, &update.version)) {
        log_info!(
            "Suppressing automatically rediscovered Daystrom {} after rollback",
            update.version
        );
        update_state(app, |status| *status = up_to_date_status());
        return;
    }
    let should_notify = record_seen_version(&update.version, trigger);
    update_state(app, |status| {
        *status = available_status(status, &update, trigger, installation_allowed());
    });

    log_info!("Daystrom update {} is available", update.version);
    if should_notify {
        crate::notifications::show_daystrom_update(app, &update.version);
    }
}

/// Return whether an automatic check must hide the release rejected by rollback.
fn should_suppress_rejected_update(trigger: CheckTrigger, rejected: bool) -> bool {
    rejected && trigger != CheckTrigger::Manual
}

/// Execute the rollback or forward-update package waiting for coordinated shutdown.
pub(crate) fn install_pending_action(app: &tauri::AppHandle) -> PendingInstallResult {
    match rollback::install_pending_rollback(app) {
        PendingInstallResult::None => install::install_pending_update(app),
        result => result,
    }
}

/// Record an observed version and decide whether this check warrants a native notification.
fn record_seen_version(version: &str, trigger: CheckTrigger) -> bool {
    let mut seen = SEEN_VERSION.lock().unwrap();
    record_seen_version_for(&mut seen, version, trigger)
}

/// Pure notification decision shared by production state and unit tests.
fn record_seen_version_for(seen: &mut Option<String>, version: &str, trigger: CheckTrigger) -> bool {
    let is_new = seen.as_deref() != Some(version);
    *seen = Some(version.to_string());
    is_new && trigger == CheckTrigger::Periodic
}

/// Construct available-update state while preserving or clearing dismissal as required.
fn available_status(
    previous: &DaystromUpdateStatus,
    update: &AvailableUpdate,
    trigger: CheckTrigger,
    can_install: bool,
) -> DaystromUpdateStatus {
    let same_version = previous.version.as_deref() == Some(&update.version);
    let dismissed = trigger != CheckTrigger::Manual && same_version && previous.dismissed;
    DaystromUpdateStatus {
        phase: DaystromUpdatePhase::Available,
        version: Some(update.version.clone()),
        notes: update.notes.clone(),
        download_progress: None,
        error: None,
        dismissed,
        can_install,
        busy: false,
    }
}

/// Construct the successful state used when no newer release exists.
fn up_to_date_status() -> DaystromUpdateStatus {
    DaystromUpdateStatus {
        phase: DaystromUpdatePhase::UpToDate,
        ..DaystromUpdateStatus::default()
    }
}

/// Construct a visible failure state without discarding a previously confirmed update.
fn failure_status(
    previous: &DaystromUpdateStatus,
    trigger: CheckTrigger,
    can_install: bool,
) -> Option<DaystromUpdateStatus> {
    if trigger == CheckTrigger::Periodic {
        return None;
    }

    let error = Some(UiErrorCode::UpdateCheckFailed);
    if previous.version.is_some() {
        let mut status = previous.clone();
        status.phase = DaystromUpdatePhase::Available;
        status.download_progress = None;
        status.error = error;
        status.can_install = can_install;
        Some(status)
    } else {
        Some(DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Failed,
            error,
            ..DaystromUpdateStatus::default()
        })
    }
}

/// Mutate the cached status and emit it only when its display-safe value changed.
fn update_state(app: &tauri::AppHandle, updater: impl FnOnce(&mut DaystromUpdateStatus)) {
    if let Some(payload) = crate::state_update::update_if_changed(&STATE, |status| {
        updater(status);
        status.busy = status.phase.is_busy();
    }) {
        log_debug!("Daystrom update status changed, emitting to frontend");
        let _ = app.emit("daystrom-update-status", payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_phases_are_backend_owned() {
        for (phase, expected) in [
            (DaystromUpdatePhase::Idle, false),
            (DaystromUpdatePhase::Checking, true),
            (DaystromUpdatePhase::UpToDate, false),
            (DaystromUpdatePhase::Available, false),
            (DaystromUpdatePhase::Confirming, true),
            (DaystromUpdatePhase::RetainingRollback, true),
            (DaystromUpdatePhase::Downloading, true),
            (DaystromUpdatePhase::Installing, true),
            (DaystromUpdatePhase::Failed, false),
        ] {
            assert_eq!(phase.is_busy(), expected, "unexpected busy state for {phase:?}");
        }
    }

    /// Fixed release-line anchor used by rollout-delay tests.
    fn rollout_time() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }

    /// Parse a current application version for rollout-delay tests.
    fn current_version(version: &str) -> Version {
        Version::parse(version).unwrap()
    }

    /// Build minimal metadata for pure transition tests.
    fn available(version: &str) -> AvailableUpdate {
        AvailableUpdate {
            version: version.to_string(),
            notes: Some("Release notes".to_string()),
        }
    }

    #[test]
    fn startup_available_update_is_notification_baseline() {
        let mut seen = None;
        assert!(!record_seen_version_for(&mut seen, "0.10.0", CheckTrigger::Startup));
        assert_eq!(seen.as_deref(), Some("0.10.0"));
    }

    #[test]
    fn periodic_check_notifies_only_for_a_new_version() {
        let mut seen = Some("0.10.0".to_string());
        assert!(!record_seen_version_for(&mut seen, "0.10.0", CheckTrigger::Periodic));
        assert!(record_seen_version_for(&mut seen, "0.11.0", CheckTrigger::Periodic));
        assert!(!record_seen_version_for(&mut seen, "0.11.0", CheckTrigger::Periodic));
    }

    #[test]
    fn manual_check_never_issues_a_native_notification() {
        let mut seen = Some("0.10.0".to_string());
        assert!(!record_seen_version_for(&mut seen, "0.11.0", CheckTrigger::Manual));
        assert_eq!(seen.as_deref(), Some("0.11.0"));
    }

    #[test]
    fn rejected_update_requires_an_explicit_manual_check() {
        assert!(should_suppress_rejected_update(CheckTrigger::Startup, true));
        assert!(should_suppress_rejected_update(CheckTrigger::Periodic, true));
        assert!(!should_suppress_rejected_update(CheckTrigger::Manual, true));
        assert!(!should_suppress_rejected_update(CheckTrigger::Periodic, false));
    }

    #[test]
    fn periodic_same_version_preserves_dismissal() {
        let previous = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Available,
            version: Some("0.10.0".to_string()),
            dismissed: true,
            ..DaystromUpdateStatus::default()
        };

        let next = available_status(&previous, &available("0.10.0"), CheckTrigger::Periodic, true);

        assert!(next.dismissed);
        assert!(next.can_install);
    }

    #[test]
    fn manual_check_reshows_dismissed_version() {
        let previous = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Available,
            version: Some("0.10.0".to_string()),
            dismissed: true,
            ..DaystromUpdateStatus::default()
        };

        let next = available_status(&previous, &available("0.10.0"), CheckTrigger::Manual, true);

        assert!(!next.dismissed);
    }

    #[test]
    fn newer_version_clears_previous_dismissal() {
        let previous = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Available,
            version: Some("0.10.0".to_string()),
            dismissed: true,
            ..DaystromUpdateStatus::default()
        };

        let next = available_status(&previous, &available("0.11.0"), CheckTrigger::Periodic, true);

        assert!(!next.dismissed);
    }

    #[test]
    fn startup_failure_without_known_update_is_visible() {
        let next = failure_status(&DaystromUpdateStatus::default(), CheckTrigger::Startup, false).unwrap();

        assert_eq!(next.phase, DaystromUpdatePhase::Failed);
        assert!(next.version.is_none());
        assert!(next.error.is_some());
    }

    #[test]
    fn manual_failure_preserves_known_update() {
        let previous = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Checking,
            version: Some("0.10.0".to_string()),
            notes: Some("Release notes".to_string()),
            dismissed: false,
            ..DaystromUpdateStatus::default()
        };

        let next = failure_status(&previous, CheckTrigger::Manual, true).unwrap();

        assert_eq!(next.phase, DaystromUpdatePhase::Available);
        assert_eq!(next.version.as_deref(), Some("0.10.0"));
        assert_eq!(next.notes.as_deref(), Some("Release notes"));
        assert!(!next.dismissed);
        assert!(next.can_install);
        assert!(next.error.is_some());
    }

    #[test]
    fn periodic_failure_does_not_replace_cached_state() {
        let previous = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Available,
            version: Some("0.10.0".to_string()),
            ..DaystromUpdateStatus::default()
        };

        assert!(failure_status(&previous, CheckTrigger::Periodic, true).is_none());
    }

    #[test]
    fn configured_endpoints_are_extracted_for_logging() {
        let config = serde_json::json!({
            "endpoints": [
                "https://example.test/latest.json",
                "https://mirror.example.test/latest.json"
            ]
        });

        assert_eq!(
            configured_update_endpoints(Some(&config)),
            vec![
                "https://example.test/latest.json".to_string(),
                "https://mirror.example.test/latest.json".to_string()
            ]
        );
    }

    #[test]
    fn release_notes_are_normalized_as_display_text() {
        let notes = normalize_release_notes(Some("  <strong>Release</strong>\r\nLine\t2\u{0000}\u{202e}  "));

        assert_eq!(notes.as_deref(), Some("<strong>Release</strong>\nLine 2"));
    }

    #[test]
    fn release_notes_are_limited_by_unicode_character_count() {
        let input = "🚀".repeat(MAX_RELEASE_NOTES_CHARACTERS + 1);

        let notes = normalize_release_notes(Some(&input)).unwrap();

        assert_eq!(notes.chars().count(), MAX_RELEASE_NOTES_CHARACTERS);
        assert!(notes.ends_with('…'));
    }

    #[test]
    fn release_notes_are_limited_by_line_count() {
        let input = (1..=MAX_RELEASE_NOTES_LINES + 1)
            .map(|line| format!("Line {line}"))
            .collect::<Vec<_>>()
            .join("\n");

        let notes = normalize_release_notes(Some(&input)).unwrap();

        assert_eq!(notes.lines().count(), MAX_RELEASE_NOTES_LINES);
        assert!(notes.ends_with('…'));
    }

    #[test]
    fn empty_release_notes_are_omitted() {
        assert_eq!(normalize_release_notes(Some(" \t\u{0000} ")), None);
    }

    #[test]
    fn automatic_check_interval_is_six_hours() {
        assert_eq!(UPDATE_CHECK_INTERVAL, Duration::from_secs(21_600));
    }

    #[test]
    fn patch_update_is_eligible_immediately() {
        let rollout_at = rollout_time();

        let decision = rollout_decision_for(&current_version("0.9.0"), "0.9.1", Some(rollout_at), rollout_at).unwrap();

        assert_eq!(decision, RolloutDecision::Eligible);
    }

    #[test]
    fn minor_update_is_deferred_for_twelve_hours() {
        let rollout_at = rollout_time();
        let available_at = rollout_at + time::Duration::hours(12);

        assert_eq!(
            rollout_decision_for(
                &current_version("0.9.4"),
                "0.10.1",
                Some(rollout_at),
                available_at - time::Duration::seconds(1),
            )
            .unwrap(),
            RolloutDecision::Deferred(available_at)
        );
        assert_eq!(
            rollout_decision_for(&current_version("0.9.4"), "0.10.1", Some(rollout_at), available_at,).unwrap(),
            RolloutDecision::Eligible
        );
    }

    #[test]
    fn major_update_is_deferred_for_twenty_four_hours() {
        let rollout_at = rollout_time();
        let available_at = rollout_at + time::Duration::hours(24);

        assert_eq!(
            rollout_decision_for(
                &current_version("0.9.4"),
                "1.0.0",
                Some(rollout_at),
                available_at - time::Duration::seconds(1),
            )
            .unwrap(),
            RolloutDecision::Deferred(available_at)
        );
        assert_eq!(
            rollout_decision_for(&current_version("0.9.4"), "1.0.0", Some(rollout_at), available_at,).unwrap(),
            RolloutDecision::Eligible
        );
    }

    #[test]
    fn update_without_final_publication_time_remains_hidden() {
        assert_eq!(
            rollout_decision_for(&current_version("0.9.0"), "0.9.1", None, rollout_time(),).unwrap(),
            RolloutDecision::MissingPublicationDate
        );
    }

    #[test]
    fn invalid_manifest_version_cannot_bypass_rollout_delay() {
        let result = rollout_decision_for(&current_version("0.9.0"), "next", Some(rollout_time()), rollout_time());

        assert!(result.unwrap_err().contains("invalid update version next"));
    }

    #[test]
    fn debug_interval_accepts_positive_seconds() {
        assert_eq!(parse_debug_interval("10"), Some(Duration::from_secs(10)));
    }

    #[test]
    fn debug_interval_rejects_zero_and_invalid_values() {
        assert_eq!(parse_debug_interval("0"), None);
        assert_eq!(parse_debug_interval("soon"), None);
    }
}
