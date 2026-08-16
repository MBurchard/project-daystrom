import type {ILogEvent, LogLevel} from '@mburchard/bit-log/definitions';
import {AbstractBaseAppender} from '@mburchard/bit-log/appender/AbstractBaseAppender';
import {toLogLevelString} from '@mburchard/bit-log/definitions';
import {debug, error, info, trace, warn} from '@tauri-apps/plugin-log';

// Unit Separator (U+001F) as a delimiter between the logger name and message.
// The Rust formatter splits on this to extract the logger name.
const SEP = '\x1F';

/**
 * Map bit-log levels to their corresponding `@tauri-apps/plugin-log` IPC functions.
 */
const levelFunctions: Partial<Record<LogLevel, typeof info>> = {
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

  /** IPC operations that have not settled yet. */
  private readonly pendingOperations = new Set<Promise<void>>();

  /** Shared close operation for concurrent callers. */
  private closePromise?: Promise<void>;

  /**
   * Wait until all IPC operations that are currently running have settled.
   *
   * The loop also picks up operations started while an earlier batch is settling.
   * @returns a promise that resolves when no pending operation remains
   */
  private async waitForPendingOperations(): Promise<void> {
    while (this.pendingOperations.size > 0) {
      await Promise.allSettled(this.pendingOperations);
    }
  }

  /**
   * Flush all pending Tauri log calls during logging shutdown.
   *
   * Concurrent calls share the same promise. Once it settles, the appender can be used and closed
   * again as required by bit-log's appender lifecycle.
   * @returns a promise that resolves after all pending IPC operations have settled
   */
  close(): Promise<void> {
    this.closePromise ??= this.waitForPendingOperations().finally(() => {
      this.closePromise = undefined;
    });
    return this.closePromise;
  }

  /**
   * Forward a log event to the Rust backend via the appropriate plugin-log function.
   * @param event - the log event from bit-log
   */
  async doHandle(event: ILogEvent): Promise<void> {
    if (this.disabled) {
      return;
    }
    const logFn = levelFunctions[toLogLevelString(event.level) as LogLevel] ?? info;

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

    const operation = logFn(text, options);
    this.pendingOperations.add(operation);

    try {
      await operation;
    } catch {
      // Tauri not available — disable permanently to avoid repeated failures
      this.disabled = true;
    } finally {
      this.pendingOperations.delete(operation);
    }
  }
}
