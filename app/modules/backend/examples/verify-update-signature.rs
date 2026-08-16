//! Release-only verifier for Tauri updater artifacts.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;

#[derive(Deserialize)]
struct TauriConfig {
    plugins: PluginsConfig,
}

#[derive(Deserialize)]
struct PluginsConfig {
    updater: UpdaterConfig,
}

#[derive(Deserialize)]
struct UpdaterConfig {
    pubkey: String,
}

/// Decode a base64-encoded UTF-8 value used by Tauri's updater configuration.
fn decode_text(encoded: &str, description: &str) -> Result<String, String> {
    let bytes = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("Failed to decode {description}: {error}"))?;
    String::from_utf8(bytes).map_err(|error| format!("Decoded {description} is not valid UTF-8: {error}"))
}

/// Verify updater bytes using the same double-encoded Minisign values consumed by Tauri.
fn verify_encoded_signature(data: &[u8], encoded_signature: &str, encoded_public_key: &str) -> Result<(), String> {
    let public_key_text = decode_text(encoded_public_key, "updater public key")?;
    let public_key =
        PublicKey::decode(&public_key_text).map_err(|error| format!("Failed to parse updater public key: {error}"))?;
    let signature_text = decode_text(encoded_signature, "updater signature")?;
    let signature =
        Signature::decode(&signature_text).map_err(|error| format!("Failed to parse updater signature: {error}"))?;

    public_key
        .verify(data, &signature, true)
        .map_err(|error| format!("Updater signature verification failed: {error}"))
}

/// Read an updater artifact, its signature, and the embedded Tauri public key and verify them.
fn verify_update_artifact(artifact_path: &Path, signature_path: &Path, config_path: &Path) -> Result<(), String> {
    let artifact = fs::read(artifact_path)
        .map_err(|error| format!("Failed to read updater artifact {}: {error}", artifact_path.display()))?;
    let encoded_signature = fs::read_to_string(signature_path)
        .map_err(|error| format!("Failed to read updater signature {}: {error}", signature_path.display()))?;
    let config_text = fs::read_to_string(config_path)
        .map_err(|error| format!("Failed to read Tauri config {}: {error}", config_path.display()))?;
    let config: TauriConfig =
        serde_json::from_str(&config_text).map_err(|error| format!("Failed to parse Tauri config: {error}"))?;

    verify_encoded_signature(&artifact, &encoded_signature, &config.plugins.updater.pubkey)
}

/// Parse command-line arguments and verify one final updater artifact.
fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let artifact_path = arguments.next().map(PathBuf::from);
    let signature_path = arguments.next().map(PathBuf::from);
    let config_path = arguments.next().map(PathBuf::from);

    let (Some(artifact_path), Some(signature_path), Some(config_path)) = (artifact_path, signature_path, config_path)
    else {
        return Err("Usage: verify-update-signature <artifact> <signature> <tauri-config>".to_string());
    };
    if arguments.next().is_some() {
        return Err("Usage: verify-update-signature <artifact> <signature> <tauri-config>".to_string());
    }

    verify_update_artifact(&artifact_path, &signature_path, &config_path)?;
    println!("Verified updater signature for {}", artifact_path.display());
    Ok(())
}

/// Exit unsuccessfully when updater signature verification fails.
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\n\
        RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\n\
        RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\n\
        trusted comment: timestamp:1555779966\tfile:test\n\
        QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";

    /// Encode Minisign text as it appears in Tauri configuration and manifests.
    fn encode_tauri_value(value: &str) -> String {
        STANDARD.encode(value)
    }

    /// Create a valid Minisign public key with a different key identifier.
    fn different_public_key() -> String {
        let mut key = Vec::from(*b"Ed");
        key.extend_from_slice(&[0; 8]);
        key.extend_from_slice(&[0; 32]);
        format!("untrusted comment: different test key\n{}", STANDARD.encode(key))
    }

    #[test]
    fn accepts_valid_tauri_encoded_signature() {
        verify_encoded_signature(b"test", &encode_tauri_value(SIGNATURE), &encode_tauri_value(PUBLIC_KEY))
            .expect("valid signature should verify");
    }

    #[test]
    fn rejects_signature_from_a_different_key() {
        let result = verify_encoded_signature(
            b"test",
            &encode_tauri_value(SIGNATURE),
            &encode_tauri_value(&different_public_key()),
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_modified_artifact() {
        let result =
            verify_encoded_signature(b"modified", &encode_tauri_value(SIGNATURE), &encode_tauri_value(PUBLIC_KEY));

        assert!(result.is_err());
    }
}
