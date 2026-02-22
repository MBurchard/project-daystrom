use std::env;
use std::fs;

use serde_json::Value;

/// Path to the root package.json (relative to the backend crate directory).
const ROOT_PACKAGE_JSON: &str = "../../../package.json";

fn main() {
    // Ensure Cargo recompiles when the root package.json changes (version source of truth).
    println!("cargo:rerun-if-changed={ROOT_PACKAGE_JSON}");

    // Read the Tauri identifier from tauri.conf.json and expose it as a compile-time env var.
    // This avoids hardcoding the identifier in Rust source files.
    if let Ok(content) = fs::read_to_string("tauri.conf.json") {
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if let Some(id) = json.get("identifier").and_then(|v| v.as_str()) {
                println!("cargo:rustc-env=TAURI_IDENTIFIER={id}");
            }
        }
    }

    check_version_sync();

    tauri_build::build();
}

/// Warn at build time if the Cargo.toml version drifts from the root package.json.
fn check_version_sync() {
    let cargo_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();

    let pkg_version = fs::read_to_string(ROOT_PACKAGE_JSON)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|json| json.get("version").and_then(|v| v.as_str()).map(String::from));

    if let Some(pkg_version) = pkg_version {
        if cargo_version != pkg_version {
            println!(
                "cargo:warning=Version mismatch: Cargo.toml has {cargo_version}, \
                 root package.json has {pkg_version} — please update Cargo.toml"
            );
        }
    }
}
