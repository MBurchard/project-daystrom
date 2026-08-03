import type {GameSettings} from '@generated/GameSettings';

import {getGameSettings, setGameSettings} from '@app/commands/settings';
import {getLogger} from '@app/log';
import {ref} from 'vue';

const log = getLogger('Settings');

// ---- State -----------------------------------------------------------------

const settings = ref<GameSettings>({
  ui: {},
  banners: {},
  cargo_view: {},
  slider_limits: {},
  shortcuts: {},
});

type SettingsUpdater = (value: GameSettings) => void;

// ---- Public API ------------------------------------------------------------

/**
 * Composable for game settings that are sent to the mod.
 *
 * Reads all settings from the backend on init and sends the full settings object on every change.
 * The backend diffs against the current state, persists changes, and broadcasts them to connected mods via WebSocket.
 */
export function useSettings() {
  /**
   * Load game settings from the backend.
   *
   * Called once during app startup to synchronize the UI with persisted values.
   */
  function init() {
    getGameSettings()
      .then((value) => {
        settings.value = value;
        log.debug(`Loaded game settings: ${JSON.stringify(value)}`);
      })
      .catch(reason => log.error(`Failed to load game settings: ${reason}`));
  }

  /**
   * Send the current settings to the backend for persistence and broadcast.
   *
   * The backend diffs the received settings against the current state and only emits events for fields that
   * actually changed.
   */
  function save() {
    setGameSettings(settings.value)
      .catch(reason => log.error(`Failed to save game settings: ${reason}`));
  }

  /**
   * Apply a settings mutation and persist the complete settings object.
   *
   * @param updater - Synchronous mutation to apply to the current settings.
   */
  function update(updater: SettingsUpdater) {
    updater(settings.value);
    save();
  }

  /**
   * Assign or disable a keyboard shortcut.
   *
   * @param key - Shortcut action identifier.
   * @param code - Physical key code, or an empty string to disable the shortcut.
   */
  function setShortcut(key: string, code: string) {
    update((value) => {
      const shortcuts = value.shortcuts ??= {};
      shortcuts[key] = code;
    });
  }

  /**
   * Enable or suppress an individual toast banner type.
   *
   * @param name - ToastState variant name.
   * @param enabled - Whether the banner should be shown.
   */
  function setBannerTypeEnabled(name: string, enabled: boolean) {
    update((value) => {
      const disabledTypes = new Set(value.banners.disabled_types ?? []);
      if (enabled) {
        disabledTypes.delete(name);
      } else {
        disabledTypes.add(name);
      }
      value.banners.disabled_types = [...disabledTypes].sort();
    });
  }

  return {
    settings,
    init,
    update,
    setShortcut,
    setBannerTypeEnabled,
  };
}
