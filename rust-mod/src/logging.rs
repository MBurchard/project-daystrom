use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};
use std::thread;

use chrono::{Local, NaiveDate};
use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::Deserialize;

use crate::TAURI_IDENTIFIER;
use crate::profile_protocol::{INITIAL_PROFILE_STEM, NEW_ACCOUNT_PROFILE_STEM, PROFILE_ENV_VAR};

/// Number of days to keep archived log files.
const MAX_LOG_AGE_DAYS: i64 = 30;

/// Maximum number of bytes to read from the end of a log file when looking for the last timestamp.
const TAIL_READ_SIZE: u64 = 4096;

/// Global logger instance, initialized once via `init()`.
static LOGGER: ModLogger = ModLogger;

/// Channel sender for the background writer thread.
static LOG_SENDER: OnceLock<Sender<LogMessage>> = OnceLock::new();

/// Messages sent to the background writer thread.
enum LogMessage {
    /// A pre-formatted log line to write.
    Line(String),
    /// Rename the log file to a new base name (e.g. "mod_106_Nabor").
    Rename(String),
}

// ---- Log path resolution --------------------------------------------------

/// Determine the platform-specific log directory for Project Daystrom.
///
/// - macOS: `~/Library/Logs/{TAURI_IDENTIFIER}/`
/// - Windows: `{LOCALAPPDATA}/{TAURI_IDENTIFIER}/logs/`
fn log_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()?;
        Some(home.join("Library/Logs").join(TAURI_IDENTIFIER))
    }

    #[cfg(target_os = "windows")]
    {
        let local = dirs::data_local_dir()?;
        Some(local.join(TAURI_IDENTIFIER).join("logs"))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Full path to the active log file.
fn log_file_path(dir: &Path, base_name: &str) -> PathBuf {
    dir.join(format!("{base_name}.log"))
}

/// Open the log file for appending, wrapped in a `BufWriter`.
fn open_log_writer(dir: &Path, base_name: &str) -> Option<BufWriter<File>> {
    let path = log_file_path(dir, base_name);
    let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
    Some(BufWriter::new(file))
}

/// Determine the initial log file base name from the `DAYSTROM_PROFILE` env variable.
///
/// - Not set or empty or `new_account`: `"mod"` (default)
/// - `106_Nabor`: `"mod_106_Nabor"`
fn initial_log_base_name() -> String {
    match std::env::var(PROFILE_ENV_VAR) {
        Ok(val) if !val.is_empty() && val != NEW_ACCOUNT_PROFILE_STEM && val != INITIAL_PROFILE_STEM => {
            format!("mod_{val}")
        }
        _ => "mod".to_string(),
    }
}

// ---- Log rotation ---------------------------------------------------------

/// Extract the date from the last timestamped line in a log file.
///
/// Reads only the last [`TAIL_READ_SIZE`] bytes to avoid loading large files into memory.
/// Scans backwards through those lines looking for one starting with an ISO 8601 date (`YYYY-MM-DD`).
fn last_log_date(path: &Path) -> Option<NaiveDate> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }

    let read_from = len.saturating_sub(TAIL_READ_SIZE);
    file.seek(SeekFrom::Start(read_from)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    // If we seeked into the middle of a line, the first "line" is a fragment; skip it.
    let lines = if read_from > 0 {
        buf.split_once('\n').map_or("", |(_fragment, rest)| rest)
    } else {
        &buf
    };

    lines
        .lines()
        .rev()
        .find_map(|line| NaiveDate::parse_from_str(line.get(..10)?, "%Y-%m-%d").ok())
}

/// Rotate the current log file if its last entry is from before today.
///
/// Renames `{base}.log` to `{base}_YYYY-MM-DD.log` (using the date of the last entry).
/// If the file contains no valid timestamps, it gets truncated.
/// If an archive with that name already exists, the rotation is skipped.
fn rotate_log_file(dir: &Path, base_name: &str, today: NaiveDate) {
    let log_file = log_file_path(dir, base_name);
    if !log_file.exists() {
        return;
    }

    match last_log_date(&log_file) {
        Some(last_date) if last_date < today => {
            let archive = dir.join(format!("{base_name}_{}.log", last_date.format("%Y-%m-%d")));
            if archive.exists() {
                // Archive for that date already exists, truncate instead
                let _ = fs::write(&log_file, "");
            } else if fs::rename(&log_file, &archive).is_err() {
                let _ = fs::write(&log_file, "");
            }
        }
        Some(_) => {} // the last entry is from today, nothing to do
        None => {
            // No valid timestamps, truncate
            let _ = fs::write(&log_file, "");
        }
    }
}

/// Delete archived log files older than [`MAX_LOG_AGE_DAYS`].
///
/// Recognizes archives named `{prefix}_YYYY-MM-DD.log` for any prefix starting with `mod`.
fn cleanup_old_archives(dir: &Path, today: NaiveDate) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Only process files that start with "mod" and end with ".log"
        if !name.starts_with("mod") || !name.ends_with(".log") {
            continue;
        }

        // Try to extract a date from the last 14 characters: _YYYY-MM-DD.log
        if name.len() < 15 {
            continue;
        }
        let date_part = &name[name.len() - 15..name.len() - 4]; // "_YYYY-MM-DD"
        let Some(date_str) = date_part.strip_prefix('_') else {
            continue;
        };
        let Ok(file_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
            continue;
        };

        if (today - file_date).num_days() > MAX_LOG_AGE_DAYS {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// ---- Background writer thread ---------------------------------------------

/// Background thread that receives log lines and control messages.
///
/// Drains all queued messages before flushing, which naturally batches writes during bursts.
/// Handles date-based rotation, archive clean-up, and live log file renaming.
fn writer_thread(rx: mpsc::Receiver<LogMessage>, dir: PathBuf, mut base_name: String) {
    let mut current_date = Local::now().date_naive();
    let mut writer = open_log_writer(&dir, &base_name);

    while let Ok(msg) = rx.recv() {
        match msg {
            LogMessage::Rename(new_name) => {
                // Flush and close current file
                if let Some(ref mut w) = writer {
                    let _ = w.flush();
                }
                drop(writer.take());

                // Rename the file on disk
                let old_path = log_file_path(&dir, &base_name);
                let new_path = log_file_path(&dir, &new_name);
                if old_path.exists() && !new_path.exists() {
                    let _ = fs::rename(&old_path, &new_path);
                }

                base_name = new_name;
                writer = open_log_writer(&dir, &base_name);
                continue;
            }
            LogMessage::Line(first) => {
                // Check rotation before writing
                let today = Local::now().date_naive();
                if today != current_date {
                    drop(writer.take());
                    rotate_log_file(&dir, &base_name, today);
                    cleanup_old_archives(&dir, today);
                    current_date = today;
                    writer = open_log_writer(&dir, &base_name);
                }

                // Write the first message + drain any additional queued messages
                if let Some(ref mut w) = writer {
                    let _ = writeln!(w, "{first}");
                    loop {
                        match rx.try_recv() {
                            Ok(LogMessage::Line(line)) => {
                                let _ = writeln!(w, "{line}");
                            }
                            Ok(LogMessage::Rename(new_name)) => {
                                // Flush, rename, reopen mid-batch
                                let _ = w.flush();
                                drop(writer.take());
                                let old_path = log_file_path(&dir, &base_name);
                                let new_path = log_file_path(&dir, &new_name);
                                if old_path.exists() && !new_path.exists() {
                                    let _ = fs::rename(&old_path, &new_path);
                                }
                                base_name = new_name;
                                writer = open_log_writer(&dir, &base_name);
                                break;
                            }
                            Err(_) => break, // queue empty
                        }
                    }
                    if let Some(ref mut w) = writer {
                        let _ = w.flush();
                    }
                }
            }
        }
    }
}

// ---- Logger implementation ------------------------------------------------

// ---- Per-target log levels from settings.toml ---------------------------------

/// Per-target log level overrides, loaded once at init from `[log_levels]` in settings.toml.
static TARGET_LEVELS: OnceLock<HashMap<String, LevelFilter>> = OnceLock::new();

/// Minimal struct to extract only the `[log_levels.game]` section from settings.toml.
#[derive(Default, Deserialize)]
struct LogLevelSettings {
    #[serde(default)]
    log_levels: LogLevelScopes,
}

/// Scoped log level overrides.
/// The mod only reads the `game` scope, `[log_levels.app]` is consumed by the backend and silently ignored here.
#[derive(Default, Deserialize)]
struct LogLevelScopes {
    #[serde(default)]
    game: HashMap<String, String>,
}

/// Parse a level string (case-insensitive) into a [`LevelFilter`].
fn parse_level_filter(s: &str) -> Option<LevelFilter> {
    s.parse().ok()
}

/// Load per-target log level overrides from `[log_levels.game]` in settings.toml.
///
/// Returns an empty map when the file is missing, unreadable, or contains no overrides.
/// Invalid level strings are silently skipped.
fn load_log_levels() -> HashMap<String, LevelFilter> {
    let Some(path) = dirs::data_dir().map(|d| d.join(TAURI_IDENTIFIER).join("settings.toml")) else {
        return HashMap::new();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let settings: LogLevelSettings = toml::from_str(&content).unwrap_or_default();

    settings
        .log_levels
        .game
        .into_iter()
        .filter_map(|(target, level_str)| Some((target, parse_level_filter(&level_str)?)))
        .collect()
}

// ---- Logger -------------------------------------------------------------------

/// Custom `log::Log` implementation that writes to the mod log file in Daystrom's bit-log format
/// via a background writer thread.
///
/// Format: `{ISO8601} {LEVEL:5} [{component:20}] ({file:30}: {line:4}): {message}`
///
/// The component name is derived from the log record's `target` field.
/// Use `log::info!(target: "NavigationZoom", "...")` to set it or rely on the module path by default.
///
/// `log()` formats the line and sends it through an `mpsc` channel.
/// The game thread never performs file I/O.
struct ModLogger;

impl Log for ModLogger {
    /// Check whether a log record should be emitted.
    ///
    /// Checks per-target level overrides from `[log_levels.game]` in settings.toml.
    /// Targets without an explicit override use the default level (Info).
    fn enabled(&self, metadata: &Metadata) -> bool {
        let max_level = TARGET_LEVELS
            .get()
            .and_then(|levels| levels.get(metadata.target()).copied())
            .unwrap_or(LevelFilter::Info);
        metadata.level() <= max_level
    }

    /// Format a log line and send it to the background writer thread.
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Some(sender) = LOG_SENDER.get() else {
            return;
        };

        let now = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z");
        let level = match record.level() {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };
        let component = record.target();
        let file = record.file().unwrap_or("unknown");
        let line = record.line().unwrap_or(0);

        let formatted = format!("{now} {level:<5} [{component:<20}] ({file:<30}: {line:>4}): {}", record.args());

        let _ = sender.send(LogMessage::Line(formatted));
    }

    /// Flush is a no-op; the background thread flushes after each batch.
    fn flush(&self) {}
}

// ---- Public API -----------------------------------------------------------

/// Initialize the mod logger as the global `log` logger.
///
/// Reads `DAYSTROM_PROFILE` to determine the initial log file name. Runs startup log rotation,
/// spawns the background writer thread, then registers the logger.
/// Must be called exactly once, typically from the `#[ctor]` entrypoint.
/// Panics if a logger has already been set.
pub fn init() {
    let Some(dir) = log_dir() else { return };
    fs::create_dir_all(&dir).ok();

    let base_name = initial_log_base_name();

    // Rotate before spawning the writer so the file is clean
    let today = Local::now().date_naive();
    rotate_log_file(&dir, &base_name, today);
    cleanup_old_archives(&dir, today);

    // Spawn background writer
    let (tx, rx) = mpsc::channel();
    LOG_SENDER.get_or_init(|| tx);

    let writer_dir = dir.clone();
    let writer_name = base_name.clone();
    thread::Builder::new()
        .name("mod-log-writer".to_string())
        .spawn(move || writer_thread(rx, writer_dir, writer_name))
        .expect("failed to spawn log writer thread");

    let levels = TARGET_LEVELS.get_or_init(load_log_levels);

    // The global max level must be the most permissive of the default (Info) and any per-target override.
    // Otherwise, the `log` crate's compile-time/static filter would discard records before they even reach `enabled()`.
    let max_override = levels.values().copied().max().unwrap_or(LevelFilter::Off);
    let effective_max = std::cmp::max(LevelFilter::Info, max_override);

    log::set_logger(&LOGGER).expect("logger already initialized");
    log::set_max_level(effective_max);
}

/// Rename the active log file to a new profile-specific name.
///
/// Called by the profile store when server + player name become known for the first time.
/// The writer thread handles the actual rename asynchronously.
pub fn rename_log(profile_stem: &str) {
    let new_name = format!("mod_{profile_stem}");
    if let Some(sender) = LOG_SENDER.get() {
        let _ = sender.send(LogMessage::Rename(new_name));
    }
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a fresh temporary directory for a single test.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("stfc-mod-tests").join(name);
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Shorthand for constructing a `NaiveDate`.
    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Standard log line template for test data.
    fn log_line(date: &str, msg: &str) -> String {
        format!(
            "{date}T22:00:00.000+01:00 INFO  [Mod                 ] (lib.rs                        :   18): {msg}\n"
        )
    }

    // -- log_file_path --

    #[test]
    fn log_file_path_default() {
        let path = log_file_path(Path::new("/some/dir"), "mod");
        assert_eq!(path, PathBuf::from("/some/dir/mod.log"));
    }

    #[test]
    fn log_file_path_profile() {
        let path = log_file_path(Path::new("/some/dir"), "mod_106_Nabor");
        assert_eq!(path, PathBuf::from("/some/dir/mod_106_Nabor.log"));
    }

    // -- last_log_date --

    #[test]
    fn last_log_date_missing_file() {
        let dir = test_dir("last_log_date_missing");
        assert_eq!(last_log_date(&dir.join("nonexistent.log")), None);
    }

    #[test]
    fn last_log_date_empty_file() {
        let dir = test_dir("last_log_date_empty");
        let path = dir.join("mod.log");
        fs::write(&path, "").unwrap();
        assert_eq!(last_log_date(&path), None);
    }

    #[test]
    fn last_log_date_single_entry() {
        let dir = test_dir("last_log_date_single");
        let path = dir.join("mod.log");
        fs::write(&path, log_line("2026-03-16", "Hallo")).unwrap();
        assert_eq!(last_log_date(&path), Some(date(2026, 3, 16)));
    }

    #[test]
    fn last_log_date_returns_last() {
        let dir = test_dir("last_log_date_last");
        let path = dir.join("mod.log");
        let mut content = log_line("2026-03-15", "First");
        content.push_str(&log_line("2026-03-16", "Second"));
        fs::write(&path, content).unwrap();
        assert_eq!(last_log_date(&path), Some(date(2026, 3, 16)));
    }

    #[test]
    fn last_log_date_garbage_content() {
        let dir = test_dir("last_log_date_garbage");
        let path = dir.join("mod.log");
        fs::write(&path, "this is not a log file\nrandom garbage\n").unwrap();
        assert_eq!(last_log_date(&path), None);
    }

    #[test]
    fn last_log_date_skips_trailing_non_timestamp() {
        let dir = test_dir("last_log_date_trailing");
        let path = dir.join("mod.log");
        let mut content = log_line("2026-03-16", "Crash");
        content.push_str("  at SomeFunction+0x42\n");
        content.push_str("  at AnotherFunction+0x100\n");
        fs::write(&path, content).unwrap();
        assert_eq!(last_log_date(&path), Some(date(2026, 3, 16)));
    }

    #[test]
    fn last_log_date_handles_large_file() {
        let dir = test_dir("last_log_date_large");
        let path = dir.join("mod.log");
        let line = log_line("2026-03-15", "padding");
        let count = (TAIL_READ_SIZE as usize / line.len()) + 10;
        let mut content = line.repeat(count);
        content.push_str(&log_line("2026-03-16", "last line"));
        fs::write(&path, content).unwrap();
        assert_eq!(last_log_date(&path), Some(date(2026, 3, 16)));
    }

    // -- rotate_log_file --

    #[test]
    fn rotate_noop_when_no_log_file() {
        let dir = test_dir("rotate_noop");
        rotate_log_file(&dir, "mod", date(2026, 3, 17));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn rotate_keeps_todays_file() {
        let dir = test_dir("rotate_today");
        let log = dir.join("mod.log");
        fs::write(&log, log_line("2026-03-16", "Today")).unwrap();
        rotate_log_file(&dir, "mod", date(2026, 3, 16));
        assert!(log.exists());
        assert!(!dir.join("mod_2026-03-16.log").exists());
    }

    #[test]
    fn rotate_archives_old_file() {
        let dir = test_dir("rotate_archive");
        let log = dir.join("mod.log");
        fs::write(&log, log_line("2026-03-15", "Yesterday")).unwrap();
        rotate_log_file(&dir, "mod", date(2026, 3, 16));
        assert!(!log.exists());
        assert!(dir.join("mod_2026-03-15.log").exists());
    }

    #[test]
    fn rotate_truncates_when_archive_exists() {
        let dir = test_dir("rotate_archive_exists");
        let log = dir.join("mod.log");
        let archive = dir.join("mod_2026-03-15.log");
        fs::write(&log, log_line("2026-03-15", "Old")).unwrap();
        fs::write(&archive, "existing archive\n").unwrap();
        rotate_log_file(&dir, "mod", date(2026, 3, 16));
        assert!(log.exists());
        assert_eq!(fs::read_to_string(&log).unwrap(), "");
        // Original archive untouched
        assert_eq!(fs::read_to_string(&archive).unwrap(), "existing archive\n");
    }

    #[test]
    fn rotate_truncates_garbage_file() {
        let dir = test_dir("rotate_garbage");
        let log = dir.join("mod.log");
        fs::write(&log, "not a valid log file\n").unwrap();
        rotate_log_file(&dir, "mod", date(2026, 3, 16));
        assert!(log.exists());
        assert_eq!(fs::read_to_string(&log).unwrap(), "");
    }

    // -- rotate with profile name --

    #[test]
    fn rotate_profile_log() {
        let dir = test_dir("rotate_profile");
        let log = dir.join("mod_106_Nabor.log");
        fs::write(&log, log_line("2026-03-15", "Old")).unwrap();
        rotate_log_file(&dir, "mod_106_Nabor", date(2026, 3, 16));
        assert!(!log.exists());
        assert!(dir.join("mod_106_Nabor_2026-03-15.log").exists());
    }

    // -- writer_thread --

    #[test]
    fn writer_thread_writes_and_flushes() {
        let dir = test_dir("writer_writes");
        let (tx, rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let handle = thread::spawn(move || writer_thread(rx, writer_dir, "mod".to_string()));

        tx.send(LogMessage::Line(
            "2026-03-16T22:00:00.000+01:00 INFO  first message".to_string(),
        ))
        .unwrap();
        tx.send(LogMessage::Line(
            "2026-03-16T22:00:01.000+01:00 INFO  second message".to_string(),
        ))
        .unwrap();
        drop(tx);
        handle.join().unwrap();

        let content = fs::read_to_string(dir.join("mod.log")).unwrap();
        assert!(content.contains("first message"));
        assert!(content.contains("second message"));
    }

    #[test]
    fn writer_thread_exits_on_sender_drop() {
        let dir = test_dir("writer_exits");
        let (tx, rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let handle = thread::spawn(move || writer_thread(rx, writer_dir, "mod".to_string()));

        drop(tx);
        // Thread should exit cleanly without hanging
        handle.join().unwrap();
    }

    #[test]
    fn writer_thread_rename() {
        let dir = test_dir("writer_rename");
        let (tx, rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let handle = thread::spawn(move || writer_thread(rx, writer_dir, "mod".to_string()));

        tx.send(LogMessage::Line("before rename".to_string())).unwrap();
        // Small delay to ensure the line is written before rename
        thread::sleep(std::time::Duration::from_millis(50));
        tx.send(LogMessage::Rename("mod_106_Nabor".to_string())).unwrap();
        // Small delay to ensure rename completes
        thread::sleep(std::time::Duration::from_millis(50));
        tx.send(LogMessage::Line("after rename".to_string())).unwrap();
        drop(tx);
        handle.join().unwrap();

        // Old file should not exist (renamed)
        assert!(!dir.join("mod.log").exists());
        // New file should contain both lines
        let content = fs::read_to_string(dir.join("mod_106_Nabor.log")).unwrap();
        assert!(content.contains("before rename"));
        assert!(content.contains("after rename"));
    }

    // ---- parse_level_filter tests -----------------------------------------

    #[test]
    fn parse_level_filter_valid_levels() {
        assert!(matches!(parse_level_filter("off"), Some(LevelFilter::Off)));
        assert!(matches!(parse_level_filter("error"), Some(LevelFilter::Error)));
        assert!(matches!(parse_level_filter("warn"), Some(LevelFilter::Warn)));
        assert!(matches!(parse_level_filter("info"), Some(LevelFilter::Info)));
        assert!(matches!(parse_level_filter("debug"), Some(LevelFilter::Debug)));
        assert!(matches!(parse_level_filter("trace"), Some(LevelFilter::Trace)));
    }

    #[test]
    fn parse_level_filter_case_insensitive() {
        let cases = [("DEBUG", LevelFilter::Debug), ("Info", LevelFilter::Info), ("TRACE", LevelFilter::Trace)];

        for (input, expected) in cases {
            assert_eq!(parse_level_filter(input), Some(expected));
        }
    }

    #[test]
    fn parse_level_filter_invalid() {
        for input in ["", "invalid", "Debu"] {
            assert_eq!(parse_level_filter(input), None);
        }
    }

    // ---- LogLevelSettings deserialization tests ----------------------------

    #[test]
    fn deserialize_log_levels_game_section() {
        let toml_str = r#"
[log_levels.game]
PlayerPrefs = "Info"
HookEngine = "Debug"
"#;
        let settings: LogLevelSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(settings.log_levels.game.len(), 2);
        assert_eq!(settings.log_levels.game["PlayerPrefs"], "Info");
        assert_eq!(settings.log_levels.game["HookEngine"], "Debug");
    }

    #[test]
    fn deserialize_log_levels_ignores_app_section() {
        let toml_str = r#"
[log_levels.app]
Settings = "Debug"

[log_levels.game]
PlayerPrefs = "Info"
"#;
        let settings: LogLevelSettings = toml::from_str(toml_str).unwrap();
        assert_eq!(settings.log_levels.game.len(), 1);
    }

    #[test]
    fn deserialize_missing_log_levels_uses_defaults() {
        let toml_str = r#"
[ui]
scale = 100
"#;
        let settings: LogLevelSettings = toml::from_str(toml_str).unwrap();
        assert!(settings.log_levels.game.is_empty());
    }
}
