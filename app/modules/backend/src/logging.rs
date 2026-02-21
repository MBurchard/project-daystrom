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

use colored::Colorize;
use log::{Level, LevelFilter};
use tauri::plugin::TauriPlugin;
use tauri_plugin_log::{Builder, Target, TargetKind, TimezoneStrategy, fern};

/// Unit Separator — delimiter between logger name and message from JS frontend.
const SEP: char = '\x1F';

/// Display width for the logger name in log output.
/// Matches bit-log's default. Names are right-padded or left-truncated to this width.
const LOGGER_NAME_WIDTH: usize = 20;

/// Display width for the file path in log output.
/// Paths longer than this are middle-truncated with "...".
const FILE_PATH_WIDTH: usize = 30;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

/// Build the tauri-plugin-log plugin with our custom format and targets.
pub fn build_plugin() -> TauriPlugin<tauri::Wry> {
    Builder::new()
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .level(LevelFilter::Debug)
        .level_for("tao", LevelFilter::Warn)
        .level_for("wry", LevelFilter::Warn)
        .format(format_log)
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir { file_name: None }),
        ])
        .build()
}
