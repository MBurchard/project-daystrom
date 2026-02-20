import type {ILogEvent} from '@mburchard/bit-log/definitions';
import {AbstractBaseAppender} from '@mburchard/bit-log/appender/AbstractBaseAppender';
import {debug, error, info, trace, warn} from '@tauri-apps/plugin-log';

// Unit Separator (U+001F) as delimiter between logger name and message.
// The Rust formatter splits on this to extract the logger name.
const SEP = '\x1F';

/**
 * Map bit-log level strings to their corresponding `@tauri-apps/plugin-log` IPC functions.
 */
const levelFunctions: Record<string, typeof info> = {
  TRACE: trace,
  DEBUG: debug,
  INFO: info,
  WARN: warn,
  ERROR: error,
  FATAL: error,
};

/**
 * Strip the browser origin (e.g. `http://localhost:1420/`) from a call-site file path
 * so Rust receives a clean relative path like `modules/app/src/App.vue`.
 * @param file - the raw file path from the call site
 * @returns the path without origin prefix, or the original string on failure
 */
function stripOrigin(file: string): string {
  try {
    return new URL(file).pathname.slice(1);
  } catch {
    return file;
  }
}

/**
 * Resolve a single payload element to a string representation.
 * Lazy functions are evaluated; all other values are formatted via `formatAny`.
 * @param value - the payload element to resolve
 * @param formatAny - the formatter from AbstractBaseAppender
 * @returns the string representation of the value
 */
function resolvePayloadItem(value: unknown, formatAny: (v: unknown) => string): string {
  if (typeof value === 'function') {
    try {
      return String(value());
    } catch {
      return '[lazy eval error]';
    }
  }
  return formatAny(value);
}

/**
 * Custom bit-log appender that forwards log events to the Rust backend
 * via `@tauri-apps/plugin-log` IPC calls.
 *
 * Uses the existing `\x1F` protocol so the Rust `format_log()` function
 * can extract the logger name from the message.
 *
 * Gracefully disables itself when Tauri is not available (e.g. pure browser dev).
 */
export class TauriAppender extends AbstractBaseAppender {
  /** Once set to `true`, no further IPC calls are attempted. */
  private disabled = false;

  /**
   * Forward a log event to the Rust backend via the appropriate plugin-log function.
   * @param event - the log event from bit-log
   */
  async handle(event: ILogEvent): Promise<void> {
    if (this.disabled || !this.willHandle(event)) {
      return;
    }

    const levelStr = typeof event.level === 'string' ? event.level : 'INFO';
    const logFn = levelFunctions[levelStr] ?? info;

    // Build the message string from the payload
    let message: string;
    if (typeof event.payload === 'function') {
      try {
        message = event.payload();
      } catch {
        message = '[lazy eval error]';
      }
    } else {
      message = event.payload
        .map(item => resolvePayloadItem(item, v => this.formatAny(v)))
        .join(' ');
    }

    // Compose the IPC message with the \x1F protocol
    const text = `${event.loggerName}${SEP}${message}`;

    // Build LogOptions from call-site info
    const options = event.callSite ?
        {file: stripOrigin(event.callSite.file), line: event.callSite.line} :
      undefined;

    try {
      await logFn(text, options);
    } catch {
      // Tauri not available — disable permanently to avoid repeated failures
      this.disabled = true;
    }
  }
}
