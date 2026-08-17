//! Persistent, signature-verified updater packages used for one-generation rollback.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Manager;
use tauri_plugin_updater::{Update, UpdaterExt};

use super::install::{
    RELEASE_HOST, RELEASE_OWNER, RELEASE_REPOSITORY, UPDATE_DOWNLOAD_TIMEOUT, is_trusted_release_download_url,
};

/// Directory below the application-data root containing signed updater packages.
const CACHE_DIRECTORY: &str = "daystrom-updates";

/// Package file extension used by the private cache.
const PACKAGE_EXTENSION: &str = "package";

/// Maximum duration allowed for fetching an immutable release manifest.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One signed updater package and the trusted remote metadata needed to reverify it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct CachedPackage {
    /// Version described by the immutable release manifest.
    version: String,
    /// Trusted GitHub release URL from which the package originated.
    url: String,
    /// Detached Tauri updater signature for the package.
    signature: String,
    /// SHA-256 digest used as the content-addressed local file name.
    sha256: String,
}

/// Incomplete update transaction retained across the updater-driven restart.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct PendingTransition {
    /// Version running when installation was requested.
    installed_from_version: String,
    /// Signed direct predecessor of the target release.
    rollback: CachedPackage,
    /// Newly installed version that becomes the current cached package after success.
    to: CachedPackage,
}

/// Durable pointers into the content-addressed package directory.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
struct CacheState {
    /// Monotonic generation used to choose the newest valid journal slot.
    generation: u64,
    /// Verified package for the version currently installed.
    current: Option<CachedPackage>,
    /// Exactly one verified predecessor available for rollback.
    rollback: Option<CachedPackage>,
    /// Verified predecessor prepared for an update that has not started installation.
    prepared_rollback: Option<CachedPackage>,
    /// Update awaiting installation or first startup of its target version.
    pending: Option<PendingTransition>,
}

/// Signed release metadata that binds one successor to its sole predecessor.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RollbackMetadata {
    /// Metadata schema version.
    schema: u8,
    /// Release that embeds this predecessor relationship.
    successor_version: String,
    /// Sole version accepted as the successor's rollback target.
    predecessor_version: String,
    /// Signed platform package locations and signatures from the predecessor manifest.
    platforms: BTreeMap<String, SignedPlatform>,
}

/// One platform entry copied from the predecessor's immutable update manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedPlatform {
    /// Detached package signature.
    signature: String,
    /// Trusted release-package URL.
    url: String,
}

/// Signed rollback payload embedded into a successor's update manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RollbackEnvelope {
    /// Exact signed metadata bytes transported as a JSON string.
    metadata: String,
    /// Tauri-compatible signature over the metadata string's UTF-8 bytes.
    signature: String,
}

/// Verified predecessor identity and platform package announced by a target release.
struct VerifiedPredecessor {
    /// Direct predecessor version signed by the target release.
    version: String,
    /// Platform package metadata signed by the target release.
    platform: SignedPlatform,
}

/// Reconcile a pending cache transaction with the application version that actually started.
pub(super) fn reconcile_after_start(app: &tauri::AppHandle) {
    let current_version = app.package_info().version.to_string();
    let result = (|| {
        let directory = cache_directory(app)?;
        let mut state = read_state(&directory)?;
        if !reconcile_state(&mut state, &current_version) {
            return Ok(());
        }
        write_state(&directory, &mut state)?;
        remove_unreferenced_packages(&directory, &state)
    })();

    match result {
        Ok(()) => log_debug!("Reconciled Daystrom update cache for version {current_version}"),
        Err(error) => log_warn!("Could not reconcile Daystrom update cache: {error}"),
    }
}

/// Ensure the target release's direct predecessor is cached as its sole rollback target.
pub(super) async fn retain_rollback_package(
    app: &tauri::AppHandle,
    target_update: &Update,
    on_chunk: impl FnMut(usize, Option<u64>),
) -> Result<CachedPackage, String> {
    let current_version = app.package_info().version.to_string();
    let predecessor = verified_predecessor(app, target_update)?;
    let mut update = release_update(app, &predecessor.version).await?;
    if update.version != predecessor.version {
        return Err(format!(
            "immutable release manifest announced {} instead of {}",
            update.version, predecessor.version
        ));
    }
    if !is_trusted_release_download_url(&update.download_url, &predecessor.version) {
        return Err(format!("untrusted predecessor package URL {}", update.download_url));
    }
    if update.download_url.as_str() != predecessor.platform.url || update.signature != predecessor.platform.signature {
        return Err("immutable predecessor manifest does not match signed rollback metadata".to_string());
    }

    let directory = cache_directory(app)?;
    let mut state = read_state(&directory)?;
    let public_key = updater_public_key(app)?;
    if let Some(package) = cached_packages(&state).find(|package| {
        package_matches_update(package, &update) && verify_cached_package(&directory, package, public_key).is_ok()
    }) {
        log_info!("Reusing verified cached Daystrom {} package for rollback", predecessor.version);
        return Ok(package.clone());
    }

    update.timeout = Some(UPDATE_DOWNLOAD_TIMEOUT);
    log_info!(
        "Downloading signed Daystrom {} package for rollback retention",
        predecessor.version
    );
    let bytes = update
        .download(on_chunk, || {})
        .await
        .map_err(|error| format!("predecessor package download or verification failed: {error}"))?;
    let package = store_verified_package(&directory, &update, &bytes)?;
    if predecessor.version == current_version {
        state.current = Some(package.clone());
    } else {
        state.prepared_rollback = Some(package.clone());
    }
    write_state(&directory, &mut state)?;
    remove_unreferenced_packages(&directory, &state)?;
    Ok(package)
}

/// Verify the target manifest's signed direct predecessor for the running platform.
fn verified_predecessor(app: &tauri::AppHandle, target_update: &Update) -> Result<VerifiedPredecessor, String> {
    let envelope: RollbackEnvelope = target_update
        .raw_json
        .get("rollback")
        .cloned()
        .ok_or_else(|| "target manifest has no signed rollback metadata".to_string())
        .and_then(|value| {
            serde_json::from_value(value).map_err(|error| format!("invalid rollback metadata: {error}"))
        })?;
    verify_encoded_signature(envelope.metadata.as_bytes(), &envelope.signature, updater_public_key(app)?)?;
    let metadata: RollbackMetadata = serde_json::from_str(&envelope.metadata)
        .map_err(|error| format!("invalid signed rollback metadata: {error}"))?;
    if metadata.schema != 1 {
        return Err(format!("unsupported rollback metadata schema {}", metadata.schema));
    }
    if metadata.successor_version != target_update.version {
        return Err(format!(
            "rollback metadata names successor {} instead of {}",
            metadata.successor_version, target_update.version
        ));
    }
    let platform_target = updater_platform_target()?;
    let platform = metadata
        .platforms
        .into_iter()
        .find_map(|(target, platform)| (target == platform_target).then_some(platform))
        .ok_or_else(|| format!("rollback metadata has no {platform_target} package"))?;
    Ok(VerifiedPredecessor {
        version: metadata.predecessor_version,
        platform,
    })
}

/// Return the static-manifest target used by supported Daystrom release builds.
fn updater_platform_target() -> Result<&'static str, String> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("darwin-aarch64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Ok("darwin-x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("windows-x86_64")
    } else {
        Err("this platform has no supported Daystrom updater package".to_string())
    }
}

/// Persist a verified target package and the predecessor transition before installation starts.
pub(super) fn stage_update(
    app: &tauri::AppHandle,
    rollback_package: CachedPackage,
    update: &Update,
    bytes: &[u8],
) -> Result<(), String> {
    if !is_trusted_release_download_url(&update.download_url, &update.version) {
        return Err(format!("untrusted staged package URL {}", update.download_url));
    }
    let directory = cache_directory(app)?;
    let target = store_verified_package(&directory, update, bytes)?;
    let mut state = read_state(&directory)?;
    state.prepared_rollback = None;
    state.pending = Some(PendingTransition {
        installed_from_version: app.package_info().version.to_string(),
        rollback: rollback_package,
        to: target,
    });
    write_state(&directory, &mut state)?;
    remove_unreferenced_packages(&directory, &state)
}

/// Clear an update transaction after the platform installer returned an error.
pub(super) fn abort_pending_update(app: &tauri::AppHandle) -> Result<(), String> {
    let directory = cache_directory(app)?;
    let mut state = read_state(&directory)?;
    let Some(pending) = state.pending.take() else {
        return Ok(());
    };
    state.prepared_rollback = Some(pending.rollback);
    write_state(&directory, &mut state)?;
    remove_unreferenced_packages(&directory, &state)
}

/// Query an immutable release manifest, accepting its exact version regardless of the installed version.
async fn release_update(app: &tauri::AppHandle, version: &str) -> Result<Update, String> {
    let endpoint = tauri::Url::parse(&format!(
        "https://{RELEASE_HOST}/{RELEASE_OWNER}/{RELEASE_REPOSITORY}/releases/download/{version}/latest.json"
    ))
    .map_err(|error| format!("invalid release manifest URL: {error}"))?;
    let expected = version.to_string();
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| format!("invalid release manifest endpoint: {error}"))?
        .version_comparator(move |_current, remote| remote.version.to_string() == expected)
        .timeout(MANIFEST_TIMEOUT)
        .build()
        .map_err(|error| format!("could not build release-manifest updater: {error}"))?;
    updater
        .check()
        .await
        .map_err(|error| format!("release manifest request failed: {error}"))?
        .ok_or_else(|| format!("release manifest did not expose Daystrom {version}"))
}

/// Iterate over all durable cache slots that may already contain a requested package.
fn cached_packages(state: &CacheState) -> impl Iterator<Item = &CachedPackage> {
    [state.current.as_ref(), state.rollback.as_ref(), state.prepared_rollback.as_ref()]
        .into_iter()
        .flatten()
}

/// Resolve the private update-cache directory for this application.
fn cache_directory(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join(CACHE_DIRECTORY))
        .map_err(|error| format!("could not resolve application-data directory: {error}"))
}

/// Read the newest valid state journal, falling back to an empty cache.
fn read_state(directory: &Path) -> Result<CacheState, String> {
    let mut states = ["state-a.json", "state-b.json"]
        .into_iter()
        .filter_map(|name| {
            let path = directory.join(name);
            let content = fs::read(&path).ok()?;
            match serde_json::from_slice::<CacheState>(&content) {
                Ok(state) => Some(state),
                Err(error) => {
                    log_warn!("Ignoring invalid update cache journal {}: {error}", path.display());
                    None
                }
            }
        })
        .collect::<Vec<_>>();
    states.sort_by_key(|state| state.generation);
    Ok(states.pop().unwrap_or_default())
}

/// Atomically write the next state generation while preserving the previous valid slot.
fn write_state(directory: &Path, state: &mut CacheState) -> Result<(), String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create update cache {}: {error}", directory.display()))?;
    state.generation = state
        .generation
        .checked_add(1)
        .ok_or_else(|| "update cache generation overflowed".to_string())?;
    let slot = if state.generation.is_multiple_of(2) { "state-a.json" } else { "state-b.json" };
    let path = directory.join(slot);
    let temporary = directory.join(format!("{slot}.tmp"));
    let bytes =
        serde_json::to_vec_pretty(state).map_err(|error| format!("could not serialize update cache: {error}"))?;
    write_synced_file(&temporary, &bytes)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    fs::rename(&temporary, &path).map_err(|error| format!("could not commit {}: {error}", path.display()))
}

/// Store verified package bytes under their SHA-256 digest without overwriting good data.
fn store_verified_package(directory: &Path, update: &Update, bytes: &[u8]) -> Result<CachedPackage, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("could not create update cache {}: {error}", directory.display()))?;
    let sha256 = hex_digest(bytes);
    let package = CachedPackage {
        version: update.version.clone(),
        url: update.download_url.to_string(),
        signature: update.signature.clone(),
        sha256,
    };
    let path = package_path(directory, &package);
    if fs::read(&path).is_ok_and(|existing| hex_digest(&existing) == package.sha256) {
        return Ok(package);
    }
    let temporary = path.with_extension("package.tmp");
    write_synced_file(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("could not replace {}: {error}", path.display()))?;
    }
    fs::rename(&temporary, &path).map_err(|error| format!("could not commit {}: {error}", path.display()))?;
    Ok(package)
}

/// Write and fsync one file before it becomes visible through a rename.
fn write_synced_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = File::create(path).map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("could not sync {}: {error}", path.display()))
}

/// Verify that cached metadata still exactly matches its immutable remote manifest.
fn package_matches_update(package: &CachedPackage, update: &Update) -> bool {
    package.version == update.version
        && package.url == update.download_url.as_str()
        && package.signature == update.signature
}

/// Reverify cached bytes against the embedded updater key and their content-addressed name.
fn verify_cached_package(directory: &Path, package: &CachedPackage, public_key: &str) -> Result<(), String> {
    let bytes = fs::read(package_path(directory, package))
        .map_err(|error| format!("could not read cached Daystrom {} package: {error}", package.version))?;
    if hex_digest(&bytes) != package.sha256 {
        return Err(format!("cached Daystrom {} package digest does not match", package.version));
    }
    verify_encoded_signature(&bytes, &package.signature, public_key)
}

/// Return the updater public key embedded in Tauri's runtime configuration.
fn updater_public_key(app: &tauri::AppHandle) -> Result<&str, String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "updater public key is missing from application configuration".to_string())
}

/// Verify Tauri's base64-wrapped Minisign signature format.
fn verify_encoded_signature(data: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    let public_key = decode_text(public_key, "updater public key")?;
    let public_key =
        PublicKey::decode(&public_key).map_err(|error| format!("could not parse updater public key: {error}"))?;
    let signature = decode_text(signature, "updater signature")?;
    let signature =
        Signature::decode(&signature).map_err(|error| format!("could not parse updater signature: {error}"))?;
    public_key
        .verify(data, &signature, true)
        .map_err(|error| format!("cached updater signature verification failed: {error}"))
}

/// Decode a base64-encoded UTF-8 value used by Tauri's updater configuration.
fn decode_text(encoded: &str, description: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("could not decode {description}: {error}"))?;
    String::from_utf8(bytes).map_err(|error| format!("decoded {description} is not UTF-8: {error}"))
}

/// Calculate a lowercase SHA-256 digest without introducing a second encoding dependency.
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Resolve the content-addressed file path for one cached updater package.
fn package_path(directory: &Path, package: &CachedPackage) -> PathBuf {
    directory.join(format!("{}.{}", package.sha256, PACKAGE_EXTENSION))
}

/// Apply or abandon an update transaction based on the version that started.
fn reconcile_state(state: &mut CacheState, running_version: &str) -> bool {
    if let Some(pending) = state.pending.take() {
        if pending.to.version == running_version {
            state.rollback = Some(pending.rollback);
            state.current = Some(pending.to);
            state.prepared_rollback = None;
            return true;
        }
        if pending.installed_from_version == running_version {
            state.prepared_rollback = Some(pending.rollback);
            return true;
        }
    }
    if state.current.as_ref().is_some_and(|package| package.version == running_version) {
        return false;
    }
    if state.current.is_some() || state.rollback.is_some() {
        state.current = None;
        state.rollback = None;
        state.prepared_rollback = None;
        return true;
    }
    false
}

/// Delete package files that are no longer reachable from durable cache state.
fn remove_unreferenced_packages(directory: &Path, state: &CacheState) -> Result<(), String> {
    let mut retained = HashSet::new();
    for package in [state.current.as_ref(), state.rollback.as_ref(), state.prepared_rollback.as_ref()]
        .into_iter()
        .flatten()
        .chain(state.pending.iter().flat_map(|pending| [&pending.rollback, &pending.to]))
    {
        retained.insert(package.sha256.as_str());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("could not inspect update cache {}: {error}", directory.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not inspect update cache entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(PACKAGE_EXTENSION) {
            continue;
        }
        let digest = path.file_stem().and_then(|value| value.to_str()).unwrap_or_default();
        if !retained.contains(digest) {
            fs::remove_file(&path).map_err(|error| format!("could not remove {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal cached package for state-transition tests.
    fn package(version: &str) -> CachedPackage {
        CachedPackage {
            version: version.to_string(),
            url: format!("https://example.test/{version}"),
            signature: format!("signature-{version}"),
            sha256: format!("digest-{version}"),
        }
    }

    /// Build a skipped-release transition from installed A to target C with B as rollback.
    fn pending_transition_state() -> CacheState {
        CacheState {
            current: Some(package("0.9.0")),
            rollback: Some(package("0.8.0")),
            pending: Some(PendingTransition {
                installed_from_version: "0.9.0".to_string(),
                rollback: package("0.10.0"),
                to: package("0.11.0"),
            }),
            ..CacheState::default()
        }
    }

    #[test]
    fn skipped_release_promotes_target_predecessor_for_rollback() {
        let mut state = pending_transition_state();

        assert!(reconcile_state(&mut state, "0.11.0"));
        assert_eq!(state.current.as_ref().map(|value| value.version.as_str()), Some("0.11.0"));
        assert_eq!(state.rollback.as_ref().map(|value| value.version.as_str()), Some("0.10.0"));
        assert!(state.prepared_rollback.is_none());
        assert!(state.pending.is_none());
    }

    #[test]
    fn failed_update_keeps_existing_current_and_rollback() {
        let mut state = pending_transition_state();

        assert!(reconcile_state(&mut state, "0.9.0"));
        assert_eq!(state.current.as_ref().map(|value| value.version.as_str()), Some("0.9.0"));
        assert_eq!(state.rollback.as_ref().map(|value| value.version.as_str()), Some("0.8.0"));
        assert_eq!(
            state.prepared_rollback.as_ref().map(|value| value.version.as_str()),
            Some("0.10.0")
        );
        assert!(state.pending.is_none());
    }

    #[test]
    fn unrelated_manual_version_invalidates_cache_pointers() {
        let mut state = CacheState {
            current: Some(package("0.10.0")),
            rollback: Some(package("0.9.0")),
            prepared_rollback: Some(package("0.11.0")),
            ..CacheState::default()
        };

        assert!(reconcile_state(&mut state, "0.12.0"));
        assert!(state.current.is_none());
        assert!(state.rollback.is_none());
        assert!(state.prepared_rollback.is_none());
    }

    #[test]
    fn rollback_metadata_rejects_unknown_fields() {
        let json = concat!(
            r#"{"schema":1,"successorVersion":"0.10.0","predecessorVersion":"0.9.0","platforms":{"#,
            r#""darwin-aarch64":{"signature":"mac","url":"https://example.test/mac"}},"unexpected":true}"#,
        );

        assert!(serde_json::from_str::<RollbackMetadata>(json).is_err());
    }
}
