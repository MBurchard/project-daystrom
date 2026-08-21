import type {ProfileState} from '@generated/ProfileState';

import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useProfileState} from '../useProfileState';

const mockInvoke = vi.hoisted(() => vi.fn());
const mockListen = vi.hoisted(() => vi.fn());
const mockGetLogger = vi.hoisted(() => vi.fn().mockReturnValue({
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

vi.mock('@app/log', () => ({
  getLogger: mockGetLogger,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}));

type ListenerCallback = (event: {payload: ProfileState}) => void;

function makeProfileState(name: string): ProfileState {
  return {
    profiles: [{
      name,
      server: 411,
      stem: `411_${name}`,
      primary: true,
    }],
    running_profiles: [],
    external_game_running: false,
    game_origin_pending: false,
    mod_connection_missing: false,
  };
}

function captureListener(): {emit: (state: ProfileState) => void} {
  let listener: ListenerCallback | undefined;
  mockListen.mockImplementation((_eventName: string, callback: ListenerCallback) => {
    listener = callback;
    return Promise.resolve(vi.fn());
  });

  return {
    emit(state: ProfileState) {
      if (!listener) {
        throw new Error('profile-status listener is not registered');
      }
      listener({payload: state});
    },
  };
}

describe('useProfileState', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(makeProfileState('Cached'));
    mockListen.mockResolvedValue(vi.fn());
  });

  it('registers the listener before fetching the cached profile state', async () => {
    let resolveListen!: (unlisten: () => void) => void;
    mockListen.mockReturnValue(new Promise((resolve) => {
      resolveListen = resolve;
    }));

    const state = useProfileState();
    state.init();

    expect(mockListen).toHaveBeenCalledWith('profile-status', expect.any(Function));
    expect(mockInvoke).not.toHaveBeenCalled();

    resolveListen(vi.fn());
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_profile_state');
    });
  });

  it('applies the cached state when no event arrives', async () => {
    const cached = makeProfileState('Cached');
    mockInvoke.mockResolvedValue(cached);
    captureListener();

    const state = useProfileState();
    state.init();

    await vi.waitFor(() => {
      expect(state.profiles.value).toEqual(cached);
    });
  });

  it('does not overwrite an event received while the cached state is loading', async () => {
    const cached = makeProfileState('Cached');
    const event = makeProfileState('Current');
    let resolveSnapshot!: (state: ProfileState) => void;
    mockInvoke.mockReturnValue(new Promise((resolve) => {
      resolveSnapshot = resolve;
    }));
    const {emit} = captureListener();

    const state = useProfileState();
    state.init();

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_profile_state');
    });
    emit(event);
    resolveSnapshot(cached);

    await vi.waitFor(() => {
      expect(state.profiles.value).toEqual(event);
    });
  });

  it('exposes profile, origin, and running state', async () => {
    const current = makeProfileState('Current');
    current.running_profiles = ['411_Current'];
    current.external_game_running = true;
    current.game_origin_pending = true;
    current.mod_connection_missing = true;
    mockInvoke.mockResolvedValue(current);
    const state = useProfileState();

    state.init();
    await vi.waitFor(() => expect(state.profiles.value).toEqual(current));

    expect(state.hasProfiles.value).toBe(true);
    expect(state.externalGameRunning.value).toBe(true);
    expect(state.gameOriginPending.value).toBe(true);
    expect(state.isProfileRunning('411_Current')).toBe(true);
    expect(state.isProfileRunning('411_Other')).toBe(false);
  });

  it('logs listener and snapshot failures', async () => {
    const logger = mockGetLogger();
    mockListen.mockRejectedValue(new Error('listen failed'));
    mockInvoke.mockRejectedValue(new Error('snapshot failed'));
    const state = useProfileState();

    state.init();

    await vi.waitFor(() => expect(logger.error).toHaveBeenCalledTimes(2));
  });

  it('applies later profile events', async () => {
    const current = makeProfileState('Current');
    const {emit} = captureListener();
    const state = useProfileState();
    state.init();
    await vi.waitFor(() => expect(mockInvoke).toHaveBeenCalled());

    emit(current);

    expect(state.profiles.value).toEqual(current);
  });

  it('unregisters its listener when destroyed', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);
    const state = useProfileState();
    state.init();
    await vi.waitFor(() => expect(mockInvoke).toHaveBeenCalled());

    state.destroy();
    state.destroy();

    expect(unlisten).toHaveBeenCalledOnce();
  });
});
