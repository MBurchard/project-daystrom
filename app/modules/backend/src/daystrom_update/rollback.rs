//! Backend-owned one-click restoration of the sole verified predecessor release.

use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;
use ts_rs::TS;

use super::install::{InstallGuard, PendingInstallResult};
use super::rollback_cache::PreparedRollback;

/// Current rollback status exposed to the display-only frontend.
static STATE: Mutex<DaystromRollbackStatus> = Mutex::new(DaystromRollbackStatus {
    phase: DaystromRollbackPhase::Unavailable,
    version: None,
    error: None,
    can_restore: false,
    mod_restore_pending: false,
});

/// Verified rollback retained until frontend logging has been flushed.
static PENDING_ROLLBACK: Mutex<Option<PreparedRollback>> = Mutex::new(None);

/// Current phase of Daystrom's one-click rollback flow.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DaystromRollbackPhase {
    /// No verified predecessor is available.
    Unavailable,
    /// A verified predecessor can be restored by explicit user request.
    Available,
    /// Cached package, authorization, and settings are being reverified.
    Preparing,
    /// Daystrom is shutting down to install the verified predecessor.
    Installing,
    /// The latest rollback attempt failed and may be retried.
    Failed,
}

/// Display-safe snapshot of Project Daystrom's rollback state.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DaystromRollbackStatus {
    /// Current rollback phase.
    pub phase: DaystromRollbackPhase,
    /// Sole predecessor version that may be restored.
    pub version: Option<String>,
    /// User-facing failure summary for the latest attempt.
    pub error: Option<String>,
    /// Whether the current state accepts an explicit restore request.
    pub can_restore: bool,
    /// Whether STFC must close before the restored bundled mod can become active.
    pub mod_restore_pending: bool,
}

/// Reconcile and publish rollback availability after application startup.
pub(super) fn initialize(app: &tauri::AppHandle) {
    publish_available(app, None);
}

/// Finish activation of a restored bundled mod once no game process is using the deployed version.
pub(super) fn resume_mod_restore(app: &tauri::AppHandle, game_running: bool) {
    if !super::rollback_cache::is_mod_restore_pending(app) {
        return;
    }
    if game_running {
        log_info!("Waiting for STFC to close before finishing the bundled mod restore");
        publish_available(app, None);
        return;
    }

    #[cfg(target_os = "windows")]
    let result = (|| {
        let game = crate::game::detect().ok_or_else(|| "STFC installation not found".to_string())?;
        if crate::game::is_running(&game.executable) {
            return Err("STFC started again before the bundled mod could be restored".to_string());
        }
        let library =
            crate::game::find_mod_library(app).ok_or_else(|| "restored bundled mod library not found".to_string())?;
        crate::game::deploy_mod(&game.install_dir, &library)
    })();
    #[cfg(not(target_os = "windows"))]
    let result: Result<(), String> = Ok(());

    if let Err(error) = result {
        log_warn!("Could not finish the bundled mod restore: {error}");
        publish_available(
            app,
            Some("Could not activate the restored mod. Close STFC and restart Daystrom to retry."),
        );
        return;
    }
    if let Err(error) = super::rollback_cache::complete_mod_restore(app) {
        log_warn!("Could not record the completed bundled mod restore: {error}");
        publish_available(app, Some("The restored mod is ready, but Daystrom could not save that state."));
        return;
    }

    #[cfg(target_os = "windows")]
    crate::game_state::update(app, |status| {
        status.mod_deployed = true;
        status.mod_outdated = false;
    });
    log_info!("Bundled mod restore completed");
    publish_available(app, None);
}

/// Clear a pending mod restore after another command deployed the restored bundle successfully.
pub(super) fn complete_mod_restore(app: &tauri::AppHandle) {
    if !super::rollback_cache::is_mod_restore_pending(app) {
        return;
    }
    match super::rollback_cache::complete_mod_restore(app) {
        Ok(()) => publish_available(app, None),
        Err(error) => log_warn!("Could not record the completed bundled mod restore: {error}"),
    }
}

/// Return the latest cached rollback status without starting work.
#[tauri::command]
pub fn get_cached_daystrom_rollback_status() -> DaystromRollbackStatus {
    STATE.lock().unwrap().clone()
}

/// Reverify and install the sole predecessor selected by the user.
#[tauri::command]
pub async fn restore_previous_daystrom_version(app: tauri::AppHandle) {
    let Some(guard) = InstallGuard::acquire() else {
        log_debug!("Ignoring duplicate rollback request");
        return;
    };
    let expected_version = {
        let status = STATE.lock().unwrap();
        if !matches!(status.phase, DaystromRollbackPhase::Available | DaystromRollbackPhase::Failed)
            || !status.can_restore
        {
            log_debug!("Ignoring stale rollback request");
            return;
        }
        status.version.clone()
    };
    let Some(expected_version) = expected_version else {
        log_debug!("Ignoring rollback request without a predecessor version");
        return;
    };
    update_state(&app, |status| {
        status.phase = DaystromRollbackPhase::Preparing;
        status.error = None;
        status.can_restore = false;
    });

    log_info!("Reverifying cached Daystrom {expected_version} rollback package");
    let prepared = match super::rollback_cache::prepare_rollback(&app).await {
        Ok(prepared) if prepared.update.version == expected_version => prepared,
        Ok(prepared) => {
            log_warn!(
                "Rollback cache changed from {expected_version} to {} while preparing",
                prepared.update.version
            );
            publish_available(&app, Some("The available rollback changed. Review it and try again."));
            return;
        }
        Err(error) => {
            log_warn!("Could not prepare Daystrom rollback: {error}");
            publish_available(&app, Some("Could not verify the previous Daystrom release. Try again later."));
            return;
        }
    };
    if let Err(error) = super::rollback_cache::stage_rollback(&app, &prepared) {
        log_warn!("Could not stage Daystrom rollback: {error}");
        publish_available(&app, Some("Could not safely prepare the Daystrom rollback. Try again later."));
        return;
    }

    update_state(&app, |status| {
        status.phase = DaystromRollbackPhase::Installing;
        status.error = None;
        status.can_restore = false;
    });
    *PENDING_ROLLBACK.lock().unwrap() = Some(prepared);
    guard.retain_until_install();
    crate::request_shutdown(&app);
}

/// Install a pending predecessor after frontend logging has flushed.
pub(super) fn install_pending_rollback(app: &tauri::AppHandle) -> PendingInstallResult {
    let Some(prepared) = PENDING_ROLLBACK.lock().unwrap().take() else {
        return PendingInstallResult::None;
    };

    let version = prepared.update.version.clone();
    let successor_settings = match crate::settings::snapshot_for_rollback() {
        Ok(snapshot) => snapshot,
        Err(error) => return rollback_failed(app, &format!("Could not preserve current settings: {error}"), None),
    };
    if let Err(error) = crate::settings::restore_rollback_snapshot(prepared.settings.as_deref()) {
        return rollback_failed(
            app,
            &format!("Could not restore predecessor settings: {error}"),
            Some(successor_settings),
        );
    }

    log_info!("Installing verified Daystrom {version} rollback package");
    match prepared.update.install(&prepared.bytes) {
        Ok(()) => {
            log_info!("Daystrom {version} rollback installed successfully");
            PendingInstallResult::Installed
        }
        Err(error) => rollback_failed(
            app,
            &format!("Failed to install Daystrom {version} rollback: {error}"),
            Some(successor_settings),
        ),
    }
}

/// Recover from a failed rollback while keeping the current application usable.
fn rollback_failed(
    app: &tauri::AppHandle,
    diagnostic: &str,
    successor_settings: Option<Option<Vec<u8>>>,
) -> PendingInstallResult {
    log_error!("{diagnostic}");
    if let Some(snapshot) = successor_settings
        && let Err(error) = crate::settings::restore_rollback_snapshot(snapshot.as_deref())
    {
        log_error!("Failed to recover settings after rollback failure: {error}");
    }
    if let Err(error) = super::rollback_cache::abort_pending_rollback(app) {
        log_warn!("Failed to discard pending rollback cache state: {error}");
    }
    InstallGuard::release();
    publish_available(
        app,
        Some("Could not restore the previous Daystrom release. Restart Daystrom and try again."),
    );
    PendingInstallResult::Failed
}

/// Publish current rollback availability with an optional actionable error.
fn publish_available(app: &tauri::AppHandle, error: Option<&str>) {
    let version = super::rollback_cache::available_rollback_version(app);
    let mod_restore_pending = super::rollback_cache::is_mod_restore_pending(app);
    update_state(app, |status| *status = availability_status(version, error, mod_restore_pending));
}

/// Construct a complete rollback status from durable cache availability.
fn availability_status(
    version: Option<String>,
    error: Option<&str>,
    mod_restore_pending: bool,
) -> DaystromRollbackStatus {
    let phase = match (&version, error) {
        (Some(_), Some(_)) => DaystromRollbackPhase::Failed,
        (Some(_), None) => DaystromRollbackPhase::Available,
        (None, _) => DaystromRollbackPhase::Unavailable,
    };
    DaystromRollbackStatus {
        phase,
        can_restore: version.is_some(),
        version,
        error: error.map(str::to_string),
        mod_restore_pending,
    }
}

/// Mutate rollback state and emit a snapshot only when it changed.
fn update_state(app: &tauri::AppHandle, updater: impl FnOnce(&mut DaystromRollbackStatus)) {
    if let Some(payload) = crate::state_update::update_if_changed(&STATE, updater) {
        log_debug!("Daystrom rollback status changed, emitting to frontend");
        let _ = app.emit("daystrom-rollback-status", payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_status_keeps_retry_action_after_failure() {
        let status = availability_status(Some("0.9.0".to_string()), Some("Restore failed"), false);

        assert_eq!(status.phase, DaystromRollbackPhase::Failed);
        assert_eq!(status.version.as_deref(), Some("0.9.0"));
        assert_eq!(status.error.as_deref(), Some("Restore failed"));
        assert!(status.can_restore);
        assert!(!status.mod_restore_pending);
    }

    #[test]
    fn pending_mod_restore_is_exposed_without_an_available_app_rollback() {
        let status = availability_status(None, None, true);

        assert_eq!(status.phase, DaystromRollbackPhase::Unavailable);
        assert!(status.version.is_none());
        assert!(!status.can_restore);
        assert!(status.mod_restore_pending);
    }
}
