use colored::Colorize;
use log::{Level, LevelFilter};
use tauri::Manager;
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
    let len = s.len();
    if len > width {
        s[len - width..].to_string()
    } else {
        format!("{s:<width$}")
    }
}

/// Pad or middle-truncate a path to exactly `width` characters.
/// Keeps the beginning and end of the path, replaces the middle with "...".
/// Short strings are right-padded with spaces.
fn fit_path(s: &str, width: usize) -> String {
    let len = s.len();
    if len <= width {
        return format!("{s:<width$}");
    }
    // 3 chars for "...", split remaining space: more at end (filename matters most)
    let available = width - 3;
    let end_len = (available + 1) / 2;
    let start_len = available - end_len;
    format!("{}...{}", &s[..start_len], &s[len - end_len..])
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

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            Builder::new()
                .timezone_strategy(TimezoneStrategy::UseLocal)
                .level(LevelFilter::Debug)
                .level_for("tao", LevelFilter::Warn)
                .level_for("wry", LevelFilter::Warn)
                .format(format_log)
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::Webview),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        .setup(|app| {
            let version = &app.package_info().version;
            log::info!("Skynet {version} initialised");
            #[cfg(debug_assertions)]
            if std::env::var("SKYNET_DEVTOOLS").as_deref() != Ok("0") {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
                log::debug!("DevTools opened");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
