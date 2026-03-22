import type {GameStatus} from '@generated/GameStatus';
import type {Ref} from 'vue';
import {getLogger} from '@app/log';
import {getVersion} from '@tauri-apps/api/app';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {ref} from 'vue';

const log = getLogger('App');

const DEFAULT_GAME_STATUS: GameStatus = {
  installed: false,
  game_version: null,
  mod_available: false,
  mod_installable: false,
  mod_deployed: false,
  mod_outdated: false,
  mod_removable: false,
  game_running: false,
  launcher_running: false,
  remote_version: null,
  update_check_failed: false,
  game_started_by_us: false,
  launcher_started_by_us: false,
  update_available: false,
  can_launch: false,
  can_install_mod: false,
  can_remove_mod: false,
  can_launch_updater: false,
  should_block_quit: false,
  version_check_class: 'neutral',
};

// ---- Public Interface -----------------------------------------------------------

export interface GameState {
  /** App version string from Tauri. */
  version: Readonly<Ref<string>>;
  /** Full game status from the backend. */
  status: Readonly<Ref<GameStatus>>;
  /** True while the initial status load is in flight. */
  loading: Readonly<Ref<boolean>>;
  /** Fatal error during the initial status load. */
  error: Readonly<Ref<string | null>>;
  /** Error from the last user-triggered action. */
  actionError: Readonly<Ref<string | null>>;
  /** Whether a user action is currently in flight. */
  actionPending: Readonly<Ref<boolean>>;
  /** Prepare the mod (patch entitlements on macOS, deploy DLL on Windows). */
  installMod: () => void;
  /** Remove the deployed mod from the game directory. */
  removeMod: () => void;
  /** Open the Scopely launcher for updating. */
  openUpdater: () => void;
  /** Launch the game with the mod injected. Optionally specify a profile. */
  launchGame: (profile?: string) => void;
  /** Register event listeners and load the initial state. Call from onMounted. */
  init: () => void;
  /** Unregister event listeners. Call from onUnmounted. */
  destroy: () => void;
}

/**
 * Composable that encapsulates all game state management, backend communication,
 * event handling, and user actions.
 *
 * The backend computes all derived state (guards, version check class, etc.).
 * The frontend only displays data and dispatches user actions.
 *
 * @returns reactive state, actions, and lifecycle functions
 */
export function useGameState(): GameState {
  // ---- Reactive State -------------------------------------------------------------

  const version = ref('');
  const status = ref<GameStatus>({...DEFAULT_GAME_STATUS});
  const loading = ref(true);
  const error = ref<string | null>(null);
  const actionError = ref<string | null>(null);
  const actionPending = ref(false);

  let unlistenGameStatus: (() => void) | null = null;

  // ---- Actions ----------------------------------------------------------------------

  /**
   * Run a backend command triggered by a user action (button click).
   *
   * Sets `actionPending` while the command is in flight and captures errors in `actionError`.
   *
   * @param command - the Tauri command name to invoke
   */
  function runAction(command: string): void {
    actionPending.value = true;
    actionError.value = null;
    invoke(command)
      .catch((err) => {
        actionError.value = String(err);
      })
      .finally(() => {
        actionPending.value = false;
      });
  }

  /**
   * Fetch cached data from the backend without triggering expensive operations.
   *
   * @param command - the Tauri command name to invoke
   * @returns the cached data, or null on error
   */
  async function getData<T>(command: string): Promise<T | null> {
    try {
      return await invoke<T | null>(command);
    } catch (err) {
      log.error(`Failed to fetch ${command}:`, err);
      return null;
    }
  }

  /**
   * Prepare the mod for use (patch entitlements on macOS, deploy DLL on Windows).
   */
  function installMod(): void {
    log.debug('User clicked Install Mod');
    runAction('prepare_mod');
  }

  /**
   * Remove the deployed mod from the game directory.
   */
  function removeMod(): void {
    log.debug('User clicked Remove Mod');
    runAction('remove_mod');
  }

  /**
   * Open the Scopely launcher for updating.
   */
  function openUpdater(): void {
    log.debug('User clicked Update');
    runAction('launch_updater');
  }

  /**
   * Launch the game with the mod injected.
   *
   * @param profile - optional profile stem (e.g. "106_Nabor" or "new_account")
   */
  function launchGame(profile?: string): void {
    log.debug('User clicked Launch Game');
    actionPending.value = true;
    actionError.value = null;
    invoke('launch_game', {profile: profile ?? null})
      .catch((err) => {
        actionError.value = String(err);
      })
      .finally(() => {
        actionPending.value = false;
      });
  }

  // ---- Lifecycle --------------------------------------------------------------------

  /**
   * Register event listeners and load the initial state.
   * Must be called from `onMounted`.
   */
  function init(): void {
    log.debug('App.vue mounted');

    getVersion().then((v) => {
      version.value = v;
    }).catch(reason => log.error('Failed to get app version:', reason));

    // Fetch cached status from the backend store (maybe null if the monitor hasn't finished
    // its initial detection yet; the game-status event will arrive shortly after).
    getData<GameStatus>('get_cached_game_status').then((cached) => {
      if (cached) {
        applyGameStatus(cached);
      }
    }).catch(/* v8 ignore next @preserve -- only a defensive guard */ reason =>
      log.error('Failed to apply cached game status:', reason));

    // Backend store pushes status on every state change (process updates, mod actions, etc.)
    listen<GameStatus>('game-status', (event) => {
      applyGameStatus(event.payload);
    }).then((unlisten) => {
      unlistenGameStatus = unlisten;
    }).catch(reason => log.error('Failed to listen for game-status:', reason));
  }

  /**
   * Apply a GameStatus update to all reactive states.
   *
   * Shared by the initial cached-data fetch and the game-status event listener.
   *
   * @param s - the incoming game status from the backend store
   */
  function applyGameStatus(s: GameStatus): void {
    status.value = s;
    loading.value = false;
  }

  /**
   * Unregister all event listeners.
   * Must be called from `onUnmounted`.
   */
  function destroy(): void {
    if (unlistenGameStatus) {
      unlistenGameStatus();
    }
  }

  return {
    version,
    status,
    loading,
    error,
    actionError,
    actionPending,
    installMod,
    removeMod,
    openUpdater,
    launchGame,
    init,
    destroy,
  };
}
