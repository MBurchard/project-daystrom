use std::fs;

use serde_json::Value;

fn main() {
    // Read the Tauri identifier from tauri.conf.json and expose it as a compile-time env var.
    // This avoids hardcoding the identifier in Rust source files.
    if let Ok(content) = fs::read_to_string("tauri.conf.json") {
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if let Some(id) = json.get("identifier").and_then(|v| v.as_str()) {
                println!("cargo:rustc-env=TAURI_IDENTIFIER={id}");
            }
        }
    }

    tauri_build::build();
}
