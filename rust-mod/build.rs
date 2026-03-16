use std::fs;

/// Read the Tauri bundle identifier from `tauri.conf.json` and expose it as the compile-time environment
/// variable `TAURI_IDENTIFIER`.
fn main() {
    let conf_path = "../app/modules/backend/tauri.conf.json";
    println!("cargo:rerun-if-changed={conf_path}");

    let content = fs::read_to_string(conf_path)
        .unwrap_or_else(|e| panic!("Cannot read {conf_path}: {e}"));

    let identifier = extract_identifier(&content)
        .unwrap_or_else(|| panic!("No \"identifier\" field found in {conf_path}"));

    println!("cargo:rustc-env=TAURI_IDENTIFIER={identifier}");
}

/// Extract the `"identifier"` value from a JSON string without pulling in a full JSON parser.
/// Looks for `"identifier": "..."` and returns the content between the quotes.
fn extract_identifier(json: &str) -> Option<String> {
    let key = "\"identifier\"";
    let pos = json.find(key)? + key.len();
    let rest = &json[pos..];
    let open = rest.find('"')? + 1;
    let close = open + rest[open..].find('"')?;
    Some(rest[open..close].to_string())
}
