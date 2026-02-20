import {configureLogging, useLog} from '@mburchard/bit-log';
import {ConsoleAppender} from '@mburchard/bit-log/appender/ConsoleAppender';
import {TauriAppender} from './TauriAppender';

/**
 * Configure the bit-log logging system with two appender:
 * - CONSOLE: formats and writes to browser DevTools
 * - TAURI: forwards structured events to the Rust backend via IPC
 *
 * Must be called once at app startup before any `useLog()` calls.
 */
configureLogging({
  appender: {
    CONSOLE: {
      Class: ConsoleAppender,
      colored: false,
      useSpecificMethods: true,
    },
    TAURI: {
      Class: TauriAppender,
    },
  },
  root: {
    level: 'DEBUG',
    includeCallSite: true,
    appender: ['CONSOLE', 'TAURI'],
  },
});

export {useLog};
