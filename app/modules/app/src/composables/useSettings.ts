import type {GameSettings} from '@generated/GameSettings';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {ref} from 'vue';

const log = getLogger('Settings');

// ---- State -----------------------------------------------------------------

const settings = ref<GameSettings>({ui: {scale: 100}});

// ---- Public API ------------------------------------------------------------

/**
 * Composable for game settings that are sent to the mod.
 *
 * Reads all settings from the backend on init and sends the full settings object on every change.
 * The backend diffs against the current state, persists changes and broadcasts them to connected
 * mods via WebSocket.
 */
export function useSettings() {
  /**
   * Load game settings from the backend.
   *
   * Called once during app startup to synchronize the UI with persisted values.
   */
  function init() {
    invoke<GameSettings>('get_game_settings')
      .then((value) => {
        settings.value = value;
        log.debug(`Loaded game settings: ${JSON.stringify(value)}`);
      })
      .catch((err: unknown) => {
        log.error(`Failed to load game settings: ${err}`);
      });
  }

  /**
   * Send the current settings to the backend for persistence and broadcast.
   *
   * The backend diffs the received settings against the current state and only emits events for
   * fields that actually changed.
   */
  function save() {
    invoke('set_game_settings', {settings: settings.value})
      .catch((err: unknown) => {
        log.error(`Failed to save game settings: ${err}`);
      });
  }

  return {
    settings,
    save,
    init,
  };
}
