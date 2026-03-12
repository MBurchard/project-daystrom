import type {GameStatus} from '@generated/GameStatus';
import type {UpdateCheck} from '@generated/UpdateCheck';

import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useGameState} from '../useGameState';

// ---- Mocks --------------------------------------------------------------------------

const mockGetLogger = vi.hoisted(() => vi.fn().mockReturnValue({
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
const mockGetVersion = vi.hoisted(() => vi.fn().mockResolvedValue('1.0.0'));
const mockInvoke = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const mockListen = vi.hoisted(() => vi.fn().mockResolvedValue(vi.fn()));

vi.mock('@app/log', () => ({
  getLogger: mockGetLogger,
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: mockGetVersion,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}));

// ---- Helpers ------------------------------------------------------------------------

/**
 * Build a minimal GameStatus for testing.
 * @param overrides - fields to override on the default status
 * @returns a complete GameStatus
 */
function makeGameStatus(overrides: Partial<GameStatus> = {}): GameStatus {
  return {
    installed: true,
    game_version: 100,
    mod_available: true,
    mod_installable: true,
    mod_deployed: true,
    mod_outdated: false,
    mod_removable: false,
    game_running: false,
    launcher_running: false,
    ...overrides,
  };
}

type ListenerCallback = (event: {payload: unknown}) => void;

/**
 * Capture registered event listeners from the listen mock.
 * @returns a map of event name to callback, plus an emitEvent helper
 */
function captureListeners(): {
  listeners: Map<string, ListenerCallback>;
  emitEvent: (name: string, payload: unknown) => void;
} {
  const listeners = new Map<string, ListenerCallback>();

  mockListen.mockImplementation((eventName: string, callback: ListenerCallback) => {
    listeners.set(eventName, callback);
    return Promise.resolve(vi.fn());
  });

  return {
    listeners,
    emitEvent(name: string, payload: unknown) {
      const cb = listeners.get(name);
      if (!cb) {
        throw new Error(`No listener registered for event "${name}"`);
      }
      cb({payload});
    },
  };
}

/**
 * Init the composable with a game-status event.
 * Waits until loading is false.
 * @param statusOverrides - fields to override on the default GameStatus
 * @returns the composable instance
 */
async function initWithStatus(statusOverrides: Partial<GameStatus> = {}) {
  const status = makeGameStatus(statusOverrides);
  const {listeners, emitEvent} = captureListeners();

  const state = useGameState();
  state.init();

  // Simulate the monitor's initial game-status event
  emitEvent('game-status', status);

  await vi.waitFor(() => {
    expect(state.loading.value).toBe(false);
  });

  // Simulate the backend's async update check event
  emitEvent('update-check', {
    installed_version: status.game_version ?? 100,
    remote_version: status.game_version,
    update_available: false,
  } satisfies UpdateCheck);

  return {state, listeners, emitEvent};
}

/**
 * Init the composable without emitting an update-check event.
 * Useful for testing the state before the backend responds.
 * @returns the composable instance
 */
async function initWithoutUpdateCheck() {
  const {emitEvent} = captureListeners();

  const state = useGameState();
  state.init();

  // Simulate the monitor's initial game-status event
  emitEvent('game-status', makeGameStatus());

  await vi.waitFor(() => {
    expect(state.loading.value).toBe(false);
  });

  return {state};
}

/**
 * Init the composable with an available update (installed: 100, remote: 200).
 * Waits until the remote version is settled.
 * @param statusOverrides - additional fields to override on the default GameStatus
 * @returns the composable instance
 */
async function initWithUpdateAvailable(statusOverrides: Partial<GameStatus> = {}) {
  const {listeners, emitEvent} = captureListeners();

  const state = useGameState();
  state.init();

  // Simulate the monitor's initial game-status event
  emitEvent('game-status', makeGameStatus({game_version: 100, ...statusOverrides}));

  await vi.waitFor(() => {
    expect(state.loading.value).toBe(false);
  });

  // Simulate the backend's async update check event
  emitEvent('update-check', {
    installed_version: 100,
    remote_version: 200,
    update_available: true,
  } satisfies UpdateCheck);

  return {state, listeners, emitEvent};
}

// ---- Tests --------------------------------------------------------------------------

describe('useGameState', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetVersion.mockResolvedValue('1.0.0');
    mockInvoke.mockResolvedValue(undefined);
    mockListen.mockResolvedValue(vi.fn());
  });

  // ---- Computed Guards --------------------------------------------------------------

  describe('computed guards', () => {
    describe('updateAvailable', () => {
      it('returns false before status is loaded', () => {
        const state = useGameState();
        expect(state.loading.value).toBe(true);
        expect(state.updateAvailable.value).toBe(false);
      });

      it('returns false when game_version is null', async () => {
        const {state} = await initWithStatus({game_version: null});
        expect(state.updateAvailable.value).toBe(false);
      });

      it('returns false when remoteVersion is null', async () => {
        const {state} = await initWithoutUpdateCheck();

        // No update-check event emitted yet, so the remoteVersion is still null
        expect(state.remoteVersion.value).toBeNull();
        expect(state.updateAvailable.value).toBe(false);
      });

      it('returns false when remote version equals installed version', async () => {
        const {state} = await initWithStatus({game_version: 100});
        expect(state.updateAvailable.value).toBe(false);
      });

      it('returns true when remote version exceeds installed version', async () => {
        const {state} = await initWithUpdateAvailable();
        expect(state.updateAvailable.value).toBe(true);
      });
    });

    describe('canLaunch', () => {
      it('returns true when all conditions are met', async () => {
        const {state} = await initWithStatus();
        expect(state.canLaunch.value).toBeTruthy();
      });

      it('returns false when mod is not deployed', async () => {
        const {state} = await initWithStatus({mod_deployed: false});
        expect(state.canLaunch.value).toBeFalsy();
      });

      it('returns false when game is running', async () => {
        const {state} = await initWithStatus({game_running: true});
        expect(state.canLaunch.value).toBeFalsy();
      });

      it('returns false when launcher is running', async () => {
        const {state} = await initWithStatus({launcher_running: true});
        expect(state.canLaunch.value).toBeFalsy();
      });

      it('returns false when update is available', async () => {
        const {state} = await initWithUpdateAvailable();
        expect(state.canLaunch.value).toBeFalsy();
      });
    });

    describe('versionCheckClass', () => {
      it('returns "warn" when update is available', async () => {
        const {state} = await initWithUpdateAvailable();
        expect(state.versionCheckClass.value).toBe('warn');
      });

      it('returns "neutral" when no update-check event received yet', async () => {
        const {state} = await initWithoutUpdateCheck();

        // No update-check event emitted, remoteVersion is still null
        expect(state.versionCheckClass.value).toBe('neutral');
      });

      it('returns "ok" when remote version is present and no update', async () => {
        const {state} = await initWithStatus({game_version: 100});
        expect(state.versionCheckClass.value).toBe('ok');
      });

      it('returns "neutral" when no remote version yet', () => {
        const {versionCheckClass} = useGameState();
        expect(versionCheckClass.value).toBe('neutral');
      });
    });

    describe('canInstallMod', () => {
      it('returns true when mod_installable, nothing running, no update', async () => {
        const {state} = await initWithStatus();
        expect(state.canInstallMod.value).toBeTruthy();
      });

      it('returns false when mod_installable is false', async () => {
        const {state} = await initWithStatus({mod_installable: false});
        expect(state.canInstallMod.value).toBeFalsy();
      });

      it('returns false when game is running', async () => {
        const {state} = await initWithStatus({game_running: true});
        expect(state.canInstallMod.value).toBeFalsy();
      });

      it('returns false when launcher is running', async () => {
        const {state} = await initWithStatus({launcher_running: true});
        expect(state.canInstallMod.value).toBeFalsy();
      });
    });

    describe('canRemoveMod', () => {
      it('returns true when mod_removable is true, nothing running', async () => {
        const {state} = await initWithStatus({mod_removable: true});
        expect(state.canRemoveMod.value).toBeTruthy();
      });

      it('returns false when mod_removable is false', async () => {
        const {state} = await initWithStatus({mod_removable: false});
        expect(state.canRemoveMod.value).toBeFalsy();
      });

      it('returns false when game is running', async () => {
        const {state} = await initWithStatus({mod_removable: true, game_running: true});
        expect(state.canRemoveMod.value).toBeFalsy();
      });

      it('returns false when launcher is running', async () => {
        const {state} = await initWithStatus({mod_removable: true, launcher_running: true});
        expect(state.canRemoveMod.value).toBeFalsy();
      });
    });

    describe('canLaunchUpdater', () => {
      it('returns true when update available and launcher not running', async () => {
        const {state} = await initWithUpdateAvailable();
        expect(state.canLaunchUpdater.value).toBe(true);
      });

      it('returns false when no update available', async () => {
        const {state} = await initWithStatus();
        expect(state.canLaunchUpdater.value).toBe(false);
      });

      it('returns false when launcher is already running', async () => {
        const {state} = await initWithUpdateAvailable({launcher_running: true});
        expect(state.canLaunchUpdater.value).toBe(false);
      });
    });
  });

  // ---- Actions ----------------------------------------------------------------------

  describe('actions', () => {
    describe('installMod', () => {
      it('clears pending after command resolves', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.installMod();
        expect(state.actionPending.value).toBe(true);

        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        captureListeners();
        mockInvoke.mockRejectedValue(new Error('permission denied'));

        const state = useGameState();
        state.init();
        state.installMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('permission denied');
      });

      it('manages actionPending lifecycle', () => {
        captureListeners();
        mockInvoke.mockReturnValue(new Promise(() => {})); // never resolves

        const state = useGameState();
        state.init();
        expect(state.actionPending.value).toBe(false);
        state.installMod();
        expect(state.actionPending.value).toBe(true);
      });

      it('clears previous actionError', async () => {
        captureListeners();

        const state = useGameState();
        state.init();

        // The first call fails
        mockInvoke.mockRejectedValueOnce(new Error('first error'));
        state.installMod();
        await vi.waitFor(() => {
          expect(state.actionError.value).toContain('first error');
        });

        // The second call succeeds, error is cleared on start
        mockInvoke.mockResolvedValue(undefined);
        state.installMod();
        expect(state.actionError.value).toBeNull();

        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });
      });
    });

    describe('removeMod', () => {
      it('clears pending after command resolves', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.removeMod();
        expect(state.actionPending.value).toBe(true);

        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        captureListeners();
        mockInvoke.mockRejectedValue(new Error('file in use'));

        const state = useGameState();
        state.init();
        state.removeMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('file in use');
      });

      it('manages actionPending lifecycle', () => {
        captureListeners();
        mockInvoke.mockReturnValue(new Promise(() => {})); // never resolves

        const state = useGameState();
        state.init();
        expect(state.actionPending.value).toBe(false);
        state.removeMod();
        expect(state.actionPending.value).toBe(true);
      });

      it('clears previous actionError', async () => {
        captureListeners();

        const state = useGameState();
        state.init();

        mockInvoke.mockRejectedValueOnce(new Error('first error'));
        state.removeMod();
        await vi.waitFor(() => {
          expect(state.actionError.value).toContain('first error');
        });

        mockInvoke.mockResolvedValue(undefined);
        state.removeMod();
        expect(state.actionError.value).toBeNull();

        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });
      });
    });

    describe('openUpdater', () => {
      it('sets launcherRunning and updaterStartedByUs on success', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.openUpdater();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.launcherRunning.value).toBe(true);
        expect(state.updaterStartedByUs.value).toBe(true);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        captureListeners();
        mockInvoke.mockRejectedValue(new Error('launcher not found'));

        const state = useGameState();
        state.init();
        state.openUpdater();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('launcher not found');
      });
    });

    describe('launchGame', () => {
      it('sets gameRunning on success', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.launchGame();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.gameRunning.value).toBe(true);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        captureListeners();
        mockInvoke.mockRejectedValue(new Error('launch failed'));

        const state = useGameState();
        state.init();
        state.launchGame();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('launch failed');
      });
    });
  });

  // ---- Event Listeners --------------------------------------------------------------

  describe('event listeners', () => {
    it('updates status and process flags on game-status event', async () => {
      const {state, emitEvent} = await initWithStatus();
      const newStatus = makeGameStatus({mod_deployed: false, game_running: true});

      emitEvent('game-status', newStatus);

      expect(state.status.value).toEqual(newStatus);
      expect(state.gameRunning.value).toBe(true);
    });

    it('resets updaterStartedByUs when launcher stops via game-status', async () => {
      const {state, emitEvent} = await initWithStatus();

      // Simulate openUpdater setting the flag
      mockInvoke.mockResolvedValueOnce(undefined);
      state.openUpdater();
      await vi.waitFor(() => {
        expect(state.updaterStartedByUs.value).toBe(true);
      });

      // Launcher stops (reported through game-status)
      emitEvent('game-status', makeGameStatus({launcher_running: false}));

      expect(state.updaterStartedByUs.value).toBe(false);
    });

    it('updates remoteVersion on update-check event', async () => {
      const {state, emitEvent} = await initWithStatus();

      emitEvent('update-check', {
        installed_version: 100,
        remote_version: 300,
        update_available: true,
      } satisfies UpdateCheck);

      expect(state.remoteVersion.value).toBe(300);
      expect(state.updateCheckFailed.value).toBe(false);
    });

    it('falls back to installed_version when remote_version is null', async () => {
      const {state, emitEvent} = await initWithStatus();

      emitEvent('update-check', {
        installed_version: 100,
        remote_version: null,
        update_available: false,
      } satisfies UpdateCheck);

      expect(state.remoteVersion.value).toBe(100);
    });

    it('sets updateCheckFailed on update-check-failed event', async () => {
      const {state, emitEvent} = await initWithStatus();

      emitEvent('update-check-failed', undefined);

      expect(state.updateCheckFailed.value).toBe(true);
    });

    it('resets updateCheckFailed when a successful update-check arrives', async () => {
      const {state, emitEvent} = await initWithStatus();

      // Simulate a failed check
      emitEvent('update-check-failed', undefined);
      expect(state.updateCheckFailed.value).toBe(true);

      // Successful check clears the failure
      emitEvent('update-check', {
        installed_version: 100,
        remote_version: 100,
        update_available: false,
      } satisfies UpdateCheck);

      expect(state.updateCheckFailed.value).toBe(false);
    });

    it('returns "neutral" versionCheckClass when update check failed', async () => {
      const {state, emitEvent} = await initWithStatus();

      emitEvent('update-check-failed', undefined);

      expect(state.versionCheckClass.value).toBe('neutral');
    });
  });

  // ---- getData (cached fetch) -------------------------------------------------------

  describe('getData', () => {
    it('fetches cached game status on init', async () => {
      const cached = makeGameStatus({mod_deployed: true, game_running: true});
      mockInvoke.mockResolvedValue(cached);
      captureListeners();

      const state = useGameState();
      state.init();

      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });

      expect(mockInvoke).toHaveBeenCalledWith('get_cached_game_status');
      expect(state.status.value).toEqual(cached);
      expect(state.gameRunning.value).toBe(true);
    });

    it('stays in loading state when cached status is null', async () => {
      mockInvoke.mockResolvedValue(null);
      captureListeners();

      const state = useGameState();
      state.init();

      // Flush the invoked promise
      await Promise.resolve();
      await Promise.resolve();

      // No cached data, loading stays true until game-status event
      expect(state.loading.value).toBe(true);
    });
  });

  // ---- Lifecycle --------------------------------------------------------------------

  describe('lifecycle', () => {
    it('logs error when getVersion fails', async () => {
      const mockLogger = mockGetLogger();
      mockGetVersion.mockRejectedValue(new Error('version unavailable'));
      captureListeners();

      const state = useGameState();
      state.init();

      await vi.waitFor(() => {
        expect(mockLogger.error).toHaveBeenCalled();
      });

      // App still works, the version stays empty
      expect(state.version.value).toBe('');
    });

    it('logs errors when listen calls fail', async () => {
      const mockLogger = mockGetLogger();
      mockListen.mockRejectedValue(new Error('listen failed'));

      const state = useGameState();
      state.init();

      await vi.waitFor(() => {
        // Three listen calls, three error logs
        expect(mockLogger.error).toHaveBeenCalledTimes(3);
      });
    });

    it('loads app version on init', async () => {
      mockGetVersion.mockResolvedValue('2.5.0');
      captureListeners();

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.version.value).toBe('2.5.0');
      });
    });

    it('registers three event listeners on init', () => {
      const {listeners} = captureListeners();

      const state = useGameState();
      state.init();

      expect(listeners.has('game-status')).toBe(true);
      expect(listeners.has('update-check')).toBe(true);
      expect(listeners.has('update-check-failed')).toBe(true);
      expect(listeners.size).toBe(3);
    });

    it('sets loading to false on first game-status event', () => {
      const {emitEvent} = captureListeners();

      const state = useGameState();
      expect(state.loading.value).toBe(true);

      state.init();
      expect(state.loading.value).toBe(true);

      emitEvent('game-status', makeGameStatus());
      expect(state.loading.value).toBe(false);
    });

    it('calls all unlisten functions on destroy', async () => {
      const unlistenFns = [vi.fn(), vi.fn(), vi.fn()];
      let callIndex = 0;
      mockListen.mockImplementation((_name: string, _cb: unknown) => {
        return Promise.resolve(unlistenFns[callIndex++]);
      });

      const state = useGameState();
      state.init();

      // Flush microtasks so .then() handlers store the unlisten functions
      await Promise.resolve();

      state.destroy();

      for (const fn of unlistenFns) {
        expect(fn).toHaveBeenCalledOnce();
      }
    });

    it('does not throw when destroy is called without init', () => {
      const state = useGameState();
      expect(() => state.destroy()).not.toThrow();
    });
  });
});
