//! Download, verification, and installation for a user-approved Daystrom update.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri_plugin_updater::Update;

use super::{AvailableUpdate, CheckTrigger, DaystromUpdatePhase, DaystromUpdateStatus};

/// Maximum duration allowed for downloading an updater package.
pub(super) const UPDATE_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// GitHub host trusted to serve production updater packages.
pub(super) const RELEASE_HOST: &str = "github.com";

/// GitHub owner trusted to publish production updater packages.
pub(super) const RELEASE_OWNER: &str = "MBurchard";

/// GitHub repository trusted to publish production updater packages.
pub(super) const RELEASE_REPOSITORY: &str = "project-daystrom";

/// Prevent concurrent update downloads and installations.
static INSTALL_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Verified updater package waiting for the coordinated frontend shutdown.
static PENDING_INSTALL: Mutex<Option<PendingInstall>> = Mutex::new(None);

/// Verified update retained until frontend logging has been flushed.
struct PendingInstall {
    /// Platform-specific updater metadata and installation behaviour.
    update: Update,
    /// Downloaded package whose signature was verified by Tauri.
    bytes: Vec<u8>,
}

/// Guard that releases the installation slot unless ownership passes to the pending package.
pub(super) struct InstallGuard {
    release_on_drop: bool,
}

impl InstallGuard {
    /// Acquire the single installation slot.
    pub(super) fn acquire() -> Option<Self> {
        INSTALL_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self { release_on_drop: true })
    }

    /// Keep the slot reserved until the pending package is installed or fails.
    pub(super) fn retain_until_install(mut self) {
        self.release_on_drop = false;
    }

    /// Release the shared update or rollback installation slot after a deferred failure.
    pub(super) fn release() {
        INSTALL_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        if self.release_on_drop {
            Self::release();
        }
    }
}

/// Coalesce download chunks into percentage changes suitable for frontend events.
#[derive(Default)]
struct ProgressTracker {
    /// Bytes received across all chunks.
    downloaded: u64,
    /// Last percentage exposed to the frontend.
    last_percentage: Option<u8>,
}

impl ProgressTracker {
    /// Record one chunk and return a newly reached percentage when the total is known.
    fn record(&mut self, chunk_length: usize, content_length: Option<u64>) -> Option<u8> {
        self.downloaded = self.downloaded.saturating_add(chunk_length as u64);
        let total = content_length.filter(|total| *total > 0)?;
        let percentage = self.downloaded.saturating_mul(100).checked_div(total).unwrap_or(0).min(100) as u8;
        if self.last_percentage == Some(percentage) {
            return None;
        }
        self.last_percentage = Some(percentage);
        Some(percentage)
    }
}

/// Return whether update installation currently owns the update state.
pub(super) fn is_in_progress() -> bool {
    INSTALL_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Result of attempting the pending platform installation during coordinated shutdown.
pub(crate) enum PendingInstallResult {
    /// No verified updater package is waiting.
    None,
    /// The current application was replaced and must restart.
    Installed,
    /// Installation failed and the current application must remain open.
    Failed,
}

/// Download and verify the currently displayed update, then start coordinated installation.
#[tauri::command]
pub async fn install_daystrom_update(app: tauri::AppHandle) {
    if !super::installation_allowed() {
        log_debug!("Ignoring update installation request because this build cannot install updates");
        return;
    }

    let Some(install_guard) = InstallGuard::acquire() else {
        log_debug!("Ignoring duplicate update installation request");
        return;
    };
    let expected_version = match requested_version() {
        Ok(version) => version,
        Err(reason) => {
            log_debug!("Ignoring stale update installation request: {reason}");
            return;
        }
    };
    super::update_state(&app, mark_confirming);

    let Some(_check_guard) = super::acquire_check(CheckTrigger::Manual).await else {
        set_install_failure(&app, "Could not prepare the Daystrom update. Try again later.");
        return;
    };
    let endpoints = super::effective_update_endpoint_description(&app);
    log_info!("Confirming Daystrom update {expected_version} at {endpoints} before download");

    let mut update = match super::check_remote(&app).await {
        Ok(Some(update)) => update,
        Ok(None) => {
            log_info!("Daystrom update {expected_version} is no longer available");
            super::update_state(&app, |status| *status = super::up_to_date_status());
            return;
        }
        Err(error) => {
            log_warn!("Daystrom update confirmation at {endpoints} failed: {error}");
            set_install_failure(&app, "Could not confirm the Daystrom update. Try again later.");
            return;
        }
    };

    if update.version != expected_version {
        let actual_version = update.version.clone();
        log_info!("Daystrom update changed from {expected_version} to {actual_version} before download");
        super::apply_available_update(&app, AvailableUpdate::from(update), CheckTrigger::Manual);
        set_install_failure(
            &app,
            "A different Daystrom version is now available. Review it before installing.",
        );
        return;
    }

    if let Err(error) = validate_download_url(&update.download_url, &update.version) {
        log_warn!("{error}");
        set_install_failure(&app, "The Daystrom update refers to an untrusted download location.");
        return;
    }
    update.timeout = Some(UPDATE_DOWNLOAD_TIMEOUT);
    super::update_state(&app, mark_retaining_rollback);

    let rollback_package =
        match super::rollback_cache::retain_rollback_package(&app, &update, progress_reporter(&app)).await {
            Ok(package) => package,
            Err(error) => {
                log_warn!("Could not retain the required Daystrom rollback release: {error}");
                set_install_failure(
                    &app,
                    "Could not retain the previous Daystrom release for rollback. Try again later.",
                );
                return;
            }
        };

    super::update_state(&app, |status| {
        status.phase = DaystromUpdatePhase::Downloading;
        status.download_progress = None;
        status.error = None;
        status.dismissed = false;
        status.can_install = false;
    });

    let download_url = update.download_url.clone();
    log_info!("Downloading signed Daystrom update {expected_version} from {download_url}");
    let bytes = match update.download(progress_reporter(&app), || {}).await {
        Ok(bytes) => bytes,
        Err(error) => {
            log_warn!("Daystrom update download or signature verification failed at {download_url}: {error}");
            set_install_failure(&app, "Could not download and verify the Daystrom update. Try again later.");
            return;
        }
    };

    log_info!("Daystrom update {expected_version} downloaded and verified successfully");
    if let Err(error) = super::rollback_cache::stage_update(&app, rollback_package, &update, &bytes) {
        log_warn!("Could not cache the verified Daystrom update: {error}");
        set_install_failure(&app, "Could not safely retain the update package. Try again later.");
        return;
    }
    super::update_state(&app, |status| {
        status.phase = DaystromUpdatePhase::Installing;
        status.download_progress = Some(100);
        status.error = None;
        status.can_install = false;
    });
    *PENDING_INSTALL.lock().unwrap() = Some(PendingInstall { update, bytes });
    install_guard.retain_until_install();
    crate::request_shutdown(&app);
}

/// Build an independent progress callback for one package download.
fn progress_reporter(app: &tauri::AppHandle) -> impl FnMut(usize, Option<u64>) + '_ {
    let mut progress = ProgressTracker::default();
    move |chunk_length, content_length| {
        if let Some(percentage) = progress.record(chunk_length, content_length) {
            super::update_state(app, |status| status.download_progress = Some(percentage));
        }
    }
}

/// Install a verified pending package after frontend logging has been flushed.
///
pub(crate) fn install_pending_update(app: &tauri::AppHandle) -> PendingInstallResult {
    let Some(pending) = PENDING_INSTALL.lock().unwrap().take() else {
        return PendingInstallResult::None;
    };

    crate::settings::flush_saves();
    let version = pending.update.version.clone();
    log_info!("Installing verified Daystrom update {version}");
    match pending.update.install(&pending.bytes) {
        Ok(()) => {
            log_info!("Daystrom update {version} installed successfully");
            PendingInstallResult::Installed
        }
        Err(error) => {
            log_error!("Failed to install Daystrom update {version}: {error}");
            if let Err(cache_error) = super::rollback_cache::abort_pending_update(app) {
                log_warn!("Failed to discard pending update cache state: {cache_error}");
            }
            InstallGuard::release();
            set_install_failure(app, "Could not install the Daystrom update. Restart Daystrom and try again.");
            PendingInstallResult::Failed
        }
    }
}

/// Return the version for which the user explicitly requested installation.
fn requested_version() -> Result<String, String> {
    let status = super::STATE.lock().unwrap();
    if status.phase != DaystromUpdatePhase::Available || status.dismissed || !status.can_install {
        return Err("No reviewed Daystrom update is ready to install.".to_string());
    }
    status
        .version
        .clone()
        .ok_or_else(|| "The available Daystrom update has no version.".to_string())
}

/// Restore the available-update UI with an actionable installation failure.
fn set_install_failure(app: &tauri::AppHandle, message: &str) {
    super::update_state(app, |status| {
        restore_available_after_failure(status, message, super::installation_allowed());
    });
}

/// Pure failure transition shared by runtime state and unit tests.
fn restore_available_after_failure(status: &mut DaystromUpdateStatus, message: &str, can_install: bool) {
    status.phase = if status.version.is_some() {
        DaystromUpdatePhase::Available
    } else {
        DaystromUpdatePhase::Failed
    };
    status.download_progress = None;
    status.error = Some(message.to_string());
    status.dismissed = false;
    status.can_install = status.version.is_some() && can_install;
}

/// Mark an accepted installation request as visible and non-repeatable before any network wait.
fn mark_confirming(status: &mut DaystromUpdateStatus) {
    status.phase = DaystromUpdatePhase::Confirming;
    status.download_progress = None;
    status.error = None;
    status.dismissed = false;
    status.can_install = false;
}

/// Mark rollback retention as a separate visible network step.
fn mark_retaining_rollback(status: &mut DaystromUpdateStatus) {
    status.phase = DaystromUpdatePhase::RetainingRollback;
    status.download_progress = None;
    status.error = None;
    status.dismissed = false;
    status.can_install = false;
}

/// Validate the package location before downloading bytes described by an unsigned manifest.
fn validate_download_url(url: &tauri::Url, version: &str) -> Result<(), String> {
    if debug_endpoint_active() || is_trusted_release_download_url(url, version) {
        return Ok(());
    }
    Err(format!("Refusing untrusted Daystrom update download URL: {url}"))
}

/// Return whether an explicit debug manifest permits local installation testing.
fn debug_endpoint_active() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os(super::DEBUG_UPDATE_ENDPOINT).is_some()
    }

    #[cfg(not(debug_assertions))]
    {
        false
    }
}

/// Return whether a production package URL is a release asset from the trusted GitHub repository.
pub(super) fn is_trusted_release_download_url(url: &tauri::Url, version: &str) -> bool {
    if url.scheme() != "https"
        || url.host_str() != Some(RELEASE_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }

    let Some(segments) = url.path_segments() else {
        return false;
    };
    let segments: Vec<_> = segments.collect();
    matches!(
        segments.as_slice(),
        [owner, repository, "releases", "download", tag, asset]
            if *owner == RELEASE_OWNER
                && *repository == RELEASE_REPOSITORY
                && *tag == version
                && !asset.is_empty()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a GitHub release URL for validator tests.
    fn release_url(scheme: &str, repository: &str, suffix: &str) -> tauri::Url {
        tauri::Url::parse(&format!(
            "{scheme}://{RELEASE_HOST}/{RELEASE_OWNER}/{repository}/releases/download/0.10.0/update.tar.gz{suffix}"
        ))
        .unwrap()
    }

    #[test]
    fn progress_reports_only_new_percentages() {
        let mut progress = ProgressTracker::default();

        assert_eq!(progress.record(10, Some(100)), Some(10));
        assert_eq!(progress.record(0, Some(100)), None);
        assert_eq!(progress.record(15, Some(100)), Some(25));
        assert_eq!(progress.record(75, Some(100)), Some(100));
    }

    #[test]
    fn progress_stays_indeterminate_without_total_size() {
        let mut progress = ProgressTracker::default();

        assert_eq!(progress.record(1_024, None), None);
        assert_eq!(progress.downloaded, 1_024);
    }

    #[test]
    fn production_download_url_requires_trusted_github_repository() {
        let trusted = release_url("https", RELEASE_REPOSITORY, "");
        let wrong_repository = release_url("https", "other", "");
        let insecure = release_url("http", RELEASE_REPOSITORY, "");
        let unrelated_path = tauri::Url::parse(&format!(
            "https://{RELEASE_HOST}/{RELEASE_OWNER}/{RELEASE_REPOSITORY}/archive/0.10.0.zip"
        ))
        .unwrap();
        let query = release_url("https", RELEASE_REPOSITORY, "?source=test");

        let wrong_version = release_url("https", RELEASE_REPOSITORY, "")
            .as_str()
            .replace("/0.10.0/", "/0.11.0/")
            .parse()
            .unwrap();

        assert!(is_trusted_release_download_url(&trusted, "0.10.0"));
        assert!(!is_trusted_release_download_url(&wrong_repository, "0.10.0"));
        assert!(!is_trusted_release_download_url(&insecure, "0.10.0"));
        assert!(!is_trusted_release_download_url(&unrelated_path, "0.10.0"));
        assert!(!is_trusted_release_download_url(&query, "0.10.0"));
        assert!(!is_trusted_release_download_url(&wrong_version, "0.10.0"));
    }

    #[test]
    fn install_failure_restores_known_update() {
        let mut status = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Installing,
            version: Some("0.10.0".to_string()),
            notes: Some("Release notes".to_string()),
            download_progress: Some(100),
            error: None,
            dismissed: false,
            can_install: false,
        };

        restore_available_after_failure(&mut status, "Install failed", true);

        assert_eq!(status.phase, DaystromUpdatePhase::Available);
        assert_eq!(status.version.as_deref(), Some("0.10.0"));
        assert_eq!(status.notes.as_deref(), Some("Release notes"));
        assert_eq!(status.download_progress, None);
        assert_eq!(status.error.as_deref(), Some("Install failed"));
        assert!(status.can_install);
    }

    #[test]
    fn confirming_state_is_visible_and_disables_repeated_installation() {
        let mut status = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Available,
            version: Some("0.10.0".to_string()),
            download_progress: Some(42),
            error: Some("Previous failure".to_string()),
            dismissed: true,
            can_install: true,
            ..DaystromUpdateStatus::default()
        };

        mark_confirming(&mut status);

        assert_eq!(status.phase, DaystromUpdatePhase::Confirming);
        assert_eq!(status.version.as_deref(), Some("0.10.0"));
        assert_eq!(status.download_progress, None);
        assert_eq!(status.error, None);
        assert!(!status.dismissed);
        assert!(!status.can_install);
    }

    #[test]
    fn rollback_retention_has_independent_progress() {
        let mut status = DaystromUpdateStatus {
            phase: DaystromUpdatePhase::Downloading,
            version: Some("0.10.0".to_string()),
            download_progress: Some(100),
            error: Some("Previous failure".to_string()),
            dismissed: true,
            can_install: true,
            ..DaystromUpdateStatus::default()
        };

        mark_retaining_rollback(&mut status);

        assert_eq!(status.phase, DaystromUpdatePhase::RetainingRollback);
        assert_eq!(status.download_progress, None);
        assert_eq!(status.error, None);
        assert!(!status.dismissed);
        assert!(!status.can_install);
    }
}
