use serde::Serialize;
#[cfg(target_os = "windows")]
use tauri::Manager;
#[cfg(target_os = "windows")]
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use ts_rs::TS;

use crate::game;
use crate::use_log;

use_log!("Commands");

/// STFC installation and entitlement status as returned to the frontend.
///
/// Contains both base fields (set by detection/commands) and derived fields (computed automatically
/// by [`recompute_derived`]). The frontend treats all fields as read-only display data.
#[derive(Clone, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct GameStatus {
    // ---- Base fields (set by detection, commands, monitor) ----

    /// Whether STFC was found on this machine.
    pub installed: bool,
    /// Installed game version from the `.version` file, if available.
    pub game_version: Option<u32>,
    /// Whether the mod library was found in the app's resource directory.
    pub mod_available: bool,
    /// Whether the mod can be installed or updated (game found and the mod library bundled).
    pub mod_installable: bool,
    /// Whether the mod is deployed and ready (macOS: entitlements OK, Windows: DLL up to date).
    pub mod_deployed: bool,
    /// Whether the mod DLL exists but is outdated (hash mismatch). Always `false` on macOS.
    pub mod_outdated: bool,
    /// Whether the mod can be removed from the disk (Windows: DLL deployed or outdated, macOS: always false).
    pub mod_removable: bool,
    /// Whether the game process is currently running.
    pub game_running: bool,
    /// Whether the Scopely launcher is currently running.
    pub launcher_running: bool,
    /// Latest version reported by the Scopely update API, if reachable.
    pub remote_version: Option<u32>,
    /// Whether the last update check failed (network error, API down, etc.).
    pub update_check_failed: bool,
    /// Whether the game was launched via Daystrom's "Launch Game" button.
    pub game_started_by_us: bool,
    /// Whether the Scopely launcher was opened via Daystrom's "Update" button.
    pub launcher_started_by_us: bool,

    // ---- Derived fields (computed by recompute_derived) ----

    /// Whether a game update is available (remote > installed).
    pub update_available: bool,
    /// Whether all preconditions for launching the game are met.
    pub can_launch: bool,
    /// Whether the mod install/reinstall button should be enabled.
    pub can_install_mod: bool,
    /// Whether the mod remove button should be enabled.
    pub can_remove_mod: bool,
    /// Whether the updater button should be enabled.
    pub can_launch_updater: bool,
    /// Whether quitting the app should be blocked (Daystrom-started process still running).
    pub should_block_quit: bool,
    /// CSS class for the version-check checklist item: `"warn"`, `"ok"`, or `"neutral"`.
    pub version_check_class: String,
}

// ---- Game Status Builder --------------------------------------------------------

/// Snapshot of a detected game installation, gathered from I/O before the pure status builder runs.
///
/// Decouples the I/O-heavy detection step from the deterministic status assembly so the latter can
/// be tested without a Tauri runtime or filesystem access.
struct DetectedGame {
    /// Installed game version from the `.version` file, if available.
    installed_version: Option<u32>,
    /// Whether the mod library is deployed and up to date in the game directory.
    mod_deployed: bool,
    /// Whether the mod library exists on disk but is outdated (hash mismatch).
    mod_outdated: bool,
    /// Whether the game process is currently running.
    game_running: bool,
}

/// Recompute all derived fields from the base fields.
///
/// Called automatically by [`crate::game_state::update`] after every mutation. Pure function: no
/// I/O, no side effects beyond the mutable reference.
pub fn recompute_derived(s: &mut GameStatus) {
    s.update_available = match (s.game_version, s.remote_version) {
        (Some(installed), Some(remote)) => remote > installed,
        _ => false,
    };

    s.can_launch = s.mod_deployed
        && !s.update_available
        && !s.game_running
        && !s.launcher_running;

    s.can_install_mod = s.mod_installable
        && !s.update_available
        && !s.game_running
        && !s.launcher_running;

    s.can_remove_mod = s.mod_removable
        && !s.game_running
        && !s.launcher_running;

    s.can_launch_updater = s.update_available && !s.launcher_running;

    s.should_block_quit = (s.game_started_by_us && s.game_running)
        || (s.launcher_started_by_us && s.launcher_running);

    s.version_check_class = if s.update_available {
        "warn".to_string()
    } else if s.update_check_failed {
        "neutral".to_string()
    } else if s.remote_version.is_some() {
        "ok".to_string()
    } else {
        "neutral".to_string()
    };
}

/// Assemble a [`GameStatus`] from already-gathered inputs.
///
/// Pure function: no I/O, no Tauri dependency. All platform-specific decisions about
/// `mod_removable` are resolved via `cfg!()` so the logic is testable on any platform.
/// Derived fields are computed automatically via [`recompute_derived`].
fn build_game_status(
    detection: Option<&DetectedGame>,
    mod_available: bool,
    launcher_running: bool,
) -> GameStatus {
    let mut status = match detection {
        Some(det) => GameStatus {
            installed: true,
            game_version: det.installed_version,
            mod_available,
            mod_installable: mod_available,
            mod_deployed: det.mod_deployed,
            mod_outdated: det.mod_outdated,
            mod_removable: cfg!(target_os = "windows") && (det.mod_deployed || det.mod_outdated),
            game_running: det.game_running,
            launcher_running,
            ..GameStatus::default()
        },
        None => GameStatus {
            installed: false,
            game_version: None,
            mod_available,
            mod_installable: false,
            mod_deployed: false,
            mod_outdated: false,
            mod_removable: false,
            game_running: false,
            launcher_running,
            ..GameStatus::default()
        },
    };
    recompute_derived(&mut status);
    status
}

/// Detect the STFC installation and check its entitlements, mod availability, and running state.
///
/// Not a Tauri command: called by the monitor on startup and after state-changing actions.
pub fn get_game_status(app: tauri::AppHandle) -> GameStatus {
    let mod_library = game::find_mod_library(&app);
    let mod_available = mod_library.is_some();

    match &mod_library {
        Some(path) => log_info!("Mod library found: {}", path.display()),
        None => log_warn!("Mod library not bundled, run pnpm build:mod"),
    }

    let launcher_running = game::is_launcher_running();

    let detection = game::detect().map(|info| {
        match info.installed_version {
            Some(v) => log_info!("STFC found (v{v}): {}", info.executable.display()),
            None => log_info!("STFC found: {}", info.executable.display()),
        }

        let status = game::entitlements::check(&info.executable);
        if status.all_granted() {
            log_info!("Entitlements OK, mod injection ready");
        } else {
            let names: Vec<_> = status.missing.iter()
                .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
                .collect();
            log_warn!("Missing entitlements: {}", names.join(", "));
        }

        let game_running = game::is_running(&info.executable);

        // macOS: mod is "deployed" when entitlements are OK (injection via DYLD)
        // Windows: mod is deployed when the DLL is copied and up to date
        #[cfg(target_os = "macos")]
        let (mod_deployed, mod_outdated) = (status.all_granted(), false);
        #[cfg(target_os = "windows")]
        let (mod_deployed, mod_outdated) = mod_library.as_ref().map(|lib| {
            match game::check_mod_deployment(&info.install_dir, lib) {
                game::ModDeploymentState::UpToDate => (true, false),
                game::ModDeploymentState::Outdated => (false, true),
                game::ModDeploymentState::NotDeployed => (false, false),
            }
        }).unwrap_or((false, false));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let (mod_deployed, mod_outdated) = (false, false);

        DetectedGame {
            installed_version: info.installed_version,
            mod_deployed,
            mod_outdated,
            game_running,
        }
    });

    build_game_status(detection.as_ref(), mod_available, launcher_running)
}

// ---- Mod Preparation / Removal --------------------------------------------------

/// Prepare the mod for use: patch entitlements on macOS, deploy the DLL on Windows.
///
/// Updates the game state store on success, which automatically notifies the frontend.
#[tauri::command]
pub fn prepare_mod(app: tauri::AppHandle) -> Result<(), String> {
    let info = game::detect().ok_or("STFC not found")?;

    if game::is_running(&info.executable) {
        return Err("Cannot prepare mod while the game is running".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        game::entitlements::patch(&info.executable)?;
    }

    #[cfg(target_os = "windows")]
    {
        let mod_library = game::find_mod_library(&app)
            .ok_or("Mod library not found — run build:mod first")?;
        game::deploy_mod(&info.install_dir, &mod_library)?;
    }

    crate::game_state::update(&app, |s| {
        s.mod_deployed = true;
        s.mod_outdated = false;
        s.mod_removable = cfg!(target_os = "windows");
    });
    Ok(())
}

/// Remove the deployed mod from the game directory after user confirmation.
///
/// Shows a warning dialogue explaining that the game will only be launchable via the Scopely
/// Launcher afterwards. Updates the game state store on confirmed removal.
#[tauri::command]
pub fn remove_mod(
    #[cfg_attr(not(target_os = "windows"), allow(unused))] window: tauri::WebviewWindow,
) -> Result<(), String> {
    // macOS: mod is injected via DYLD at launch, nothing to remove from the disk
    #[cfg(not(target_os = "windows"))]
    #[allow(clippy::needless_return)]
    {
        log_warn!("remove_mod called on macOS, this should not happen");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let info = game::detect().ok_or("STFC not found")?;

        if game::is_running(&info.executable) {
            return Err("Cannot remove mod while the game is running".to_string());
        }

        let confirmed = window.dialog()
            .message("Remove the Community Mod?\n\n\
                      After removal, the game can only be launched through the Scopely Launcher.")
            .title("Remove Mod")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Remove".into(), "Cancel".into()))
            .blocking_show();

        if !confirmed {
            log_info!("Mod removal cancelled by user");
            return Ok(());
        }

        log_info!("User confirmed mod removal");
        game::remove_mod(&info.install_dir)?;

        let app = window.app_handle().clone();
        crate::game_state::update(&app, |s| {
            s.mod_deployed = false;
            s.mod_outdated = false;
            s.mod_removable = false;
        });
        Ok(())
    }
}

// ---- Update Check ---------------------------------------------------------------

/// Run an update check and write the result into the game state store.
///
/// On success, sets `remote_version` and clears `update_check_failed`. On failure, sets
/// `update_check_failed` and clears `remote_version`. The store's automatic change detection
/// handles frontend notification.
pub fn update_check_into_store(app: &tauri::AppHandle) {
    let result = (|| -> Result<(u32, Option<u32>), String> {
        let info = game::detect().ok_or("STFC not found")?;
        let installed = info.installed_version.ok_or("Could not read installed game version")?;
        let remote = game::version::fetch_remote(installed)?;
        Ok((installed, remote))
    })();

    match result {
        Ok((installed, remote)) => {
            crate::game_state::update(app, |s| {
                // When the API says "no update" (None), use the installed version so the
                // frontend knows the check completed successfully.
                s.remote_version = Some(remote.unwrap_or(installed));
                s.update_check_failed = false;
            });
        }
        Err(e) => {
            log_warn!("Update check failed: {e}");
            crate::game_state::update(app, |s| {
                s.update_check_failed = true;
            });
        }
    }
}

// ---- Cached Status --------------------------------------------------------------

/// Return the current cached game status, or `None` if the initial detection hasn't completed.
///
/// Reads from the in-memory store without triggering any I/O. The frontend calls this once after
/// registering event listeners to get the current state immediately (if available).
#[tauri::command]
pub fn get_cached_game_status() -> Option<GameStatus> {
    if crate::game_state::is_initialized() {
        Some(crate::game_state::get())
    } else {
        None
    }
}

// ---- Launch Commands ------------------------------------------------------------

/// Open the Scopely launcher so the user can install an update.
#[tauri::command]
pub fn launch_updater(app: tauri::AppHandle) -> Result<(), String> {
    game::launcher::open_updater()?;
    crate::process_origin::mark_launcher_started();
    crate::game_state::update(&app, |s| {
        s.launcher_started_by_us = true;
    });
    Ok(())
}

/// Launch the game with the mod library injected.
///
/// On macOS, checks entitlements before launching. On Windows, auto-deploys the DLL if needed.
/// The optional `profile` parameter sets the `DAYSTROM_PROFILE` environment variable:
/// - `None`: first start (Registry import)
/// - `Some("106_Nabor")`: launch with a known profile
/// - `Some("new_account")`: start a fresh account
#[tauri::command]
pub fn launch_game(app: tauri::AppHandle, profile: Option<String>) -> Result<(), String> {
    let info = game::detect().ok_or("STFC not found")?;

    let mod_library = game::find_mod_library(&app)
        .ok_or("Mod library not found — run build:mod first")?;

    // macOS: entitlements must be patched before launching
    #[cfg(target_os = "macos")]
    {
        let status = game::entitlements::check(&info.executable);
        if !status.all_granted() {
            let names: Vec<_> = status.missing.iter()
                .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
                .collect();
            return Err(format!("Missing entitlements: {} — patch them first", names.join(", ")));
        }
    }

    let pid = game::launcher::launch(&info, &mod_library, profile.as_deref())?;
    let profile_stem = profile.unwrap_or_default();
    crate::process_origin::register_launch(pid, profile_stem);
    crate::process_origin::mark_game_started();
    crate::game_state::update(&app, |s| {
        s.game_started_by_us = true;
    });
    Ok(())
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- build_game_status --

    #[test]
    fn game_not_found_returns_minimal_status() {
        let status = build_game_status(None, false, false);
        assert!(!status.installed);
        assert!(status.game_version.is_none());
        assert!(!status.mod_available);
        assert!(!status.mod_installable);
        assert!(!status.mod_deployed);
        assert!(!status.mod_outdated);
        assert!(!status.mod_removable);
        assert!(!status.game_running);
        assert!(!status.launcher_running);
    }

    #[test]
    fn game_not_found_but_mod_bundled() {
        let status = build_game_status(None, true, false);
        assert!(!status.installed);
        assert!(status.mod_available);
        assert!(!status.mod_installable, "mod not installable without game");
    }

    #[test]
    fn game_not_found_launcher_running() {
        let status = build_game_status(None, false, true);
        assert!(!status.installed);
        assert!(status.launcher_running);
        assert!(!status.game_running);
    }

    #[test]
    fn game_found_basic_fields() {
        let det = DetectedGame {
            installed_version: Some(12345),

            mod_deployed: false,
            mod_outdated: false,
            game_running: false,
        };
        let status = build_game_status(Some(&det), true, false);
        assert!(status.installed);
        assert_eq!(status.game_version, Some(12345));
        assert!(status.mod_available);
        assert!(status.mod_installable);
    }

    #[test]
    fn game_found_no_version() {
        let det = DetectedGame {
            installed_version: None,

            mod_deployed: false,
            mod_outdated: false,
            game_running: false,
        };
        let status = build_game_status(Some(&det), false, false);
        assert!(status.installed);
        assert!(status.game_version.is_none());
        assert!(!status.mod_available);
        assert!(!status.mod_installable, "no mod library bundled");
    }

    #[test]
    fn game_found_mod_deployed() {
        let det = DetectedGame {
            installed_version: Some(100),

            mod_deployed: true,
            mod_outdated: false,
            game_running: false,
        };
        let status = build_game_status(Some(&det), true, false);
        assert!(status.mod_deployed);
        assert!(!status.mod_outdated);
        // mod_removable is platform-dependent: true on Windows, false on macOS
        assert_eq!(status.mod_removable, cfg!(target_os = "windows"));
    }

    #[test]
    fn game_found_mod_outdated() {
        let det = DetectedGame {
            installed_version: Some(100),

            mod_deployed: false,
            mod_outdated: true,
            game_running: false,
        };
        let status = build_game_status(Some(&det), true, false);
        assert!(!status.mod_deployed);
        assert!(status.mod_outdated);
        assert_eq!(status.mod_removable, cfg!(target_os = "windows"));
    }

    #[test]
    fn game_found_mod_not_deployed_not_outdated() {
        let det = DetectedGame {
            installed_version: Some(100),

            mod_deployed: false,
            mod_outdated: false,
            game_running: false,
        };
        let status = build_game_status(Some(&det), true, false);
        assert!(!status.mod_deployed);
        assert!(!status.mod_outdated);
        assert!(!status.mod_removable);
    }

    #[test]
    fn game_found_game_running() {
        let det = DetectedGame {
            installed_version: Some(100),

            mod_deployed: true,
            mod_outdated: false,
            game_running: true,
        };
        let status = build_game_status(Some(&det), true, true);
        assert!(status.game_running);
        assert!(status.launcher_running);
    }

    // -- recompute_derived --

    /// Helper: build a GameStatus with base fields set, derived fields at default.
    fn make_status(f: impl FnOnce(&mut GameStatus)) -> GameStatus {
        let mut s = GameStatus::default();
        f(&mut s);
        recompute_derived(&mut s);
        s
    }

    #[test]
    fn update_available_when_remote_exceeds_installed() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(200);
        });
        assert!(s.update_available);
    }

    #[test]
    fn no_update_when_same_version() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(100);
        });
        assert!(!s.update_available);
    }

    #[test]
    fn no_update_when_remote_older() {
        let s = make_status(|s| {
            s.game_version = Some(200);
            s.remote_version = Some(100);
        });
        assert!(!s.update_available);
    }

    #[test]
    fn no_update_when_remote_missing() {
        let s = make_status(|s| {
            s.game_version = Some(100);
        });
        assert!(!s.update_available);
    }

    #[test]
    fn no_update_when_game_version_missing() {
        let s = make_status(|s| {
            s.remote_version = Some(200);
        });
        assert!(!s.update_available);
    }

    #[test]
    fn can_launch_all_conditions_met() {
        let s = make_status(|s| {
            s.mod_deployed = true;
        });
        assert!(s.can_launch);
    }

    #[test]
    fn can_launch_false_when_mod_not_deployed() {
        let s = make_status(|_| {});
        assert!(!s.can_launch);
    }

    #[test]
    fn can_launch_false_when_update_available() {
        let s = make_status(|s| {
            s.mod_deployed = true;
            s.game_version = Some(100);
            s.remote_version = Some(200);
        });
        assert!(!s.can_launch);
    }

    #[test]
    fn can_launch_false_when_game_running() {
        let s = make_status(|s| {
            s.mod_deployed = true;
            s.game_running = true;
        });
        assert!(!s.can_launch);
    }

    #[test]
    fn can_launch_false_when_launcher_running() {
        let s = make_status(|s| {
            s.mod_deployed = true;
            s.launcher_running = true;
        });
        assert!(!s.can_launch);
    }

    #[test]
    fn can_install_mod_all_conditions_met() {
        let s = make_status(|s| {
            s.mod_installable = true;
        });
        assert!(s.can_install_mod);
    }

    #[test]
    fn can_install_mod_false_when_not_installable() {
        let s = make_status(|_| {});
        assert!(!s.can_install_mod);
    }

    #[test]
    fn can_install_mod_false_when_update_available() {
        let s = make_status(|s| {
            s.mod_installable = true;
            s.game_version = Some(100);
            s.remote_version = Some(200);
        });
        assert!(!s.can_install_mod);
    }

    #[test]
    fn can_install_mod_false_when_game_running() {
        let s = make_status(|s| {
            s.mod_installable = true;
            s.game_running = true;
        });
        assert!(!s.can_install_mod);
    }

    #[test]
    fn can_remove_mod_when_removable_nothing_running() {
        let s = make_status(|s| {
            s.mod_removable = true;
        });
        assert!(s.can_remove_mod);
    }

    #[test]
    fn can_remove_mod_false_when_game_running() {
        let s = make_status(|s| {
            s.mod_removable = true;
            s.game_running = true;
        });
        assert!(!s.can_remove_mod);
    }

    #[test]
    fn can_launch_updater_when_update_available() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(200);
        });
        assert!(s.can_launch_updater);
    }

    #[test]
    fn can_launch_updater_false_when_launcher_running() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(200);
            s.launcher_running = true;
        });
        assert!(!s.can_launch_updater);
    }

    #[test]
    fn should_block_quit_when_game_started_by_us_and_running() {
        let s = make_status(|s| {
            s.game_started_by_us = true;
            s.game_running = true;
        });
        assert!(s.should_block_quit);
    }

    #[test]
    fn should_block_quit_when_launcher_started_by_us_and_running() {
        let s = make_status(|s| {
            s.launcher_started_by_us = true;
            s.launcher_running = true;
        });
        assert!(s.should_block_quit);
    }

    #[test]
    fn should_not_block_quit_when_process_not_started_by_us() {
        let s = make_status(|s| {
            s.game_running = true;
            s.launcher_running = true;
        });
        assert!(!s.should_block_quit);
    }

    #[test]
    fn should_not_block_quit_when_started_process_exited() {
        let s = make_status(|s| {
            s.game_started_by_us = true;
            s.game_running = false;
        });
        assert!(!s.should_block_quit);
    }

    #[test]
    fn version_check_class_warn_when_update_available() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(200);
        });
        assert_eq!(s.version_check_class, "warn");
    }

    #[test]
    fn version_check_class_ok_when_up_to_date() {
        let s = make_status(|s| {
            s.game_version = Some(100);
            s.remote_version = Some(100);
        });
        assert_eq!(s.version_check_class, "ok");
    }

    #[test]
    fn version_check_class_neutral_when_check_failed() {
        let s = make_status(|s| {
            s.update_check_failed = true;
        });
        assert_eq!(s.version_check_class, "neutral");
    }

    #[test]
    fn version_check_class_neutral_when_no_remote() {
        let s = make_status(|_| {});
        assert_eq!(s.version_check_class, "neutral");
    }
}
