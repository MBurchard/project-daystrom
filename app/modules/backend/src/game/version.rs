use std::path::Path;
use std::process::Command;

use crate::use_log;

use_log!("Version");

/// Scopely project ID for Star Trek Fleet Command.
const PROJECT_ID: u32 = 152033;

/// Maximum time in seconds to wait for the Scopely update API response.
const CURL_TIMEOUT: u32 = 10;

// ---- Installed Version ----------------------------------------------------------

/// Read the installed game version from the `.version` file in the game directory.
///
/// The file contains a single line in the format `&game=<integer>`.
/// Returns `None` if the file is missing, empty, or has an unexpected format.
pub fn read_installed(install_dir: &Path) -> Option<u32> {
    let version_file = install_dir.join(".version");
    let content = std::fs::read_to_string(&version_file)
        .map_err(|e| log_debug!("Could not read .version file: {e}"))
        .ok()?;

    parse_version_string(&content)
}

/// Parse the version integer from a `.version` file content string.
///
/// Expected format: `&game=<integer>` (possibly with surrounding whitespace).
fn parse_version_string(content: &str) -> Option<u32> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("&game=") {
            return value.parse::<u32>().ok();
        }
    }
    None
}

// ---- Remote Version -------------------------------------------------------------

/// Fetch the latest game version from the Scopely update API.
///
/// Calls `curl -s --max-time 10` against the Scopely update endpoint.
/// Returns `Ok(Some(version))` when an update is available (remote version > 0),
/// `Ok(None)` when the game is up to date, or `Err(...)` on network/parse failures.
///
/// The API returns XML with a `<version>` element. When no update is available, the
/// version attribute is `-1` or the element is absent.
pub fn fetch_remote(installed: u32) -> Result<Option<u32>, String> {
    let platform = if cfg!(target_os = "macos") { "mac_os" } else { "windows" };
    let url = format!(
        "https://gus.xsolla.com/updates?version={installed}&project_id={PROJECT_ID}\
         &region=&platform={platform}"
    );

    let output = Command::new("curl")
        .args(["-s", "--max-time", &CURL_TIMEOUT.to_string(), &url])
        .output()
        .map_err(|e| format!("Failed to run curl: {e}"))?;

    if !output.status.success() {
        return Err(format!("curl exited with status {}", output.status));
    }

    let body = String::from_utf8_lossy(&output.stdout);
    parse_update_response(&body)
}

/// Parse the Scopely update API XML response to extract the remote game version.
///
/// Looks for `type="version"` and `version="<N>"` attributes in the XML.
/// Returns `Ok(Some(N))` when N > 0 (update available), `Ok(None)` when
/// N <= 0 or the version element is absent (no update).
fn parse_update_response(xml: &str) -> Result<Option<u32>, String> {
    // Look for an element with type="version" and extract its version="<N>" attribute
    for line in xml.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("type=\"version\"") {
            continue;
        }
        // Extract version="<N>" from this line
        if let Some(start) = trimmed.find("version=\"") {
            let after_prefix = &trimmed[start + 9..];
            if let Some(end) = after_prefix.find('"') {
                let version_str = &after_prefix[..end];
                // Parse as signed first: -1 means "no update"
                return match version_str.parse::<i32>() {
                    Ok(v) if v > 0 => Ok(Some(v as u32)),
                    Ok(_) => Ok(None),
                    Err(e) => Err(format!("Failed to parse version '{version_str}': {e}")),
                };
            }
        }
    }

    // No version element found — no update available
    Ok(None)
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_version_string --

    #[test]
    fn parse_version_normal() {
        assert_eq!(parse_version_string("&game=139"), Some(139));
    }

    #[test]
    fn parse_version_with_whitespace() {
        assert_eq!(parse_version_string("  &game=139  \n"), Some(139));
    }

    #[test]
    fn parse_version_empty() {
        assert_eq!(parse_version_string(""), None);
    }

    #[test]
    fn parse_version_missing_prefix() {
        assert_eq!(parse_version_string("game=139"), None);
    }

    #[test]
    fn parse_version_invalid_number() {
        assert_eq!(parse_version_string("&game=abc"), None);
    }

    #[test]
    fn parse_version_multiple_lines() {
        assert_eq!(parse_version_string("&launcher=5\n&game=140\n"), Some(140));
    }

    // -- parse_update_response --

    #[test]
    fn remote_update_available() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<updates>
  <update type="version" version="140" />
</updates>"#;
        assert_eq!(parse_update_response(xml).unwrap(), Some(140));
    }

    #[test]
    fn remote_no_update() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<updates>
  <update type="version" version="-1" />
</updates>"#;
        assert_eq!(parse_update_response(xml).unwrap(), None);
    }

    #[test]
    fn remote_no_version_element() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<updates></updates>"#;
        assert_eq!(parse_update_response(xml).unwrap(), None);
    }

    #[test]
    fn remote_version_zero() {
        let xml = r#"<updates><update type="version" version="0" /></updates>"#;
        assert_eq!(parse_update_response(xml).unwrap(), None);
    }

    #[test]
    fn remote_malformed_version() {
        let xml = r#"<updates><update type="version" version="abc" /></updates>"#;
        assert!(parse_update_response(xml).is_err());
    }

    // -- read_installed (filesystem) --

    #[test]
    fn read_installed_normal() {
        let dir = std::env::temp_dir().join("daystrom_test_version_read");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".version"), "&game=139").unwrap();

        assert_eq!(read_installed(&dir), Some(139));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_installed_missing_file() {
        let dir = std::env::temp_dir().join("daystrom_test_version_missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(read_installed(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_installed_bad_format() {
        let dir = std::env::temp_dir().join("daystrom_test_version_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".version"), "garbage content").unwrap();

        assert_eq!(read_installed(&dir), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
