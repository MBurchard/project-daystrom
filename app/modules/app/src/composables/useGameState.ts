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

// ---- Public Interface -----------------------------------------------------------

export interface GameState {
  /** App version string from Tauri. */
  version: Readonly<Ref<string>>;
  /** Full game status from the backend, null until the first load. */
  status: Readonly<Ref<GameStatus | null>>;
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
  /** True when the entitlements button should be enabled. */
  canPatchEntitlements: Readonly<Ref<boolean>>;
  /** True when the updater button should be enabled. */
  canLaunchUpdater: Readonly<Ref<boolean>>;
  /** Patch the game executable's entitlements. */
  fixEntitlements: () => void;
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
  const status = ref<GameStatus | null>(null);
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
   * Whether a game update is available.
   * @returns true when the remote version exceeds the installed version
   */
  const updateAvailable = computed(() => {
    const s = status.value;
    if (!s?.game_version || remoteVersion.value == null) {
      return false;
    }
    return remoteVersion.value > s.game_version;
  });

  /**
   * Whether all conditions for launching the game are met.
   * @returns true when installed, entitlements OK, mod available, nothing running, no update
   */
  const canLaunch = computed(() => {
    const s = status.value;
    return !!s?.installed && s.entitlements_ok && s.mod_available &&
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
   * Whether the entitlements button should be enabled.
   * @returns true when game is installed, nothing running, and no update pending
   */
  const canPatchEntitlements = computed(() => {
    const s = status.value;
    return !!s?.installed && !gameRunning.value && !launcherRunning.value && !updateAvailable.value;
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
   * Patch the game executable's entitlements.
   * The backend patches and returns the refreshed status in one step.
   */
  function fixEntitlements(): void {
    log.debug('User clicked Fix Entitlements');
    actionPending.value = true;
    actionError.value = null;
    invoke<GameStatus>('patch_entitlements')
      .then((result) => {
        status.value = result;
        gameRunning.value = result.game_running;
        launcherRunning.value = result.launcher_running;
      })
      .catch((err) => {
        actionError.value = String(err);
      })
      .finally(() => {
        actionPending.value = false;
      });
  }

  /**
   * Open the Scopely launcher for updating.
   * The backend starts process monitoring and pushes state updates via events.
   */
  function openUpdater(): void {
    log.debug('User clicked Update');
    actionPending.value = true;
    actionError.value = null;
    invoke('launch_updater')
      .then(() => {
        launcherRunning.value = true;
        updaterStartedByUs.value = true;
      })
      .catch((err) => {
        actionError.value = String(err);
      })
      .finally(() => {
        actionPending.value = false;
      });
  }

  /**
   * Launch the game with the mod injected.
   * The backend starts process monitoring and pushes state updates via events.
   */
  function launchGame(): void {
    log.debug('User clicked Launch Game');
    actionPending.value = true;
    actionError.value = null;
    invoke('launch_game')
      .then(() => {
        gameRunning.value = true;
      })
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
    canPatchEntitlements,
    canLaunchUpdater,
    fixEntitlements,
    openUpdater,
    launchGame,
    checkForUpdate,
    init,
    destroy,
  };
}
