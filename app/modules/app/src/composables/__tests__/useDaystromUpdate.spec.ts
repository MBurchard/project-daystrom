import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useDaystromUpdate} from '../useDaystromUpdate';

const mockInvoke = vi.hoisted(() => vi.fn());
const mockListen = vi.hoisted(() => vi.fn());
const mockLog = vi.hoisted(() => ({
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

vi.mock('@app/log', () => ({
  getLogger: vi.fn(() => mockLog),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}));

type ListenerCallback = (event: {payload: DaystromUpdateStatus}) => void;

/**
 * Build a complete update status for composable tests.
 *
 * @param overrides - Status fields to replace.
 * @returns A complete backend status snapshot.
 */
function makeStatus(overrides: Partial<DaystromUpdateStatus> = {}): DaystromUpdateStatus {
  return {
    phase: 'up_to_date',
    version: null,
    notes: null,
    download_progress: null,
    error: null,
    dismissed: false,
    can_install: false,
    ...overrides,
  };
}

/**
 * Capture the registered event callback and expose a deterministic emitter.
 *
 * @returns An emitter for update-status events.
 */
function captureListener(): {emit: (status: DaystromUpdateStatus) => void} {
  let listener: ListenerCallback | undefined;
  mockListen.mockImplementation((_eventName: string, callback: ListenerCallback) => {
    listener = callback;
    return Promise.resolve(vi.fn());
  });

  return {
    emit(status: DaystromUpdateStatus): void {
      if (!listener) {
        throw new Error('daystrom-update-status listener is not registered');
      }
      listener({payload: status});
    },
  };
}

describe('useDaystromUpdate', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(makeStatus());
    mockListen.mockResolvedValue(vi.fn());
  });

  it('registers the listener before requesting the cached snapshot', async () => {
    let resolveListen!: (unlisten: () => void) => void;
    mockListen.mockReturnValue(new Promise((resolve) => {
      resolveListen = resolve;
    }));

    const state = useDaystromUpdate();
    state.init();

    expect(mockListen).toHaveBeenCalledWith('daystrom-update-status', expect.any(Function));
    expect(mockInvoke).not.toHaveBeenCalled();

    resolveListen(vi.fn());
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_daystrom_update_status');
    });
  });

  it('does not overwrite an event received while the cached snapshot is loading', async () => {
    const cached = makeStatus();
    const available = makeStatus({phase: 'available', version: '0.10.0'});
    let resolveSnapshot!: (status: DaystromUpdateStatus) => void;
    mockInvoke.mockReturnValue(new Promise((resolve) => {
      resolveSnapshot = resolve;
    }));
    const {emit} = captureListener();

    const state = useDaystromUpdate();
    state.init();

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_daystrom_update_status');
    });
    emit(available);
    resolveSnapshot(cached);

    await vi.waitFor(() => {
      expect(state.status.value).toEqual(available);
    });
  });

  it('requests a manual check through the narrow backend command', () => {
    const state = useDaystromUpdate();

    state.check();

    expect(mockInvoke).toHaveBeenCalledWith('check_for_daystrom_update');
  });

  it('dismisses through the backend without mutating local state', () => {
    const state = useDaystromUpdate();

    state.dismiss();

    expect(mockInvoke).toHaveBeenCalledWith('dismiss_daystrom_update');
    expect(state.status.value.dismissed).toBe(false);
  });

  it('requests installation through the narrow backend command', () => {
    const state = useDaystromUpdate();

    state.install();

    expect(mockInvoke).toHaveBeenCalledWith('install_daystrom_update');
  });

  it('unregisters the event listener on destroy', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);
    const state = useDaystromUpdate();
    state.init();

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_daystrom_update_status');
    });
    state.destroy();

    expect(unlisten).toHaveBeenCalledOnce();
  });
});
