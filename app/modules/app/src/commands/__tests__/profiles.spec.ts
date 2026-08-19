import {invoke} from '@tauri-apps/api/core';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {deleteLocalProfile} from '../profiles';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

const mockInvoke = vi.mocked(invoke);

describe('profile commands', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it('deletes a backend-owned local profile stem', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await expect(deleteLocalProfile('1_TestAccount')).resolves.toBeUndefined();

    expect(mockInvoke).toHaveBeenCalledWith('delete_local_profile', {stem: '1_TestAccount'});
  });
});
