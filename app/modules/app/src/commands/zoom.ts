/**
 * Typed wrappers for backend-owned application zoom commands.
 */

import type {UiZoomAction} from '@generated/UiZoomAction';
import type {UiZoomState} from '@generated/UiZoomState';
import {invoke} from '@tauri-apps/api/core';

/**
 * Ask the backend to apply and persist one application zoom action.
 *
 * @param action - Browser-style zoom intent.
 * @returns The authoritative zoom state after applying the action.
 */
export function changeUiZoom(action: UiZoomAction): Promise<UiZoomState> {
  return invoke('change_ui_zoom', {action});
}
