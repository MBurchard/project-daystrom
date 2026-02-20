import type {LogOptions} from '@tauri-apps/plugin-log';
import {
  debug as _debug,
  error as _error,
  info as _info,
  trace as _trace,
  warn as _warn,
} from '@tauri-apps/plugin-log';
import {SourceMapConsumer} from 'source-map-js';

// Unit Separator (U+001F) as delimiter between logger name and message.
// The Rust formatter splits on this to extract the logger name.
const SEP = '\x1F';

/**
 * A named logger providing levelled log methods that resolve the caller's source location.
 */
export interface Logger {
  trace: (message: string, ...args: unknown[]) => void;
  debug: (message: string, ...args: unknown[]) => void;
  info: (message: string, ...args: unknown[]) => void;
  warn: (message: string, ...args: unknown[]) => void;
  error: (message: string, ...args: unknown[]) => void;
}

/**
 * A parsed but not yet source-mapped stack frame location.
 */
interface RawCallSite {
  url: string;
  file: string;
  line: number;
  column: number;
}

// Cached source map consumers per file URL.
const sourceMapCache = new Map<string, SourceMapConsumer | null>();

/**
 * Check whether a stack frame belongs to logger internals and should be skipped
 * when walking the stack to find the real caller.
 * @param file - the file path or URL from the stack frame
 * @returns `true` if the frame is from logging infrastructure
 * @internal
 */
export function isInternalFrame(file: string): boolean {
  return file.includes('/log/') ||
    file.includes('/log.') ||
    file.includes('\\log\\') ||
    file.includes('\\log.') ||
    file.includes('plugin-log');
}

/**
 * Parse a single stack trace line into url, line, and column.
 * Handles both V8/Chrome (`at func (file:line:col)`) and Safari/WebKit (`func@file:line:col`).
 * @param line - a single line from `Error.stack`
 * @returns the parsed call site, or `undefined` if the line cannot be parsed
 * @internal
 */
export function parseCallSiteLine(line: string): RawCallSite | undefined {
  let candidate = line.trim();

  if (candidate.startsWith('at ')) {
    candidate = candidate.slice(3).trim();
  }

  if (candidate.endsWith(')') && candidate.includes('(')) {
    const open = candidate.lastIndexOf('(');
    candidate = candidate.slice(open + 1, -1);
  }

  const atIdx = candidate.indexOf('@');
  if (atIdx >= 0) {
    candidate = candidate.slice(atIdx + 1);
  }

  const lastColon = candidate.lastIndexOf(':');
  if (lastColon <= 0)
    return undefined;
  const secondLastColon = candidate.lastIndexOf(':', lastColon - 1);
  if (secondLastColon <= 0)
    return undefined;

  const url = candidate.slice(0, secondLastColon);
  const lineNumber = Number(candidate.slice(secondLastColon + 1, lastColon));
  const columnNumber = Number(candidate.slice(lastColon + 1));

  if (!Number.isInteger(lineNumber) || !Number.isInteger(columnNumber)) {
    return undefined;
  }

  return {url, file: stripOrigin(url), line: lineNumber, column: columnNumber};
}

/**
 * Strip the origin (e.g. `http://localhost:1420/`) from a URL to get a relative path.
 * @param url - an absolute URL served by the dev server or production build
 * @returns the pathname without leading slash, or the original string on parse failure
 */
function stripOrigin(url: string): string {
  try {
    return new URL(url).pathname.slice(1);
  } catch {
    return url;
  }
}

/**
 * Walk the stack trace and return the first frame that is not from logger internals.
 * @returns the first non-internal call site, or `undefined` if none is found
 */
function rawCallerLocation(): RawCallSite | undefined {
  // eslint-disable-next-line unicorn/error-message
  const stack = new Error().stack;
  if (!stack)
    return undefined;

  for (const line of stack.split('\n')) {
    const parsed = parseCallSiteLine(line);
    if (!parsed)
      continue;
    if (isInternalFrame(parsed.url))
      continue;
    return parsed;
  }
  return undefined;
}

/**
 * Fetch and cache the inline source map for a given file URL.
 * @param url - the script URL to retrieve the source map from
 * @returns a cached or freshly parsed consumer, or `null` if unavailable
 */
async function getSourceMap(url: string): Promise<SourceMapConsumer | null> {
  const cached = sourceMapCache.get(url);
  if (cached !== undefined)
    return cached;

  try {
    const res = await fetch(url);
    const text = await res.text();
    const match = text.match(/\/\/# sourceMappingURL=data:application\/json;(?:charset=utf-8;)?base64,(.+)$/m);
    if (!match) {
      sourceMapCache.set(url, null);
      return null;
    }
    const consumer = new SourceMapConsumer(JSON.parse(atob(match[1]!)));
    sourceMapCache.set(url, consumer);
    return consumer;
  } catch {
    sourceMapCache.set(url, null);
    return null;
  }
}

/**
 * Resolve a raw (transformed) call site to the original source location via source maps.
 * Falls back to the raw location if no source map is available.
 * @param raw - the bundled/transformed call site to resolve
 * @returns file and line suitable for passing to the Tauri log plugin
 */
async function resolveLocation(raw: RawCallSite): Promise<LogOptions> {
  const consumer = await getSourceMap(raw.url);
  if (consumer) {
    const pos = consumer.originalPositionFor({line: raw.line, column: raw.column});
    if (pos.line != null) {
      return {file: pos.source ? stripOrigin(pos.source) : raw.file, line: pos.line};
    }
  }
  return {file: raw.file, line: raw.line};
}

/**
 * Format variadic arguments into a single string.
 * Strings are appended as-is, objects are JSON-serialised.
 * @param message - the base log message
 * @param args - additional values to append (strings verbatim, objects as JSON)
 * @returns the combined message string
 */
function formatArgs(message: string, args: unknown[]): string {
  if (args.length === 0)
    return message;
  const parts = args.map((arg) => {
    if (typeof arg === 'string')
      return arg;
    try {
      return JSON.stringify(arg, null, 2);
    } catch {
      return String(arg);
    }
  });
  return `${message} ${parts.join(' ')}`;
}

/**
 * Create a named logger. The name appears in the `[brackets]` part of the log line,
 * independent of the source file location.
 * @param name - the logger name shown in brackets (e.g. `'Auth'`)
 * @returns a {@link Logger} with trace/debug/info/warn/error methods
 *
 * @example
 * const log = createLogger('Auth');
 * log.info('User logged in');
 * // → 2026-02-20T16:15:07.117+01:00 INFO  [Auth                ] (src/auth.ts:  42): User logged in
 */
export function createLogger(name: string): Logger {
  const prefix = `${name}${SEP}`;
  const fire = (fn: typeof _info, msg: string, args: unknown[]) => {
    const raw = rawCallerLocation();
    const text = `${prefix}${formatArgs(msg, args)}`;
    if (!raw) {
      fn(text).catch(() => {});
      return;
    }
    resolveLocation(raw).then((location) => {
      fn(text, location).catch(() => {});
    }).catch(() => {});
  };
  return {
    trace: (msg, ...args) => fire(_trace, msg, args),
    debug: (msg, ...args) => fire(_debug, msg, args),
    info: (msg, ...args) => fire(_info, msg, args),
    warn: (msg, ...args) => fire(_warn, msg, args),
    error: (msg, ...args) => fire(_error, msg, args),
  };
}
