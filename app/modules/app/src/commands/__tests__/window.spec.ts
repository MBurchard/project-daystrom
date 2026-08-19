import {invoke} from '@tauri-apps/api/core';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {closeMainWindow} from '../window';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

const mockInvoke = vi.mocked(invoke);

describe('window commands', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it('delegates main-window close policy to the backend', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await expect(closeMainWindow()).resolves.toBeUndefined();

    expect(mockInvoke).toHaveBeenCalledWith('request_main_window_close');
  });
});
