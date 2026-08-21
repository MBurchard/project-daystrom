import type {SafetyNoticeContext} from '@generated/SafetyNoticeContext';
import {invoke} from '@tauri-apps/api/core';

/**
 * Load the platform and absolute removal paths used by the safety notice.
 *
 * @returns Platform-specific removal context from the backend.
 */
export function getSafetyNoticeContext(): Promise<SafetyNoticeContext> {
  return invoke<SafetyNoticeContext>('get_safety_notice_context');
}

/**
 * Ask the backend whether the current safety notice requires acknowledgement.
 *
 * @returns Whether Daystrom must block normal interaction with the notice.
 */
export function isSafetyNoticeRequired(): Promise<boolean> {
  return invoke<boolean>('is_safety_notice_required');
}

/** Persist acknowledgement of the current safety-notice revision. */
export function acknowledgeSafetyNotice(): Promise<void> {
  return invoke('acknowledge_safety_notice');
}
