use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::thread;

use chrono::{Local, NaiveDate};
use log::{Level, Log, Metadata, Record};

use crate::TAURI_IDENTIFIER;

/// Base name of the log file (without `.log` extension).
const LOG_FILE_NAME: &str = "mod";

/// Number of days to keep archived log files.
const MAX_LOG_AGE_DAYS: i64 = 30;

/// Maximum number of bytes to read from the end of a log file when looking for the last timestamp.
const TAIL_READ_SIZE: u64 = 4096;

/// Global logger instance, initialised once via `init()`.
static LOGGER: ModLogger = ModLogger;

/// Channel sender for the background writer thread.
static LOG_SENDER: OnceLock<Sender<String>> = OnceLock::new();

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

/// Full path to the active log file (`mod.log`).
fn log_file_path(dir: &Path) -> PathBuf {
    dir.join(format!("{LOG_FILE_NAME}.log"))
}

/// Open the log file for appending, wrapped in a `BufWriter`.
fn open_log_writer(dir: &Path) -> Option<BufWriter<File>> {
    let path = log_file_path(dir);
    let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
    Some(BufWriter::new(file))
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
/// Renames `mod.log` to `mod_YYYY-MM-DD.log` (using the date of the last entry).
/// If the file contains no valid timestamps, it gets truncated.
/// If an archive with that name already exists, the rotation is skipped.
fn rotate_log_file(dir: &Path, today: NaiveDate) {
    let log_file = log_file_path(dir);
    if !log_file.exists() {
        return;
    }

    match last_log_date(&log_file) {
        Some(last_date) if last_date < today => {
            let archive = dir.join(format!(
                "{LOG_FILE_NAME}_{}.log",
                last_date.format("%Y-%m-%d")
            ));
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
/// Recognises archives named `mod_YYYY-MM-DD.log` by parsing the first 10 characters after the prefix as a date.
fn cleanup_old_archives(dir: &Path, today: NaiveDate) {
    let prefix = format!("{LOG_FILE_NAME}_");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        let Some(rest) = name.strip_prefix(prefix.as_str()) else {
            continue;
        };
        if !rest.ends_with(".log") || rest.len() < 14 {
            // "YYYY-MM-DD.log" is 14 chars minimum
            continue;
        }
        let Ok(file_date) = NaiveDate::parse_from_str(&rest[..10], "%Y-%m-%d") else {
            continue;
        };

        if (today - file_date).num_days() > MAX_LOG_AGE_DAYS {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// ---- Background writer thread ---------------------------------------------

/// Background thread that receives pre-formatted log lines and writes them to `mod.log`.
///
/// Drains all queued messages before flushing, which naturally batches writes during bursts.
/// Handles date-based rotation and archive cleanup without blocking the game thread.
fn writer_thread(rx: mpsc::Receiver<String>, dir: PathBuf) {
    let mut current_date = Local::now().date_naive();
    let mut writer = open_log_writer(&dir);

    while let Ok(first) = rx.recv() {
        // Check rotation before writing
        let today = Local::now().date_naive();
        if today != current_date {
            // Release the file handle before rotating
            drop(writer.take());
            rotate_log_file(&dir, today);
            cleanup_old_archives(&dir, today);
            current_date = today;
            writer = open_log_writer(&dir);
        }

        // Write the first message + drain any additional queued messages
        if let Some(ref mut w) = writer {
            let _ = writeln!(w, "{first}");
            while let Ok(line) = rx.try_recv() {
                let _ = writeln!(w, "{line}");
            }
            let _ = w.flush();
        }
    }
}

// ---- Logger implementation ------------------------------------------------

/// Custom `log::Log` implementation that writes to `mod.log` in Daystrom's bit-log format via a background
/// writer thread.
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
    /// Always returns `true` because compile-time level filtering via Cargo features (`max_level_debug` /
    /// `max_level_trace`) already eliminates unwanted levels at zero cost.
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    /// Format a log line and send it to the background writer thread.
    fn log(&self, record: &Record) {
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

        let formatted = format!(
            "{now} {level:<5} [{component:<20}] ({file:<30}: {line:>4}): {}",
            record.args()
        );

        let _ = sender.send(formatted);
    }

    /// Flush is a no-op; the background thread flushes after each batch.
    fn flush(&self) {}
}

// ---- Public API -----------------------------------------------------------

/// Initialise the mod logger as the global `log` logger.
///
/// Runs startup log rotation, spawns the background writer thread, then registers the logger.
/// Must be called exactly once, typically from the `#[ctor]` entrypoint.
/// Panics if a logger has already been set.
pub fn init() {
    let Some(dir) = log_dir() else { return };
    fs::create_dir_all(&dir).ok();

    // Rotate before spawning the writer so the file is clean
    let today = Local::now().date_naive();
    rotate_log_file(&dir, today);
    cleanup_old_archives(&dir, today);

    // Spawn background writer
    let (tx, rx) = mpsc::channel();
    LOG_SENDER.get_or_init(|| tx);

    let writer_dir = dir.clone();
    thread::Builder::new()
        .name("mod-log-writer".to_string())
        .spawn(move || writer_thread(rx, writer_dir))
        .expect("failed to spawn log writer thread");

    log::set_logger(&LOGGER).expect("logger already initialised");
    log::set_max_level(log::LevelFilter::Trace);
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
        format!("{date}T22:00:00.000+01:00 INFO  [Mod                 ] (lib.rs                        :   18): {msg}\n")
    }

    // -- log_file_path --

    #[test]
    fn log_file_path_format() {
        let path = log_file_path(Path::new("/some/dir"));
        assert_eq!(path, PathBuf::from("/some/dir/mod.log"));
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
        rotate_log_file(&dir, date(2026, 3, 17));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn rotate_keeps_todays_file() {
        let dir = test_dir("rotate_today");
        let log = dir.join("mod.log");
        fs::write(&log, log_line("2026-03-16", "Today")).unwrap();
        rotate_log_file(&dir, date(2026, 3, 16));
        assert!(log.exists());
        assert!(!dir.join("mod_2026-03-16.log").exists());
    }

    #[test]
    fn rotate_archives_old_file() {
        let dir = test_dir("rotate_archive");
        let log = dir.join("mod.log");
        fs::write(&log, log_line("2026-03-15", "Yesterday")).unwrap();
        rotate_log_file(&dir, date(2026, 3, 16));
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
        rotate_log_file(&dir, date(2026, 3, 16));
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
        rotate_log_file(&dir, date(2026, 3, 16));
        assert!(log.exists());
        assert_eq!(fs::read_to_string(&log).unwrap(), "");
    }

    // -- cleanup_old_archives --

    #[test]
    fn cleanup_deletes_old_archives() {
        let dir = test_dir("cleanup_old");
        fs::write(dir.join("mod_2026-01-01.log"), "old").unwrap();
        fs::write(dir.join("mod_2026-03-15.log"), "recent").unwrap();
        cleanup_old_archives(&dir, date(2026, 3, 16));
        assert!(!dir.join("mod_2026-01-01.log").exists());
        assert!(dir.join("mod_2026-03-15.log").exists());
    }

    #[test]
    fn cleanup_keeps_recent_archives() {
        let dir = test_dir("cleanup_recent");
        fs::write(dir.join("mod_2026-03-10.log"), "recent").unwrap();
        fs::write(dir.join("mod_2026-03-15.log"), "very recent").unwrap();
        cleanup_old_archives(&dir, date(2026, 3, 16));
        assert!(dir.join("mod_2026-03-10.log").exists());
        assert!(dir.join("mod_2026-03-15.log").exists());
    }

    #[test]
    fn cleanup_ignores_unrelated_files() {
        let dir = test_dir("cleanup_unrelated");
        fs::write(dir.join("other_2026-01-01.log"), "unrelated").unwrap();
        fs::write(dir.join("mod.log"), "active log").unwrap();
        fs::write(dir.join("readme.txt"), "docs").unwrap();
        cleanup_old_archives(&dir, date(2026, 3, 16));
        assert!(dir.join("other_2026-01-01.log").exists());
        assert!(dir.join("mod.log").exists());
        assert!(dir.join("readme.txt").exists());
    }

    #[test]
    fn cleanup_ignores_invalid_date_format() {
        let dir = test_dir("cleanup_invalid_date");
        fs::write(dir.join("mod_not-a-date.log"), "bad").unwrap();
        fs::write(dir.join("mod_20260101.log"), "no dashes").unwrap();
        cleanup_old_archives(&dir, date(2026, 3, 16));
        assert!(dir.join("mod_not-a-date.log").exists());
        assert!(dir.join("mod_20260101.log").exists());
    }

    // -- writer_thread --

    #[test]
    fn writer_thread_writes_and_flushes() {
        let dir = test_dir("writer_writes");
        let (tx, rx) = mpsc::channel();
        let writer_dir = dir.clone();
        let handle = thread::spawn(move || writer_thread(rx, writer_dir));

        tx.send("2026-03-16T22:00:00.000+01:00 INFO  first message".to_string()).unwrap();
        tx.send("2026-03-16T22:00:01.000+01:00 INFO  second message".to_string()).unwrap();
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
        let handle = thread::spawn(move || writer_thread(rx, writer_dir));

        drop(tx);
        // Thread should exit cleanly without hanging
        handle.join().unwrap();
    }
}
