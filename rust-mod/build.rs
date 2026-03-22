use std::env;
use std::fs;

/// Path to the root package.json (version source of truth).
const ROOT_PACKAGE_JSON: &str = "../package.json";

/// Read the Tauri bundle identifier from `tauri.conf.json`, expose it as compile-time env var,
/// and check that the crate version matches the root package.json.
fn main() {
    let conf_path = "../app/modules/backend/tauri.conf.json";
    println!("cargo:rerun-if-changed={conf_path}");
    println!("cargo:rerun-if-changed={ROOT_PACKAGE_JSON}");

    let content = fs::read_to_string(conf_path)
        .unwrap_or_else(|e| panic!("Cannot read {conf_path}: {e}"));

    let identifier = extract_identifier(&content)
        .unwrap_or_else(|| panic!("No \"identifier\" field found in {conf_path}"));

    println!("cargo:rustc-env=TAURI_IDENTIFIER={identifier}");

    check_version_sync();

    // On Windows, link the version.def file so our DLL exports the version.dll API symbols.
    // The actual forwarding is handled at runtime in src/proxy.rs.
    #[cfg(target_os = "windows")]
    {
        let def_path = std::path::Path::new("version.def").canonicalize()
            .expect("version.def not found in rust-mod root");
        println!("cargo:rerun-if-changed=version.def");
        println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path.display());
    }
}

/// Extract a quoted JSON string value by key name (without a full JSON parser).
///
/// Finds `"key": "value"` and returns the value between the quotes.
fn extract_json_string(json: &str, key_name: &str) -> Option<String> {
    let key = format!("\"{key_name}\"");
    let pos = json.find(&key)? + key.len();
    let rest = &json[pos..];
    let open = rest.find('"')? + 1;
    let close = open + rest[open..].find('"')?;
    Some(rest[open..close].to_string())
}

/// Extract the `"identifier"` value from tauri.conf.json.
fn extract_identifier(json: &str) -> Option<String> {
    extract_json_string(json, "identifier")
}

/// Warn at build time if the Cargo.toml version drifts from the root package.json.
fn check_version_sync() {
    let cargo_version = env::var("CARGO_PKG_VERSION").unwrap_or_default();

    let pkg_version = fs::read_to_string(ROOT_PACKAGE_JSON)
        .ok()
        .and_then(|content| extract_json_string(&content, "version"));

    if let Some(pkg_version) = pkg_version.filter(|v| v != &cargo_version) {
        println!(
            "cargo:warning=Version mismatch: rust-mod Cargo.toml has {cargo_version}, \
             root package.json has {pkg_version} — please keep them in sync"
        );
    }
}
