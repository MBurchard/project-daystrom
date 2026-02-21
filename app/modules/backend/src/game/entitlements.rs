use std::fs;
use std::path::Path;
use std::process::Command;

use crate::use_log;

use_log!("Entitlements");

/// The four macOS entitlements the game executable needs for DYLD-based mod injection.
const REQUIRED: [&str; 4] = [
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.allow-unsigned-executable-memory",
    "com.apple.security.cs.disable-library-validation",
    "com.apple.security.get-task-allow",
];

/// Result of checking the game executable's code-signing entitlements.
pub struct EntitlementStatus {
    /// Entitlement keys that are present and set to `true`.
    pub granted: Vec<&'static str>,
    /// Entitlement keys that are absent or not `true`.
    pub missing: Vec<&'static str>,
}

impl EntitlementStatus {
    /// Returns `true` when all four required entitlements are granted.
    pub fn all_granted(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Check whether a plist XML fragment contains `<key>{key}</key>` followed by `<true/>`.
fn has_entitlement(xml: &str, key: &str) -> bool {
    let needle = format!("<key>{key}</key>");
    let Some(pos) = xml.find(&needle) else {
        return false;
    };
    xml[pos + needle.len()..].trim_start().starts_with("<true/>")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.allow-dyld-environment-variables</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.get-task-allow</key>
    <true/>
</dict>
</plist>"#;

    #[test]
    fn has_entitlement_present_and_true() {
        assert!(has_entitlement(
            FULL_PLIST,
            "com.apple.security.cs.allow-dyld-environment-variables",
        ));
    }

    #[test]
    fn has_entitlement_present_but_false() {
        let xml = r#"<dict>
    <key>com.apple.security.get-task-allow</key>
    <false/>
</dict>"#;
        assert!(!has_entitlement(xml, "com.apple.security.get-task-allow"));
    }

    #[test]
    fn has_entitlement_missing_key() {
        assert!(!has_entitlement(FULL_PLIST, "com.apple.security.app-sandbox"));
    }

    #[test]
    fn has_entitlement_empty_xml() {
        assert!(!has_entitlement("", "com.apple.security.get-task-allow"));
    }

    #[test]
    fn has_entitlement_key_without_value() {
        let xml = "<dict><key>com.apple.security.get-task-allow</key></dict>";
        assert!(!has_entitlement(xml, "com.apple.security.get-task-allow"));
    }

    #[test]
    fn has_entitlement_tolerates_whitespace_variants() {
        // Value on same line as key
        let xml = "<key>com.apple.security.get-task-allow</key><true/>";
        assert!(has_entitlement(xml, "com.apple.security.get-task-allow"));

        // Extra whitespace / newlines between key and value
        let xml = "<key>com.apple.security.get-task-allow</key>\n\t\t<true/>";
        assert!(has_entitlement(xml, "com.apple.security.get-task-allow"));
    }
}

/// Query the code signature of `executable` and check which of the four
/// required mod-injection entitlements are present.
pub fn check(executable: &Path) -> EntitlementStatus {
    log_debug!("Checking entitlements on {}", executable.display());

    let output = Command::new("codesign")
        .args(["-d", "--entitlements", ":-", "--xml"])
        .arg(executable)
        .output();

    let xml = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log_debug!("codesign failed: {stderr}");
            return EntitlementStatus { granted: vec![], missing: REQUIRED.to_vec() };
        }
        Err(e) => {
            log_debug!("Could not run codesign: {e}");
            return EntitlementStatus { granted: vec![], missing: REQUIRED.to_vec() };
        }
    };

    let mut granted = vec![];
    let mut missing = vec![];

    for &key in &REQUIRED {
        if has_entitlement(&xml, key) {
            granted.push(key);
        } else {
            missing.push(key);
        }
    }

    EntitlementStatus { granted, missing }
}

/// XML plist containing the four required entitlements for mod injection.
const ENTITLEMENTS_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.allow-dyld-environment-variables</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.get-task-allow</key>
    <true/>
</dict>
</plist>"#;

/// Re-sign the game executable with the four required entitlements for mod injection.
///
/// Writes a temporary plist file, runs `codesign --force --sign -` with it, then verifies
/// the result by calling `check()` again.
pub fn patch(executable: &Path) -> Result<(), String> {
    log_info!("Patching entitlements on {}", executable.display());

    let plist_path = std::env::temp_dir().join("skynet-entitlements.plist");

    fs::write(&plist_path, ENTITLEMENTS_PLIST)
        .map_err(|e| format!("Failed to write entitlements plist: {e}"))?;

    let output = Command::new("codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(&plist_path)
        .arg(executable)
        .output()
        .map_err(|e| format!("Failed to run codesign: {e}"))?;

    // Clean up temp file regardless of outcome
    let _ = fs::remove_file(&plist_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("codesign failed: {stderr}"));
    }

    // Verify the patch worked
    let status = check(executable);
    if status.all_granted() {
        log_info!("Entitlements patched successfully");
        Ok(())
    } else {
        let names: Vec<_> = status.missing.iter()
            .map(|k| k.strip_prefix("com.apple.security.").unwrap_or(k))
            .collect();
        Err(format!("Entitlements still missing after patch: {}", names.join(", ")))
    }
}
