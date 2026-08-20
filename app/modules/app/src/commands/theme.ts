import type {AppTheme} from '@generated/AppTheme';
import {invoke} from '@tauri-apps/api/core';

/**
 * Load the application theme selected by the backend.
 *
 * @returns The persisted theme or Daystrom's default theme.
 */
export function getAppTheme(): Promise<AppTheme> {
  return invoke<AppTheme>('get_app_theme');
}

/**
 * Persist an explicit application theme selection.
 *
 * @param theme - Supported theme selected by the user.
 * @returns A promise that resolves after the setting has been accepted.
 */
export function setAppTheme(theme: AppTheme): Promise<void> {
  return invoke('set_app_theme', {theme});
}
