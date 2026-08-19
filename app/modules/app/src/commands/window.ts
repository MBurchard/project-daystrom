/**
 * Typed wrappers for main-window Tauri commands.
 */

import {invoke} from '@tauri-apps/api/core';

/**
 * Ask the backend to close or hide the main window according to current process state.
 *
 * @returns a promise that resolves after the backend accepts the close request
 */
export function closeMainWindow(): Promise<void> {
  return invoke('request_main_window_close');
}
