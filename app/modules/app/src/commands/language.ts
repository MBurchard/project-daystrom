import type {AppLanguage} from '@generated/AppLanguage';
import {invoke} from '@tauri-apps/api/core';

/**
 * Resolve the persisted or system-derived application language in the backend.
 *
 * @param systemLocale - Raw locale reported by the WebView.
 * @returns The language selected by Daystrom's backend policy.
 */
export function getAppLanguage(systemLocale: string): Promise<AppLanguage> {
  return invoke<AppLanguage>('get_app_language', {systemLocale});
}

/**
 * Persist an explicit application language selection.
 *
 * @param language - Supported language selected by the user.
 * @returns A promise that resolves after the setting has been accepted.
 */
export function setAppLanguage(language: AppLanguage): Promise<void> {
  return invoke('set_app_language', {language});
}
