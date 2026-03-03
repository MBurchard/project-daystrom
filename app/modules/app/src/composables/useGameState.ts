import type {GameStatus} from '@generated/GameStatus';
import type {ProcessStatus} from '@generated/ProcessStatus';
import type {UpdateCheck} from '@generated/UpdateCheck';
import type {Ref} from 'vue';

import {getLogger} from '@app/log';
import {getVersion} from '@tauri-apps/api/app';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {computed, ref} from 'vue';

const log = getLogger('App');

const DEFAULT_GAME_STATUS: GameStatus = {
  installed: false,
  install_dir: null,
  executable: null,
  game_version: null,
  entitlements_ok: false,
  granted_entitlements: [],
  missing_entitlements: [],
  mod_available: false,
  mod_deployed: false,
  mod_outdated: false,
  game_running: false,
  launcher_running: false,
};

// ---- Public Interface -----------------------------------------------------------

export interface GameState {
  /** App version string from Tauri. */
  version: Readonly<Ref<string>>;
  /** Full game status from the backend. */
  status: Readonly<Ref<GameStatus>>;
  /** True while the initial status load is in flight. */
  loading: Readonly<Ref<boolean>>;
  /** Whether the game is installed (false while status is loading). */
  installed: Readonly<Ref<boolean>>;
  /** Fatal error during the initial status load. */
  error: Readonly<Ref<string | null>>;
  /** Error from the last user-triggered action. */
  actionError: Readonly<Ref<string | null>>;
  /** Whether a user action is currently in flight. */
  actionPending: Readonly<Ref<boolean>>;
  /** Remote game version from the Scopely update API. */
  remoteVersion: Readonly<Ref<number | null>>;
  /** Whether the last update check failed. */
  updateCheckFailed: Readonly<Ref<boolean>>;
  /** Whether the Scopely launcher process is running. */
  launcherRunning: Readonly<Ref<boolean>>;
  /** Whether the game process is running. */
  gameRunning: Readonly<Ref<boolean>>;
  /** Whether we started the updater in this session. */
  updaterStartedByUs: Readonly<Ref<boolean>>;
  /** True when the remote version exceeds the installed version. */
  updateAvailable: Readonly<Ref<boolean>>;
  /** True when all preconditions for launching the game are met. */
  canLaunch: Readonly<Ref<boolean>>;
  /** CSS class for the version-check checklist item. */
  versionCheckClass: Readonly<Ref<string>>;
  /** True when the mod install/reinstall button should be enabled. */
  canInstallMod: Readonly<Ref<boolean>>;
  /** True when the mod remove button should be enabled. */
  canRemoveMod: Readonly<Ref<boolean>>;
  /** True when the updater button should be enabled. */
  canLaunchUpdater: Readonly<Ref<boolean>>;
  /** Prepare the mod (patch entitlements on macOS, deploy DLL on Windows). */
  installMod: () => void;
  /** Remove the deployed mod from the game directory. */
  removeMod: () => void;
  /** Open the Scopely launcher for updating. */
  openUpdater: () => void;
  /** Launch the game with the mod injected. */
  launchGame: () => void;
  /** Check for a game update via the Scopely update API. */
  checkForUpdate: () => void;
  /** Register event listeners and load the initial state. Call from onMounted. */
  init: () => void;
  /** Unregister event listeners. Call from onUnmounted. */
  destroy: () => void;
}

/**
 * Composable that encapsulates all game state management, backend communication,
 * event handling, computed guards, and user actions.
 *
 * @returns reactive state, computed guards, actions, and lifecycle functions
 */
export function useGameState(): GameState {
  // ---- Reactive State -------------------------------------------------------------

  const version = ref('');
  const status = ref<GameStatus>({...DEFAULT_GAME_STATUS});
  const loading = ref(true);
  const error = ref<string | null>(null);
  const actionError = ref<string | null>(null);
  const actionPending = ref(false);
  const remoteVersion = ref<number | null>(null);
  const updateCheckFailed = ref(false);
  const launcherRunning = ref(false);
  const gameRunning = ref(false);
  const updaterStartedByUs = ref(false);

  let unlistenProcessStatus: (() => void) | null = null;
  let unlistenGameStatus: (() => void) | null = null;
  let unlistenUpdateCheck: (() => void) | null = null;

  // ---- Computed Guards --------------------------------------------------------------

  /**
   * Whether the game is installed.
   * @returns false while status is still loading or when the game is not found
   */
  const installed = computed(() => status.value.installed);

  /**
   * Whether a game update is available.
   * @returns true when the remote version exceeds the installed version
   */
  const updateAvailable = computed(() => {
    const s = status.value;
    if (!s.game_version || remoteVersion.value == null) {
      return false;
    }
    return remoteVersion.value > s.game_version;
  });

  /**
   * Whether all conditions for launching the game are met.
   * @returns true when installed, mod deployed, mod available, nothing running, no update
   */
  const canLaunch = computed(() => {
    const s = status.value;
    return s.installed && s.mod_deployed && s.mod_available &&
      !gameRunning.value && !launcherRunning.value && !updateAvailable.value;
  });

  /**
   * CSS class for the version check checklist item.
   * @returns 'ok' when up to date, 'warn' when update available, 'neutral' when check failed/pending
   */
  const versionCheckClass = computed(() => {
    if (updateAvailable.value) {
      return 'warn';
    }
    if (updateCheckFailed.value) {
      return 'neutral';
    }
    if (remoteVersion.value != null) {
      return 'ok';
    }
    return 'neutral';
  });

  /**
   * Whether the mod install/reinstall button should be enabled.
   * @returns true when game is installed, mod available, nothing running, and no update pending
   */
  const canInstallMod = computed(() => {
    const s = status.value;
    return s.installed && s.mod_available &&
      !gameRunning.value && !launcherRunning.value && !updateAvailable.value;
  });

  /**
   * Whether the mod remove button should be enabled.
   * @returns true when installed, mod deployed or outdated, nothing running
   */
  const canRemoveMod = computed(() => {
    const s = status.value;
    return s.installed && (s.mod_deployed || s.mod_outdated) &&
      !gameRunning.value && !launcherRunning.value;
  });

  /**
   * Whether the Update button should be shown and enabled.
   * @returns true when an update is available, launcher is not already running, and no action pending
   */
  const canLaunchUpdater = computed(() => {
    return updateAvailable.value && !launcherRunning.value;
  });

  // ---- Actions ----------------------------------------------------------------------

  /**
   * Run a backend command with a shared pending / error lifecycle.
   *
   * @param command - the Tauri command name to invoke
   * @param onSuccess - callback receiving the command result on success
   */
  function runAction<T>(command: string, onSuccess: (result: T) => void): void {
    actionPending.value = true;
    actionError.value = null;
    invoke<T>(command)
      .then(onSuccess)
      .catch((err) => {
        actionError.value = String(err);
      })
      .finally(() => {
        actionPending.value = false;
      });
  }

  /**
   * Apply a full GameStatus from the backend to the local state.
   *
   * @param result - the refreshed game status
   */
  function applyStatus(result: GameStatus): void {
    status.value = result;
    gameRunning.value = result.game_running;
    launcherRunning.value = result.launcher_running;
  }

  /**
   * Check for a game update via the Scopely update API and cache the result.
   */
  function checkForUpdate(): void {
    invoke<UpdateCheck>('check_for_update')
      .then((result) => {
        updateCheckFailed.value = false;
        remoteVersion.value = result.remote_version ?? result.installed_version;
      })
      .catch(() => {
        updateCheckFailed.value = true;
      });
  }

  /**
   * Prepare the mod for use (patch entitlements on macOS, deploy DLL on Windows).
   */
  function installMod(): void {
    log.debug('User clicked Install Mod');
    runAction<GameStatus>('prepare_mod', applyStatus);
  }

  /**
   * Remove the deployed mod from the game directory.
   */
  function removeMod(): void {
    log.debug('User clicked Remove Mod');
    runAction<GameStatus>('remove_mod', applyStatus);
  }

  /**
   * Open the Scopely launcher for updating.
   */
  function openUpdater(): void {
    log.debug('User clicked Update');
    runAction('launch_updater', () => {
      launcherRunning.value = true;
      updaterStartedByUs.value = true;
    });
  }

  /**
   * Launch the game with the mod injected.
   */
  function launchGame(): void {
    log.debug('User clicked Launch Game');
    runAction('launch_game', () => {
      gameRunning.value = true;
    });
  }

  // ---- Lifecycle --------------------------------------------------------------------

  /**
   * Register event listeners and load the initial state.
   * Must be called from `onMounted`.
   */
  function init(): void {
    log.debug('App.vue mounted');

    getVersion()
      .then((v) => {
        version.value = v;
      })
      .catch((err) => {
        log.error(`Failed to get app version: ${err}`);
      });

    // Backend pushes process state changes while monitoring
    listen<ProcessStatus>('process-status', (event) => {
      if (!event.payload.launcher_running) {
        updaterStartedByUs.value = false;
      }
      launcherRunning.value = event.payload.launcher_running;
      gameRunning.value = event.payload.game_running;
    })
      .then((unlisten) => {
        unlistenProcessStatus = unlisten;
      })
      .catch((err) => {
        log.error(`Failed to listen for process-status: ${err}`);
      });

    // Backend pushes full status refresh when a watched process exits
    listen<GameStatus>('game-status', (event) => {
      status.value = event.payload;
      gameRunning.value = event.payload.game_running;
      launcherRunning.value = event.payload.launcher_running;
    })
      .then((unlisten) => {
        unlistenGameStatus = unlisten;
      })
      .catch((err) => {
        log.error(`Failed to listen for game-status: ${err}`);
      });

    // Backend pushes update check results during periodic API rechecks
    listen<UpdateCheck>('update-check', (event) => {
      updateCheckFailed.value = false;
      if (event.payload.remote_version != null) {
        remoteVersion.value = event.payload.remote_version;
      }
    })
      .then((unlisten) => {
        unlistenUpdateCheck = unlisten;
      })
      .catch((err) => {
        log.error(`Failed to listen for update-check: ${err}`);
      });

    // Initial full detection (once)
    invoke<GameStatus>('get_game_status')
      .then((result) => {
        status.value = result;
        gameRunning.value = result.game_running;
        launcherRunning.value = result.launcher_running;
        if (result.installed) {
          checkForUpdate();
        }
      })
      .catch((err) => {
        error.value = String(err);
        log.error(`Failed to get game status: ${err}`);
      })
      .finally(() => {
        loading.value = false;
      });
  }

  /**
   * Unregister all event listeners.
   * Must be called from `onUnmounted`.
   */
  function destroy(): void {
    if (unlistenProcessStatus) {
      unlistenProcessStatus();
    }
    if (unlistenGameStatus) {
      unlistenGameStatus();
    }
    if (unlistenUpdateCheck) {
      unlistenUpdateCheck();
    }
  }

  return {
    version,
    status,
    loading,
    installed,
    error,
    actionError,
    actionPending,
    remoteVersion,
    updateCheckFailed,
    launcherRunning,
    gameRunning,
    updaterStartedByUs,
    updateAvailable,
    canLaunch,
    versionCheckClass,
    canInstallMod,
    canRemoveMod,
    canLaunchUpdater,
    installMod,
    removeMod,
    openUpdater,
    launchGame,
    checkForUpdate,
    init,
    destroy,
  };
}
