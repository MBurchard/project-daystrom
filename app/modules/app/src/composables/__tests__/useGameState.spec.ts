import type {GameStatus} from '@generated/GameStatus';

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
    remote_version: 100,
    update_check_failed: false,
    game_started_by_us: false,
    launcher_started_by_us: false,
    update_available: false,
    can_launch: true,
    can_install_mod: true,
    can_remove_mod: false,
    can_launch_updater: false,
    should_block_quit: false,
    version_check_class: 'ok',
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

  // ---- Status passthrough -----------------------------------------------------------

  describe('status passthrough', () => {
    it('exposes backend-computed fields directly', async () => {
      const {state} = await initWithStatus({
        update_available: true,
        can_launch: false,
        can_install_mod: false,
        can_launch_updater: true,
        version_check_class: 'warn',
      });

      expect(state.status.value.update_available).toBe(true);
      expect(state.status.value.can_launch).toBe(false);
      expect(state.status.value.can_install_mod).toBe(false);
      expect(state.status.value.can_launch_updater).toBe(true);
      expect(state.status.value.version_check_class).toBe('warn');
    });

    it('reflects updated status on new game-status events', async () => {
      const {state, emitEvent} = await initWithStatus();
      const newStatus = makeGameStatus({
        mod_deployed: false,
        game_running: true,
        can_launch: false,
      });

      emitEvent('game-status', newStatus);

      expect(state.status.value).toEqual(newStatus);
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
      it('clears pending after command resolves', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.openUpdater();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

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
      it('clears pending after command resolves', async () => {
        captureListeners();
        mockInvoke.mockResolvedValue(undefined);

        const state = useGameState();
        state.init();
        state.launchGame();
        await vi.waitFor(() => {
          expect(state.actionPending.value).toBe(false);
        });

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
    it('updates status on game-status event', async () => {
      const {state, emitEvent} = await initWithStatus();
      const newStatus = makeGameStatus({mod_deployed: false, game_running: true});

      emitEvent('game-status', newStatus);

      expect(state.status.value).toEqual(newStatus);
    });

    it('registers the listener before fetching the cached status', async () => {
      let resolveListen!: (unlisten: () => void) => void;
      mockListen.mockReturnValue(new Promise((resolve) => {
        resolveListen = resolve;
      }));

      const state = useGameState();
      state.init();

      expect(mockListen).toHaveBeenCalledWith('game-status', expect.any(Function));
      expect(mockInvoke).not.toHaveBeenCalledWith('get_cached_game_status');

      resolveListen(vi.fn());
      await vi.waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith('get_cached_game_status');
      });
    });

    it('does not overwrite an event received while the cached status is loading', async () => {
      const cached = makeGameStatus({remote_version: null, version_check_class: 'neutral'});
      const event = makeGameStatus({remote_version: 185, version_check_class: 'ok'});
      let resolveSnapshot!: (status: GameStatus) => void;
      mockInvoke.mockImplementation((command: string) => command === 'get_cached_game_status' ?
          new Promise((resolve) => {
            resolveSnapshot = resolve;
          }) :
          Promise.resolve(null));
      const {emitEvent} = captureListeners();

      const state = useGameState();
      state.init();

      await vi.waitFor(() => {
        expect(mockInvoke).toHaveBeenCalledWith('get_cached_game_status');
      });
      emitEvent('game-status', event);
      resolveSnapshot(cached);

      await vi.waitFor(() => {
        expect(state.status.value).toEqual(event);
      });
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

    it('logs error when listen call fails', async () => {
      const mockLogger = mockGetLogger();
      mockListen.mockRejectedValue(new Error('listen failed'));

      const state = useGameState();
      state.init();

      await vi.waitFor(() => {
        expect(mockLogger.error).toHaveBeenCalledTimes(1);
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

    it('registers one event listener on init', () => {
      const {listeners} = captureListeners();

      const state = useGameState();
      state.init();

      expect(listeners.has('game-status')).toBe(true);
      expect(listeners.size).toBe(1);
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

    it('calls unlisten function on destroy', async () => {
      const unlistenFn = vi.fn();
      mockListen.mockImplementation((_name: string, _cb: unknown) => {
        return Promise.resolve(unlistenFn);
      });

      const state = useGameState();
      state.init();

      // Flush microtasks so .then() handlers store the unlisten function
      await Promise.resolve();

      state.destroy();

      expect(unlistenFn).toHaveBeenCalledOnce();
    });

    it('does not throw when destroy is called without init', () => {
      const state = useGameState();
      expect(() => state.destroy()).not.toThrow();
    });
  });
});
