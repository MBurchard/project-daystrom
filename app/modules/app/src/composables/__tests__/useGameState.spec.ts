import type {GameStatus} from '@generated/GameStatus';
import type {ProcessStatus} from '@generated/ProcessStatus';
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
    install_dir: '/Applications/STFC.app',
    executable: '/Applications/STFC.app/Contents/MacOS/stfc',
    game_version: 100,
    entitlements_ok: true,
    granted_entitlements: [],
    missing_entitlements: [],
    mod_available: true,
    mod_deployed: true,
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
 * Init the composable with a prepared game status.
 * Awaits all microtasks so the state is settled.
 * @param statusOverrides - fields to override on the default GameStatus
 * @returns the composable instance
 */
async function initWithStatus(statusOverrides: Partial<GameStatus> = {}) {
  const status = makeGameStatus(statusOverrides);
  const {listeners, emitEvent} = captureListeners();

  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_game_status') {
      return Promise.resolve(status);
    }
    if (cmd === 'check_for_update') {
      return Promise.resolve({
        installed_version: status.game_version ?? 100,
        remote_version: status.game_version,
        update_available: false,
      } satisfies UpdateCheck);
    }
    return Promise.resolve(undefined);
  });

  const state = useGameState();
  state.init();
  await vi.waitFor(() => {
    expect(state.loading.value).toBe(false);
  });

  return {state, listeners, emitEvent};
}

/**
 * Init the composable with an available update (installed: 100, remote: 200).
 * Waits until the remote version is settled.
 * @param statusOverrides - additional fields to override on the default GameStatus
 * @returns the composable instance
 */
async function initWithUpdateAvailable(statusOverrides: Partial<GameStatus> = {}) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_game_status') {
      return Promise.resolve(makeGameStatus({game_version: 100, ...statusOverrides}));
    }
    if (cmd === 'check_for_update') {
      return Promise.resolve({
        installed_version: 100,
        remote_version: 200,
        update_available: true,
      } satisfies UpdateCheck);
    }
    return Promise.resolve(undefined);
  });
  captureListeners();

  const state = useGameState();
  state.init();
  await vi.waitFor(() => {
    expect(state.remoteVersion.value).toBe(200);
  });

  return {state};
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
        await initWithStatus();
        // checkForUpdate hasn't resolved with a different version
        // Let's override the mock to not call check_for_update
        mockInvoke.mockImplementation((cmd: string) => {
          if (cmd === 'get_game_status') {
            return Promise.resolve(makeGameStatus());
          }
          if (cmd === 'check_for_update') {
            return new Promise(() => {}); // never resolves
          }
          return Promise.resolve(undefined);
        });
        const state2 = useGameState();
        state2.init();
        await vi.waitFor(() => {
          expect(state2.loading.value).toBe(false);
        });
        // The remoteVersion is still null because check_for_update never resolved,
        // but initWithStatus resolves it. Use a fresh instance:
        expect(state2.updateAvailable.value).toBe(false);
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

      it('returns false when not installed', async () => {
        const {state} = await initWithStatus({installed: false});
        expect(state.canLaunch.value).toBeFalsy();
      });

      it('returns false when mod is not deployed', async () => {
        const {state} = await initWithStatus({mod_deployed: false});
        expect(state.canLaunch.value).toBeFalsy();
      });

      it('returns false when mod is not available', async () => {
        const {state} = await initWithStatus({mod_available: false});
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

      it('returns "neutral" when update check failed', async () => {
        mockInvoke.mockImplementation((cmd: string) => {
          if (cmd === 'get_game_status') {
            return Promise.resolve(makeGameStatus());
          }
          if (cmd === 'check_for_update') {
            return Promise.reject(new Error('network error'));
          }
          return Promise.resolve(undefined);
        });
        captureListeners();

        const state = useGameState();
        state.init();
        await vi.waitFor(() => {
          expect(state.updateCheckFailed.value).toBe(true);
        });

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
      it('returns true when installed, nothing running, no update', async () => {
        const {state} = await initWithStatus();
        expect(state.canInstallMod.value).toBeTruthy();
      });

      it('returns false when not installed', async () => {
        const {state} = await initWithStatus({installed: false});
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

      it('returns false when mod is not available', async () => {
        const {state} = await initWithStatus({mod_available: false});
        expect(state.canInstallMod.value).toBeFalsy();
      });
    });

    describe('canRemoveMod', () => {
      it('returns true when installed, mod deployed, nothing running', async () => {
        const {state} = await initWithStatus({mod_deployed: true});
        expect(state.canRemoveMod.value).toBeTruthy();
      });

      it('returns false when not installed', async () => {
        const {state} = await initWithStatus({installed: false});
        expect(state.canRemoveMod.value).toBeFalsy();
      });

      it('returns false when mod is not deployed', async () => {
        const {state} = await initWithStatus({mod_deployed: false});
        expect(state.canRemoveMod.value).toBeFalsy();
      });

      it('returns false when game is running', async () => {
        const {state} = await initWithStatus({mod_deployed: true, game_running: true});
        expect(state.canRemoveMod.value).toBeFalsy();
      });

      it('returns false when launcher is running', async () => {
        const {state} = await initWithStatus({mod_deployed: true, launcher_running: true});
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
    describe('checkForUpdate', () => {
      it('sets remoteVersion on success', async () => {
        mockInvoke.mockResolvedValue({
          installed_version: 100,
          remote_version: 200,
          update_available: true,
        } satisfies UpdateCheck);

        const state = useGameState();
        state.checkForUpdate();
        await vi.waitFor(() => {
          expect(state.remoteVersion.value).toBe(200);
        });

        expect(state.updateCheckFailed.value).toBe(false);
      });

      it('falls back to installed_version when remote_version is null', async () => {
        mockInvoke.mockResolvedValue({
          installed_version: 100,
          remote_version: null,
          update_available: false,
        } satisfies UpdateCheck);

        const state = useGameState();
        state.checkForUpdate();
        await vi.waitFor(() => {
          expect(state.remoteVersion.value).toBe(100);
        });
      });

      it('sets updateCheckFailed on error', async () => {
        mockInvoke.mockRejectedValue(new Error('network error'));

        const state = useGameState();
        state.checkForUpdate();
        await vi.waitFor(() => {
          expect(state.updateCheckFailed.value).toBe(true);
        });
      });
    });

    describe('installMod', () => {
      it('updates status on success', async () => {
        const newStatus = makeGameStatus({entitlements_ok: true});
        mockInvoke.mockResolvedValue(newStatus);

        const state = useGameState();
        state.installMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.status.value).toEqual(newStatus);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        mockInvoke.mockRejectedValue(new Error('permission denied'));

        const state = useGameState();
        state.installMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('permission denied');
      });

      it('manages actionPending lifecycle', () => {
        mockInvoke.mockReturnValue(new Promise(() => {})); // never resolves

        const state = useGameState();
        expect(state.actionPending.value).toBe(false);
        state.installMod();
        expect(state.actionPending.value).toBe(true);
      });

      it('clears previous actionError', async () => {
        // The first call fails
        mockInvoke.mockRejectedValueOnce(new Error('first error'));
        const state = useGameState();
        state.installMod();
        await vi.waitFor(() => {
          expect(state.actionError.value).toContain('first error');
        });

        // The second call succeeds
        mockInvoke.mockResolvedValue(makeGameStatus());
        state.installMod();
        expect(state.actionError.value).toBeNull();
      });
    });

    describe('removeMod', () => {
      it('updates status on success', async () => {
        const newStatus = makeGameStatus({mod_deployed: false});
        mockInvoke.mockResolvedValue(newStatus);

        const state = useGameState();
        state.removeMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.status.value).toEqual(newStatus);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        mockInvoke.mockRejectedValue(new Error('file in use'));

        const state = useGameState();
        state.removeMod();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('file in use');
      });

      it('manages actionPending lifecycle', () => {
        mockInvoke.mockReturnValue(new Promise(() => {})); // never resolves

        const state = useGameState();
        expect(state.actionPending.value).toBe(false);
        state.removeMod();
        expect(state.actionPending.value).toBe(true);
      });

      it('clears previous actionError', async () => {
        mockInvoke.mockRejectedValueOnce(new Error('first error'));
        const state = useGameState();
        state.removeMod();
        await vi.waitFor(() => {
          expect(state.actionError.value).toContain('first error');
        });

        mockInvoke.mockResolvedValue(makeGameStatus({mod_deployed: false}));
        state.removeMod();
        expect(state.actionError.value).toBeNull();
      });
    });

    describe('openUpdater', () => {
      it('sets launcherRunning and updaterStartedByUs on success', async () => {
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.openUpdater();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.launcherRunning.value).toBe(true);
        expect(state.updaterStartedByUs.value).toBe(true);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        mockInvoke.mockRejectedValue(new Error('launcher not found'));

        const state = useGameState();
        state.openUpdater();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.actionError.value).toContain('launcher not found');
      });
    });

    describe('launchGame', () => {
      it('sets gameRunning on success', async () => {
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.launchGame();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

        expect(state.gameRunning.value).toBe(true);
        expect(state.actionError.value).toBeNull();
      });

      it('sets actionError on failure', async () => {
        mockInvoke.mockRejectedValue(new Error('launch failed'));

        const state = useGameState();
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
    it('updates launcherRunning and gameRunning on process-status event', async () => {
      const {state, emitEvent} = await initWithStatus();

      emitEvent('process-status', {
        launcher_running: true,
        game_running: true,
      } satisfies ProcessStatus);

      expect(state.launcherRunning.value).toBe(true);
      expect(state.gameRunning.value).toBe(true);
    });

    it('resets updaterStartedByUs when launcher stops', async () => {
      const {state, emitEvent} = await initWithStatus();

      // First: launcher starts (simulate openUpdater)
      emitEvent('process-status', {
        launcher_running: true,
        game_running: false,
      } satisfies ProcessStatus);

      // Manually set updaterStartedByUs (normally done by openUpdater action)
      // We test this via the event path: when the launcher stops, updaterStartedByUs resets.
      mockInvoke.mockResolvedValueOnce(undefined);
      state.openUpdater();
      await vi.waitFor(() => {
        expect(state.updaterStartedByUs.value).toBe(true);
      });

      // Launcher stops
      emitEvent('process-status', {
        launcher_running: false,
        game_running: false,
      } satisfies ProcessStatus);

      expect(state.updaterStartedByUs.value).toBe(false);
    });

    it('updates status on game-status event', async () => {
      const {state, emitEvent} = await initWithStatus();
      const newStatus = makeGameStatus({entitlements_ok: false, game_running: true});

      emitEvent('game-status', newStatus);

      expect(state.status.value).toEqual(newStatus);
      expect(state.gameRunning.value).toBe(true);
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

    it('ignores null remote_version in update-check event', async () => {
      const {state, emitEvent} = await initWithStatus();
      const previousRemote = state.remoteVersion.value;

      emitEvent('update-check', {
        installed_version: 100,
        remote_version: null,
        update_available: false,
      } satisfies UpdateCheck);

      expect(state.remoteVersion.value).toBe(previousRemote);
    });
  });

  // ---- Lifecycle --------------------------------------------------------------------

  describe('lifecycle', () => {
    it('loads app version on init', async () => {
      mockGetVersion.mockResolvedValue('2.5.0');
      captureListeners();
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_game_status') {
          return Promise.resolve(makeGameStatus());
        }
        if (cmd === 'check_for_update') {
          return Promise.resolve({
            installed_version: 100,
            remote_version: 100,
            update_available: false,
          } satisfies UpdateCheck);
        }
        return Promise.resolve(undefined);
      });

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.version.value).toBe('2.5.0');
      });
    });

    it('registers three event listeners on init', async () => {
      const {listeners} = captureListeners();
      mockInvoke.mockResolvedValue(makeGameStatus({installed: false}));

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });

      expect(listeners.has('process-status')).toBe(true);
      expect(listeners.has('game-status')).toBe(true);
      expect(listeners.has('update-check')).toBe(true);
    });

    it('calls get_game_status on init', async () => {
      captureListeners();
      mockInvoke.mockResolvedValue(makeGameStatus({installed: false}));

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });

      expect(mockInvoke).toHaveBeenCalledWith('get_game_status');
    });

    it('calls check_for_update when game is installed', async () => {
      captureListeners();
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_game_status') {
          return Promise.resolve(makeGameStatus({installed: true}));
        }
        if (cmd === 'check_for_update') {
          return Promise.resolve({
            installed_version: 100,
            remote_version: 100,
            update_available: false,
          } satisfies UpdateCheck);
        }
        return Promise.resolve(undefined);
      });

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith('check_for_update');
      });
    });

    it('skips check_for_update when game is not installed', async () => {
      captureListeners();
      mockInvoke.mockResolvedValue(makeGameStatus({installed: false}));

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });

      expect(mockInvoke).not.toHaveBeenCalledWith('check_for_update');
    });

    it('sets error when get_game_status fails', async () => {
      captureListeners();
      mockInvoke.mockRejectedValue(new Error('backend unavailable'));

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.error.value).toContain('backend unavailable');
      });
    });

    it('calls all unlisten functions on destroy', async () => {
      const unlistenFns = [vi.fn(), vi.fn(), vi.fn()];
      let callIndex = 0;
      mockListen.mockImplementation((_name: string, _cb: unknown) => {
        return Promise.resolve(unlistenFns[callIndex++]);
      });
      mockInvoke.mockResolvedValue(makeGameStatus({installed: false}));

      const state = useGameState();
      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });

      state.destroy();

      for (const fn of unlistenFns) {
        expect(fn).toHaveBeenCalledOnce();
      }
    });

    it('sets loading to false after successful init', async () => {
      captureListeners();
      mockInvoke.mockResolvedValue(makeGameStatus({installed: false}));

      const state = useGameState();
      expect(state.loading.value).toBe(true);

      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });
    });

    it('sets loading to false after failed init', async () => {
      captureListeners();
      mockInvoke.mockRejectedValue(new Error('backend unavailable'));

      const state = useGameState();
      expect(state.loading.value).toBe(true);

      state.init();
      await vi.waitFor(() => {
        expect(state.loading.value).toBe(false);
      });
    });

    it('does not throw when destroy is called without init', () => {
      const state = useGameState();
      expect(() => state.destroy()).not.toThrow();
    });
  });
});
