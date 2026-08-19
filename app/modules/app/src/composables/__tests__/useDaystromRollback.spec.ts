import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useDaystromRollback} from '../useDaystromRollback';

const mockInvoke = vi.hoisted(() => vi.fn());
const mockListen = vi.hoisted(() => vi.fn());
const mockLog = vi.hoisted(() => ({error: vi.fn()}));

vi.mock('@app/log', () => ({
  getLogger: vi.fn(() => mockLog),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}));

/**
 * Build a complete rollback status for composable tests.
 *
 * @param overrides - Status fields to replace.
 * @returns A complete backend status snapshot.
 */
function makeStatus(overrides: Partial<DaystromRollbackStatus> = {}): DaystromRollbackStatus {
  return {
    phase: 'unavailable',
    version: null,
    error: null,
    can_restore: false,
    mod_restore_pending: false,
    ...overrides,
  };
}

describe('useDaystromRollback', () => {
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
    const state = useDaystromRollback();

    state.init();

    expect(mockListen).toHaveBeenCalledWith('daystrom-rollback-status', expect.any(Function));
    expect(mockInvoke).not.toHaveBeenCalled();
    resolveListen(vi.fn());
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_daystrom_rollback_status');
    });
  });

  it('requests restoration through the narrow backend command', () => {
    const state = useDaystromRollback();

    state.restore();

    expect(mockInvoke).toHaveBeenCalledWith('restore_previous_daystrom_version');
  });

  it('logs rejected restoration requests', async () => {
    const error = new Error('restore failed');
    mockInvoke.mockRejectedValue(error);
    const state = useDaystromRollback();

    state.restore();

    await vi.waitFor(() => expect(mockLog.error).toHaveBeenCalledWith(
      'Failed to request Daystrom rollback:',
      error,
    ));
  });

  it('applies backend rollback events', async () => {
    let listener: ((event: {payload: DaystromRollbackStatus}) => void) | undefined;
    mockListen.mockImplementation((_eventName, callback) => {
      listener = callback;
      return Promise.resolve(vi.fn());
    });
    const state = useDaystromRollback();
    state.init();
    await vi.waitFor(() => expect(listener).toBeDefined());

    listener!({payload: makeStatus({phase: 'available', version: '0.9.0', can_restore: true})});

    expect(state.status.value.version).toBe('0.9.0');
    expect(state.status.value.can_restore).toBe(true);
  });

  it('applies the cached rollback snapshot', async () => {
    const available = makeStatus({phase: 'available', version: '0.9.1'});
    mockInvoke.mockResolvedValue(available);
    const state = useDaystromRollback();

    state.init();

    await vi.waitFor(() => expect(state.status.value).toEqual(available));
  });

  it('does not overwrite an event received while the cached snapshot is loading', async () => {
    let listener!: (event: {payload: DaystromRollbackStatus}) => void;
    let resolveSnapshot!: (status: DaystromRollbackStatus) => void;
    mockListen.mockImplementation((_name, callback) => {
      listener = callback;
      return Promise.resolve(vi.fn());
    });
    mockInvoke.mockReturnValue(new Promise(resolve => resolveSnapshot = resolve));
    const state = useDaystromRollback();
    state.init();
    await vi.waitFor(() => expect(mockInvoke).toHaveBeenCalled());

    listener({payload: makeStatus({phase: 'available', version: '0.9.1'})});
    resolveSnapshot(makeStatus());

    await vi.waitFor(() => expect(state.status.value.version).toBe('0.9.1'));
  });

  it('logs listener and cached snapshot failures', async () => {
    mockListen.mockRejectedValue(new Error('listen failed'));
    mockInvoke.mockRejectedValue(new Error('snapshot failed'));
    const state = useDaystromRollback();

    state.init();

    await vi.waitFor(() => expect(mockLog.error).toHaveBeenCalledTimes(2));
  });

  it('unregisters its listener when destroyed', async () => {
    const unlisten = vi.fn();
    mockListen.mockResolvedValue(unlisten);
    const state = useDaystromRollback();
    state.init();
    await vi.waitFor(() => expect(mockInvoke).toHaveBeenCalled());

    state.destroy();
    state.destroy();

    expect(unlisten).toHaveBeenCalledOnce();
  });
});
