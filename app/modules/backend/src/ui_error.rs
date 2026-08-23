//! Stable user-interface error codes shared between the Rust backend and localized frontend.

use serde::Serialize;
use ts_rs::TS;

/// User-facing failure selected by backend policy and translated only at the presentation boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
#[allow(dead_code)] // The serialized contract includes frontend-only and platform-specific variants.
pub enum UiErrorCode {
    /// An unexpected IPC or runtime failure occurred.
    Unexpected,
    /// STFC could not be found on this system.
    StfcNotFound,
    /// STFC must be closed before the requested action can continue.
    GameRunning,
    /// The bundled Daystrom mod is unavailable.
    ModUnavailable,
    /// macOS entitlement patching failed.
    EntitlementPatchingFailed,
    /// Required macOS entitlements are missing.
    EntitlementsMissing,
    /// The Daystrom mod could not be deployed.
    ModDeploymentFailed,
    /// The deployed Daystrom mod could not be removed.
    ModRemovalFailed,
    /// The Scopely launcher could not be opened.
    LauncherUnavailable,
    /// STFC could not be launched.
    GameLaunchFailed,
    /// A game process with a failed UI startup could not be terminated.
    GameTerminationFailed,
    /// The selected local player profile no longer exists.
    ProfileNotFound,
    /// The selected local player profile could not be deleted.
    ProfileDeletionFailed,
    /// The Daystrom update check failed.
    UpdateCheckFailed,
    /// The Daystrom update could not be prepared.
    UpdatePrepareFailed,
    /// The selected Daystrom update could not be confirmed.
    UpdateConfirmFailed,
    /// A different Daystrom version replaced the reviewed update.
    UpdateChanged,
    /// The update manifest referred to an untrusted package location.
    UpdateUntrusted,
    /// The previous release could not be retained for rollback.
    RollbackRetentionFailed,
    /// The update package could not be downloaded or verified.
    UpdateDownloadFailed,
    /// The verified update package could not be cached safely.
    UpdateCacheFailed,
    /// The verified update package could not be installed.
    UpdateInstallFailed,
    /// The restored bundled mod could not be activated.
    RestoredModActivationFailed,
    /// Daystrom could not save that the restored mod is ready.
    RestoredModStateSaveFailed,
    /// The available rollback changed while it was being prepared.
    RollbackChanged,
    /// The previous release could not be verified.
    RollbackVerifyFailed,
    /// The rollback could not be prepared safely.
    RollbackPrepareFailed,
    /// The previous release could not be restored.
    RollbackRestoreFailed,
}
