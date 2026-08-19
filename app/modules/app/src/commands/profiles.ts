/**
 * Typed wrappers for profile-related Tauri commands.
 */

import {invoke} from '@tauri-apps/api/core';

/**
 * Delete one known local profile and its locally stored login data.
 *
 * @param stem - backend-owned profile stem identifying the selected account
 * @returns a promise that resolves after the profile state has been updated
 */
export function deleteLocalProfile(stem: string): Promise<void> {
  return invoke('delete_local_profile', {stem});
}
