import {invoke} from '@tauri-apps/api/core';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {changeUiZoom} from '../zoom';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

const mockInvoke = vi.mocked(invoke);

describe('zoom commands', () => {
  beforeEach(() => vi.clearAllMocks());

  it('forwards zoom intent and returns backend-owned state', async () => {
    mockInvoke.mockResolvedValue({factor: 1.1});

    await expect(changeUiZoom('increase')).resolves.toEqual({factor: 1.1});
    expect(mockInvoke).toHaveBeenCalledWith('change_ui_zoom', {action: 'increase'});
  });
});
