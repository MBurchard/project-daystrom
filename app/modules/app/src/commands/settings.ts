/**
 * Typed wrappers for settings-related Tauri commands.
 *
 * Centralizes command names and parameter keys so that tests catch mismatches between frontend and backend in one
 * place instead of silently failing at runtime.
 */

import type {GameSettings} from '@generated/GameSettings';
import {invoke} from '@tauri-apps/api/core';

/**
 * Load the current game settings from the backend.
 *
 * @returns the persisted settings (or defaults if no file exists yet)
 */
export function getGameSettings(): Promise<GameSettings> {
  return invoke<GameSettings>('get_game_settings');
}

/**
 * Send updated game settings to the backend for persistence and broadcast.
 *
 * The backend diffs against the current state and only emits events for fields that actually changed.
 *
 * @param settings - the full settings object (not a partial)
 */
export function setGameSettings(settings: GameSettings): Promise<void> {
  return invoke('set_game_settings', {settings});
}
