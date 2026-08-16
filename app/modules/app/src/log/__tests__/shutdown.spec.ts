import {beforeEach, describe, expect, it, vi} from 'vitest';

const {mockCloseLogging, mockInvoke, mockListen} = vi.hoisted(() => ({
  mockCloseLogging: vi.fn().mockResolvedValue(undefined),
  mockInvoke: vi.fn().mockResolvedValue(undefined),
  mockListen: vi.fn().mockResolvedValue(vi.fn()),
}));

vi.mock('@mburchard/bit-log', () => ({
  closeLogging: mockCloseLogging,
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}));

describe('logging shutdown', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it('closes logging before completing the backend shutdown', async () => {
    let resolveClose!: () => void;
    const closePromise = new Promise<void>((resolve) => {
      resolveClose = resolve;
    });
    mockCloseLogging.mockReturnValueOnce(closePromise);
    const {registerLoggingShutdownHandler} = await import('../shutdown');

    await registerLoggingShutdownHandler();
    expect(mockListen).toHaveBeenCalledWith('shutdown-requested', expect.any(Function));

    const handler = mockListen.mock.calls[0]?.[1] as () => void;
    handler();
    expect(mockInvoke).not.toHaveBeenCalled();

    resolveClose();
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith('complete_shutdown');
    });
  });
});
