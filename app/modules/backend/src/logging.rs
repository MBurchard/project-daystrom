use std::{fs, path::{Path, PathBuf}, sync::Mutex};

use colored::Colorize;
use log::{Level, LevelFilter};
use tauri::plugin::TauriPlugin;
use tauri_plugin_log::{Builder, Target, TargetKind, TimezoneStrategy, fern};

// ---- Macros (public API) --------------------------------------------------------

/// Create a named logger for the current scope, mirroring the frontend's `createLogger('Name')`.
///
/// Generates `log_trace!`, `log_debug!`, `log_info!`, `log_warn!` and `log_error!` macros
/// that automatically set the log target to the given name. The name appears in the
/// `[loggerName]` column of the log output.
///
/// # Example
///
/// ```ignore
/// use_log!("Startup");
/// log_info!("Project Daystrom {version} initialised");
/// // → ... INFO [Startup] (Backend: lib.rs: 14): Project Daystrom 0.1.0 initialised
/// ```
#[macro_export]
macro_rules! use_log {
    ($target:expr) => {
        $crate::__define_log_macros!($target, $);
    };
}

/// Implementation detail of [`use_log!`]. The extra `$d` parameter receives a literal `$` token
/// so the inner macro definitions can use `$d(...)` to produce `$(...)` in their output.
#[doc(hidden)]
#[macro_export]
macro_rules! __define_log_macros {
    ($target:expr, $d:tt) => {
        #[allow(unused_macros)]
        macro_rules! log_trace { ($d ( $d arg:tt )*) => { ::log::trace!(target: $target, $d ( $d arg )*) }; }
        #[allow(unused_macros)]
        macro_rules! log_debug { ($d ( $d arg:tt )*) => { ::log::debug!(target: $target, $d ( $d arg )*) }; }
        #[allow(unused_macros)]
        macro_rules! log_info { ($d ( $d arg:tt )*) => { ::log::info!(target: $target, $d ( $d arg )*) }; }
        #[allow(unused_macros)]
        macro_rules! log_warn { ($d ( $d arg:tt )*) => { ::log::warn!(target: $target, $d ( $d arg )*) }; }
        #[allow(unused_macros)]
        macro_rules! log_error { ($d ( $d arg:tt )*) => { ::log::error!(target: $target, $d ( $d arg )*) }; }
    };
}

// ---- Plugin builder -------------------------------------------------------------

/// Base name for log files (without extension).
const LOG_FILE_NAME: &str = "project-daystrom";

/// Build the tauri-plugin-log plugin with our custom format and targets.
///
/// Performs log rotation before initialising the plugin, because the plugin opens its
/// file handle in append mode — renaming afterwards would not take effect.
pub fn build_plugin() -> TauriPlugin<tauri::Wry> {
    rotate_logs();
    init_runtime_rotation();

    Builder::new()
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .max_file_size(1_000_000) // 1 MB, plugin-internal size rotation
        .level(LevelFilter::Debug)
        .level_for("tao", LevelFilter::Warn)
        .level_for("wry", LevelFilter::Warn)
        .format(format_log)
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir {
                file_name: Some(LOG_FILE_NAME.into()),
            }),
        ])
        .build()
}

// ---- Log rotation ---------------------------------------------------------------

/// Rotate log files before the logging plugin opens its file handle.
///
/// Parses the last timestamp from the current log file to decide whether rotation
/// is needed. If the last entry is from before today, the file gets archived as
/// `project-daystrom_YYYY-MM-DD.log` (using the parsed date, not filesystem metadata).
/// Empty or missing log files are left alone.
/// Archived logs older than [`MAX_LOG_AGE_DAYS`] are deleted.
///
/// Errors go to stderr because the logger is not yet initialised.
fn rotate_logs() {
    let Some(dir) = log_dir() else { return };
    if !dir.is_dir() {
        return;
    }
    rotate_logs_in(&dir);
}

/// Return the platform-specific log directory, if applicable.
///
/// On macOS this is `~/Library/Logs/{identifier}/` where the identifier
/// is read from `tauri.conf.json` at compile time.
/// Returns `None` on other platforms (no game client, no rotation needed).
fn log_dir() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    Some(dirs::home_dir()?.join(format!("Library/Logs/{}", env!("TAURI_IDENTIFIER"))))
}

// ---- Runtime rotation state -----------------------------------------------------

/// Tracks the current date so [`check_runtime_rotation`] can detect midnight crossings.
struct RotationState {
    current_date: time::Date,
    log_dir: PathBuf,
}

/// Global state for runtime log rotation, initialised by [`init_runtime_rotation`].
static ROTATION_STATE: Mutex<Option<RotationState>> = Mutex::new(None);

/// Initialise the runtime rotation state with today's date and the log directory.
///
/// Called once from [`build_plugin`] after the startup rotation has completed.
/// On platforms without a log directory (non-macOS), this is a no-op.
fn init_runtime_rotation() {
    let Some(dir) = log_dir() else { return };
    let today = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date();
    *ROTATION_STATE.lock().unwrap() = Some(RotationState {
        current_date: today,
        log_dir: dir,
    });
}

// ---- Log cleanup & rotation -----------------------------------------------------

/// Core rotation logic, separated from [`rotate_logs`] for testability.
fn rotate_logs_in(dir: &Path) {
    let today = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date();
    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");

    // Rotate current log file if its last entry is from before today
    let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
    if log_file.exists() {
        match last_log_date(&log_file) {
            Some(last_date) if last_date < today => {
                if let Ok(date_str) = last_date.format(&date_fmt) {
                    let time_suffix = normalize_plugin_archives(dir, &date_str);
                    let archive_name = match &time_suffix {
                        Some(ts) => format!("{LOG_FILE_NAME}_{date_str}_{ts}.log"),
                        None => format!("{LOG_FILE_NAME}_{date_str}.log"),
                    };
                    let archive_path = dir.join(&archive_name);

                    if archive_path.exists() {
                        eprintln!(
                            "Log rotation: {archive_name} already exists, skipping {}",
                            log_file.display()
                        );
                    } else if let Err(e) = fs::rename(&log_file, &archive_path) {
                        eprintln!(
                            "Log rotation: failed to archive {} as {archive_name}: {e}",
                            log_file.display()
                        );
                    }
                }
            }
            Some(_) => {} // last entry is from today, nothing to do
            None => {
                // File exists but contains no valid timestamps — truncate it
                if let Err(e) = fs::write(&log_file, "") {
                    eprintln!(
                        "Log rotation: failed to truncate {}: {e}",
                        log_file.display()
                    );
                }
            }
        }
    }

    cleanup_old_archives(dir, today);
}

/// Number of days to keep archived log files.
const MAX_LOG_AGE_DAYS: i64 = 30;

/// Delete archived log files older than [`MAX_LOG_AGE_DAYS`].
///
/// Recognises both our date-only archives (`project-daystrom_YYYY-MM-DD.log`) and the
/// plugin's size-rotation archives (`project-daystrom_YYYY-MM-DD_HH-MM-SS.log`) by
/// parsing only the first 10 characters after the prefix as a date.
fn cleanup_old_archives(dir: &Path, today: time::Date) {
    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");
    let prefix = format!("{LOG_FILE_NAME}_");
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Log rotation: cannot read {}: {e}", dir.display());
            return;
        }
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        let Some(rest) = name.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if !rest.ends_with(".log") || rest.len() < 10 {
            continue;
        }
        let Ok(file_date) = time::Date::parse(&rest[..10], &date_fmt) else {
            continue;
        };

        if (today - file_date).whole_days() > MAX_LOG_AGE_DAYS {
            if let Err(e) = fs::remove_file(entry.path()) {
                eprintln!("Log rotation: failed to delete old log {name}: {e}");
            }
        }
    }
}

/// Rename plugin-rotated archives so timestamps reflect content start instead of rotation time.
///
/// The plugin's size-based rotation creates files like `{LOG_FILE_NAME}_{date}_HH-MM-SS.log`
/// where HH-MM-SS is the rotation (end) time. This function renames them so each file's
/// timestamp indicates when its content begins:
/// - The earliest file gets `_00-00-00` (midnight / start of day)
/// - Each subsequent file gets the original timestamp of its predecessor
///
/// Returns the last original timestamp (used as time suffix for the copy-truncate archive),
/// or `None` if no plugin-rotated files were found (or already normalized).
fn normalize_plugin_archives(dir: &Path, date_str: &str) -> Option<String> {
    let prefix = format!("{LOG_FILE_NAME}_{date_str}_");
    let entries = fs::read_dir(dir).ok()?;

    let mut times: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let rest = name.strip_prefix(prefix.as_str())?;
            let time_part = rest.strip_suffix(".log")?;
            // HH-MM-SS is exactly 8 characters
            (time_part.len() == 8).then(|| time_part.to_string())
        })
        .collect();

    if times.is_empty() {
        return None;
    }

    times.sort();

    // First file already starts at midnight — archives were already normalized
    if times[0] == "00-00-00" {
        return None;
    }

    // Rename from early to late: each slot is freed before it gets reused
    let mut prev_time = "00-00-00".to_string();
    for time in &times {
        let old_name = format!("{LOG_FILE_NAME}_{date_str}_{time}.log");
        let new_name = format!("{LOG_FILE_NAME}_{date_str}_{prev_time}.log");

        if let Err(e) = fs::rename(dir.join(&old_name), dir.join(&new_name)) {
            eprintln!("Log rotation: failed to rename {old_name} to {new_name}: {e}");
        }
        prev_time = time.clone();
    }

    Some(prev_time)
}

/// Copy-truncate the current log file into a dated archive.
///
/// Uses `fs::copy` + `set_len(0)` instead of rename because the logging plugin holds
/// the file handle open. When `time_suffix` is provided, the archive includes a time
/// component (`_YYYY-MM-DD_HH-MM-SS.log`); otherwise it uses date-only naming.
/// Skips silently if the log file is missing, has no valid timestamps, or the target
/// archive already exists.
fn copy_truncate_rotation(dir: &Path, time_suffix: Option<&str>) {
    let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
    if !log_file.exists() {
        return;
    }

    let Some(last_date) = last_log_date(&log_file) else { return };
    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");
    let Ok(date_str) = last_date.format(&date_fmt) else { return };
    let archive_name = match time_suffix {
        Some(ts) => format!("{LOG_FILE_NAME}_{date_str}_{ts}.log"),
        None => format!("{LOG_FILE_NAME}_{date_str}.log"),
    };
    let archive_path = dir.join(&archive_name);

    if archive_path.exists() {
        return;
    }

    if let Err(e) = fs::copy(&log_file, &archive_path) {
        eprintln!("Runtime rotation: failed to copy log to {archive_name}: {e}");
        return;
    }

    if let Err(e) = fs::File::options()
        .write(true)
        .open(&log_file)
        .and_then(|f| f.set_len(0))
    {
        eprintln!("Runtime rotation: failed to truncate {}: {e}", log_file.display());
    }
}

/// Check whether the date has changed since the last log event and rotate if needed.
///
/// Called at the start of every [`format_log`] invocation. The fast path (same date)
/// is a single mutex lock + date comparison. On date change, performs a copy-truncate
/// rotation followed by archive cleanup.
fn check_runtime_rotation() {
    let mut guard = match ROTATION_STATE.lock() {
        Ok(g) => g,
        Err(_) => return, // poisoned mutex, skip rotation
    };
    let Some(state) = guard.as_mut() else { return };

    let today = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .date();

    // Fast path: same date, nothing to do
    if today == state.current_date {
        return;
    }

    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");
    let last_time = state.current_date.format(&date_fmt).ok().and_then(|date_str| {
        normalize_plugin_archives(&state.log_dir, &date_str)
    });
    copy_truncate_rotation(&state.log_dir, last_time.as_deref());
    cleanup_old_archives(&state.log_dir, today);
    state.current_date = today;
}

/// Maximum number of bytes to read from the end of a log file when looking for the last timestamp.
const TAIL_READ_SIZE: u64 = 4096;

/// Extract the date from the last timestamped line in a log file.
///
/// Reads only the last [`TAIL_READ_SIZE`] bytes to avoid loading large files into memory.
/// Scans backwards through those lines looking for one starting with an ISO 8601
/// date (`YYYY-MM-DD`). Returns the parsed date, or `None` if the file is empty,
/// missing, or contains no valid timestamp.
fn last_log_date(path: &Path) -> Option<time::Date> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }

    let read_from = len.saturating_sub(TAIL_READ_SIZE);
    file.seek(SeekFrom::Start(read_from)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    let date_fmt = time::macros::format_description!("[year]-[month]-[day]");

    // If we seeked into the middle of a line, the first "line" is a fragment — skip it
    let lines = if read_from > 0 {
        buf.split_once('\n').map_or("", |(_fragment, rest)| rest)
    } else {
        &buf
    };

    lines.lines().rev().find_map(|line| {
        let date_str = line.get(..10)?;
        time::Date::parse(date_str, &date_fmt).ok()
    })
}

// ---- Log formatting -------------------------------------------------------------

/// Unit Separator — delimiter between logger name and message from JS frontend.
const SEP: char = '\x1F';

/// Display width for the logger name in log output.
/// Matches bit-log's default. Names are right-padded or left-truncated to this width.
const LOGGER_NAME_WIDTH: usize = 20;

/// Display width for the file path in log output.
/// Paths longer than this are middle-truncated with "...".
const FILE_PATH_WIDTH: usize = 30;

/// Build the log line matching bit-log's format:
/// `{timestamp} {LEVEL} [{loggerName}] ({file}:{line}): {message}`
///
/// For JS-originated logs, the logger name is embedded in the message as `name\x1Fmessage`.
/// For Rust-originated logs, `record.target()` is used as the logger name.
fn format_log(
    callback: fern::FormatCallback,
    message: &std::fmt::Arguments,
    record: &log::Record,
) {
    check_runtime_rotation();

    let timestamp = format_timestamp();
    let level = coloured_level(record.level());
    let file = record.file().unwrap_or("unknown");
    let file = file.strip_prefix("src/").unwrap_or(file);
    let file_display = fit_path(file, FILE_PATH_WIDTH);
    let line = record.line().unwrap_or(0);

    let raw = message.to_string();
    let (origin, logger_name, msg) = match raw.split_once(SEP) {
        Some((name, rest)) => ("Frontend", name, rest),
        None => ("Backend ", record.target(), raw.as_str()),
    };
    let target = fit(logger_name, LOGGER_NAME_WIDTH);

    callback.finish(format_args!(
        "{timestamp} {level} [{target}] ({origin}: {file_display}: {line:>4}): {msg}"
    ));
}

/// Format the current local time as ISO 8601 with milliseconds and timezone offset.
/// Example: `2026-02-20T14:30:45.123+01:00`
fn format_timestamp() -> String {
    let now = TimezoneStrategy::UseLocal.get_now();
    let format = time::format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]\
         [offset_hour sign:mandatory]:[offset_minute]",
    )
    .expect("invalid time format");
    now.format(&format).unwrap_or_else(|_| "????-??-??T??:??:??.???+??:??".to_string())
}

/// Colourise a log level string matching bit-log's colour scheme:
/// TRACE=dark gray, DEBUG=gray, INFO=green, WARN=yellow, ERROR=red
fn coloured_level(level: Level) -> String {
    let tag = fit(&level.to_string(), 5);
    match level {
        Level::Trace => tag.dimmed().to_string(),
        Level::Debug => tag.bright_black().to_string(),
        Level::Info => tag.green().to_string(),
        Level::Warn => tag.yellow().to_string(),
        Level::Error => tag.red().to_string(),
    }
}

/// Pad or left-truncate a string to exactly `width` characters.
/// Truncates from the left (keeps the end), pads on the right.
fn fit(s: &str, width: usize) -> String {
    let char_count = s.chars().count();
    if char_count > width {
        s.chars().skip(char_count - width).collect()
    } else {
        format!("{s:<width$}")
    }
}

/// Pad or middle-truncate a path to exactly `width` characters.
/// Keeps the beginning and end of the path, replaces the middle with "...".
/// Short strings are right-padded with spaces.
fn fit_path(s: &str, width: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= width {
        return format!("{s:<width$}");
    }
    // 3 chars for "...", split remaining space: more at end (filename matters most)
    let available = width - 3;
    let end_len = (available + 1) / 2;
    let start_len = available - end_len;
    let start: String = s.chars().take(start_len).collect();
    let end: String = s.chars().skip(char_count - end_len).collect();
    format!("{start}...{end}")
}

// ---- Tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temporary directory for a test, removing leftovers from previous runs.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("daystrom_test_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Format a fake log line starting with the given date.
    fn log_line(date: &str) -> String {
        format!(
            "{date}T14:30:45.123+01:00 INFO  [Test                ] \
             (Backend : test.rs                       :    1): message\n"
        )
    }

    /// Mutex to serialise tests that read/write the global [`ROTATION_STATE`].
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Return today's date.
    fn today_date() -> time::Date {
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .date()
    }

    /// Format a [`time::Date`] as `YYYY-MM-DD`.
    fn format_date(date: time::Date) -> String {
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        date.format(&fmt).unwrap()
    }

    /// Return today's date formatted as YYYY-MM-DD.
    fn today_str() -> String {
        format_date(today_date())
    }

    /// Return a date N days ago formatted as YYYY-MM-DD.
    fn days_ago_str(n: i64) -> String {
        format_date(today_date() - time::Duration::days(n))
    }

    // -- last_log_date --

    #[test]
    fn last_log_date_missing_file() {
        let path = std::env::temp_dir().join("daystrom_nonexistent.log");
        assert!(last_log_date(&path).is_none());
    }

    #[test]
    fn last_log_date_empty_file() {
        let dir = test_dir("last_log_date_empty");
        let path = dir.join("test.log");
        fs::write(&path, "").unwrap();
        assert!(last_log_date(&path).is_none());
    }

    #[test]
    fn last_log_date_garbage_content() {
        let dir = test_dir("last_log_date_garbage");
        let path = dir.join("test.log");
        fs::write(&path, "just some random text\nno timestamps here\n").unwrap();
        assert!(last_log_date(&path).is_none());
    }

    #[test]
    fn last_log_date_single_entry() {
        let dir = test_dir("last_log_date_single");
        let path = dir.join("test.log");
        fs::write(&path, log_line("2026-02-20")).unwrap();

        let date = last_log_date(&path).unwrap();
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        assert_eq!(date.format(&fmt).unwrap(), "2026-02-20");
    }

    #[test]
    fn last_log_date_returns_last_date() {
        let dir = test_dir("last_log_date_multi");
        let path = dir.join("test.log");
        let content = format!("{}{}", log_line("2026-02-19"), log_line("2026-02-20"));
        fs::write(&path, content).unwrap();

        let date = last_log_date(&path).unwrap();
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        assert_eq!(date.format(&fmt).unwrap(), "2026-02-20");
    }

    #[test]
    fn last_log_date_skips_trailing_stacktrace() {
        let dir = test_dir("last_log_date_stacktrace");
        let path = dir.join("test.log");
        let content = format!(
            "{}  at SomeClass.method(File.java:42)\n  at Another.call(Other.java:99)\n",
            log_line("2026-02-20")
        );
        fs::write(&path, content).unwrap();

        let date = last_log_date(&path).unwrap();
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        assert_eq!(date.format(&fmt).unwrap(), "2026-02-20");
    }

    #[test]
    fn last_log_date_handles_large_file() {
        let dir = test_dir("last_log_date_large");
        let path = dir.join("test.log");

        // Create a file larger than TAIL_READ_SIZE (4096) with the timestamp near the end
        let padding = "x".repeat(200);
        let mut content = String::new();
        for _ in 0..30 {
            content.push_str(&padding);
            content.push('\n');
        }
        content.push_str(&log_line("2026-02-20"));
        assert!(content.len() > TAIL_READ_SIZE as usize);

        fs::write(&path, content).unwrap();

        let date = last_log_date(&path).unwrap();
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        assert_eq!(date.format(&fmt).unwrap(), "2026-02-20");
    }

    // -- rotate_logs_in --

    #[test]
    fn rotate_archives_old_file() {
        let dir = test_dir("rotate_archive");
        let yesterday = days_ago_str(1);
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&yesterday)).unwrap();

        rotate_logs_in(&dir);

        assert!(!log_file.exists(), "original log should be gone");
        let archive = dir.join(format!("{LOG_FILE_NAME}_{yesterday}.log"));
        assert!(archive.exists(), "archive should exist");
    }

    #[test]
    fn rotate_keeps_todays_file() {
        let dir = test_dir("rotate_today");
        let today = today_str();
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&today)).unwrap();

        rotate_logs_in(&dir);

        assert!(log_file.exists(), "today's log should remain");
    }

    #[test]
    fn rotate_truncates_garbage_file() {
        let dir = test_dir("rotate_garbage");
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, "no valid timestamps here\n").unwrap();

        rotate_logs_in(&dir);

        assert!(log_file.exists(), "file should still exist");
        assert_eq!(fs::read_to_string(&log_file).unwrap(), "", "file should be empty");
    }

    #[test]
    fn rotate_noop_when_no_log_file() {
        let dir = test_dir("rotate_noop");
        // Empty dir, no log file — should not panic
        rotate_logs_in(&dir);
    }

    #[test]
    fn rotate_deletes_old_archives() {
        let dir = test_dir("rotate_delete_old");
        let old_date = days_ago_str(31);
        let old_archive = dir.join(format!("{LOG_FILE_NAME}_{old_date}.log"));
        fs::write(&old_archive, "old logs").unwrap();

        rotate_logs_in(&dir);

        assert!(!old_archive.exists(), "archive older than 30 days should be deleted");
    }

    #[test]
    fn rotate_keeps_recent_archives() {
        let dir = test_dir("rotate_keep_recent");
        let recent_date = days_ago_str(15);
        let recent_archive = dir.join(format!("{LOG_FILE_NAME}_{recent_date}.log"));
        fs::write(&recent_archive, "recent logs").unwrap();

        rotate_logs_in(&dir);

        assert!(recent_archive.exists(), "archive within 30 days should be kept");
    }

    // -- fit --

    #[test]
    fn fit_exact_width() {
        assert_eq!(fit("hello", 5), "hello");
    }

    #[test]
    fn fit_shorter_pads_right() {
        assert_eq!(fit("hi", 5), "hi   ");
    }

    #[test]
    fn fit_longer_truncates_left() {
        assert_eq!(fit("abcdef", 4), "cdef");
    }

    #[test]
    fn fit_empty_string() {
        assert_eq!(fit("", 5), "     ");
    }

    #[test]
    fn fit_multibyte_truncates_left() {
        // "über" = 4 chars but 5 bytes — must not panic
        assert_eq!(fit("über", 3), "ber");
    }

    #[test]
    fn fit_multibyte_pads_right() {
        assert_eq!(fit("ü", 3), "ü  ");
    }

    // -- fit_path --

    #[test]
    fn fit_path_exact_width() {
        assert_eq!(fit_path("src/main.rs", 11), "src/main.rs");
    }

    #[test]
    fn fit_path_shorter_pads_right() {
        assert_eq!(fit_path("lib.rs", 10), "lib.rs    ");
    }

    #[test]
    fn fit_path_longer_middle_truncates() {
        // width=15: available=12, end_len=6, start_len=6
        let result = fit_path("src/game/entitlements.rs", 15);
        assert_eq!(result.len(), 15);
        assert!(result.contains("..."), "expected '...' in '{result}'");
    }

    #[test]
    fn fit_path_preserves_extension() {
        // The end (filename) should survive truncation
        let result = fit_path("some/very/deep/nested/path/file.rs", 20);
        assert!(result.ends_with(".rs"), "expected '.rs' suffix in '{result}'");
    }

    #[test]
    fn fit_path_empty_string() {
        assert_eq!(fit_path("", 10), "          ");
    }

    #[test]
    fn fit_path_multibyte_middle_truncates() {
        // "src/müll/datei.rs" = 17 chars, width=15 — must not panic
        let result = fit_path("src/müll/datei.rs", 15);
        assert_eq!(result.chars().count(), 15);
        assert!(result.contains("..."), "expected '...' in '{result}'");
    }

    // -- copy_truncate_rotation --

    #[test]
    fn runtime_rotation_copies_and_truncates() {
        let dir = test_dir("runtime_copy_truncate");
        let yesterday = days_ago_str(1);
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&yesterday)).unwrap();

        copy_truncate_rotation(&dir, None);

        let archive = dir.join(format!("{LOG_FILE_NAME}_{yesterday}.log"));
        assert!(archive.exists(), "archive should exist");
        assert_eq!(fs::read_to_string(&log_file).unwrap(), "", "log file should be truncated");
    }

    #[test]
    fn runtime_rotation_skips_same_day() {
        let _lock = TEST_LOCK.lock().unwrap();
        let dir = test_dir("runtime_same_day");
        let today = today_str();
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        let content = log_line(&today);
        fs::write(&log_file, &content).unwrap();

        *ROTATION_STATE.lock().unwrap() = Some(RotationState {
            current_date: today_date(),
            log_dir: dir.clone(),
        });

        check_runtime_rotation();

        // Clean up global state
        *ROTATION_STATE.lock().unwrap() = None;

        assert_eq!(fs::read_to_string(&log_file).unwrap(), content, "log file should be unchanged");
    }

    #[test]
    fn runtime_rotation_skips_existing_archive() {
        let dir = test_dir("runtime_skip_existing");
        let yesterday = days_ago_str(1);
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&yesterday)).unwrap();

        // Pre-create the archive with different content
        let archive = dir.join(format!("{LOG_FILE_NAME}_{yesterday}.log"));
        fs::write(&archive, "existing archive content").unwrap();

        copy_truncate_rotation(&dir, None);

        assert_eq!(
            fs::read_to_string(&archive).unwrap(),
            "existing archive content",
            "existing archive should not be overwritten"
        );
    }

    // -- cleanup_old_archives --

    #[test]
    fn cleanup_handles_plugin_rotation_format() {
        let dir = test_dir("cleanup_plugin_format");
        let old_date = days_ago_str(31);
        // Plugin format: project-daystrom_YYYY-MM-DD_HH-MM-SS.log
        let plugin_archive = dir.join(format!("{LOG_FILE_NAME}_{old_date}_14-30-45.log"));
        fs::write(&plugin_archive, "old plugin log").unwrap();

        cleanup_old_archives(&dir, today_date());

        assert!(!plugin_archive.exists(), "plugin-format archive older than 30 days should be deleted");
    }

    #[test]
    fn cleanup_deletes_both_formats() {
        let dir = test_dir("cleanup_both_formats");
        let old_date = days_ago_str(31);

        // Our format: project-daystrom_YYYY-MM-DD.log
        let our_archive = dir.join(format!("{LOG_FILE_NAME}_{old_date}.log"));
        fs::write(&our_archive, "old log").unwrap();

        // Plugin format: project-daystrom_YYYY-MM-DD_HH-MM-SS.log
        let plugin_archive = dir.join(format!("{LOG_FILE_NAME}_{old_date}_14-30-45.log"));
        fs::write(&plugin_archive, "old plugin log").unwrap();

        cleanup_old_archives(&dir, today_date());

        assert!(!our_archive.exists(), "our archive older than 30 days should be deleted");
        assert!(!plugin_archive.exists(), "plugin archive older than 30 days should be deleted");
    }

    // -- normalize_plugin_archives --

    #[test]
    fn normalize_no_plugin_files() {
        let dir = test_dir("normalize_empty");
        assert_eq!(normalize_plugin_archives(&dir, "2026-01-15"), None);
    }

    #[test]
    fn normalize_renames_single_file() {
        let dir = test_dir("normalize_single");
        let date = "2026-01-15";
        fs::write(
            dir.join(format!("{LOG_FILE_NAME}_{date}_09-00-00.log")),
            "log content",
        )
        .unwrap();

        let result = normalize_plugin_archives(&dir, date);
        assert_eq!(result.as_deref(), Some("09-00-00"));

        assert!(
            dir.join(format!("{LOG_FILE_NAME}_{date}_00-00-00.log")).exists(),
            "file should be renamed to _00-00-00"
        );
        assert!(
            !dir.join(format!("{LOG_FILE_NAME}_{date}_09-00-00.log")).exists(),
            "original file should no longer exist"
        );
    }

    #[test]
    fn normalize_renames_chain() {
        let dir = test_dir("normalize_chain");
        let date = "2026-01-15";
        for time in ["09-00-00", "13-00-00", "21-00-00"] {
            fs::write(
                dir.join(format!("{LOG_FILE_NAME}_{date}_{time}.log")),
                format!("content from {time}"),
            )
            .unwrap();
        }

        let result = normalize_plugin_archives(&dir, date);
        assert_eq!(result.as_deref(), Some("21-00-00"));

        // Verify renamed files exist with correct content
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LOG_FILE_NAME}_{date}_00-00-00.log"))).unwrap(),
            "content from 09-00-00"
        );
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LOG_FILE_NAME}_{date}_09-00-00.log"))).unwrap(),
            "content from 13-00-00"
        );
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LOG_FILE_NAME}_{date}_13-00-00.log"))).unwrap(),
            "content from 21-00-00"
        );
        assert!(
            !dir.join(format!("{LOG_FILE_NAME}_{date}_21-00-00.log")).exists(),
            "_21-00-00 should no longer exist"
        );
    }

    #[test]
    fn normalize_skips_other_dates() {
        let dir = test_dir("normalize_other_dates");
        let target = "2026-01-15";
        let other = "2026-01-14";
        fs::write(
            dir.join(format!("{LOG_FILE_NAME}_{target}_09-00-00.log")),
            "target",
        )
        .unwrap();
        fs::write(
            dir.join(format!("{LOG_FILE_NAME}_{other}_12-00-00.log")),
            "other",
        )
        .unwrap();

        normalize_plugin_archives(&dir, target);

        assert!(
            dir.join(format!("{LOG_FILE_NAME}_{other}_12-00-00.log")).exists(),
            "other date's file should remain unchanged"
        );
    }

    // -- copy_truncate_rotation with time suffix --

    #[test]
    fn copy_truncate_with_time_suffix() {
        let dir = test_dir("copy_truncate_time_suffix");
        let yesterday = days_ago_str(1);
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&yesterday)).unwrap();

        copy_truncate_rotation(&dir, Some("21-00-00"));

        let archive = dir.join(format!("{LOG_FILE_NAME}_{yesterday}_21-00-00.log"));
        assert!(archive.exists(), "archive should include time suffix");
        assert_eq!(fs::read_to_string(&log_file).unwrap(), "", "log file should be truncated");
    }

    // -- rotate_logs_in with plugin archives --

    #[test]
    fn rotate_startup_normalizes_archives() {
        let dir = test_dir("startup_normalize");
        let yesterday = days_ago_str(1);
        let log_file = dir.join(format!("{LOG_FILE_NAME}.log"));
        fs::write(&log_file, log_line(&yesterday)).unwrap();

        // Create plugin-rotated files for yesterday
        for time in ["09-00-00", "13-00-00"] {
            fs::write(
                dir.join(format!("{LOG_FILE_NAME}_{yesterday}_{time}.log")),
                format!("plugin content {time}"),
            )
            .unwrap();
        }

        rotate_logs_in(&dir);

        // Plugin files should be normalized
        assert!(
            dir.join(format!("{LOG_FILE_NAME}_{yesterday}_00-00-00.log")).exists(),
            "first plugin file should be renamed to _00-00-00"
        );
        assert!(
            dir.join(format!("{LOG_FILE_NAME}_{yesterday}_09-00-00.log")).exists(),
            "second plugin file should be renamed to _09-00-00"
        );
        // Main log file should be archived with the last original time
        assert!(
            dir.join(format!("{LOG_FILE_NAME}_{yesterday}_13-00-00.log")).exists(),
            "main log should be archived with last plugin time"
        );
        assert!(!log_file.exists(), "original log file should be gone");
    }
}
