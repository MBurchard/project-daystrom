import {invoke} from '@tauri-apps/api/core';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {
  acknowledgeSafetyNotice,
  getSafetyNoticeContext,
  isSafetyNoticeRequired,
} from '../safetyNotice';

vi.mock('@tauri-apps/api/core', () => ({invoke: vi.fn()}));

const mockInvoke = vi.mocked(invoke);

describe('safety notice commands', () => {
  beforeEach(() => mockInvoke.mockReset());

  it('loads the backend-owned requirement', async () => {
    mockInvoke.mockResolvedValue(true);

    await expect(isSafetyNoticeRequired()).resolves.toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith('is_safety_notice_required');
  });

  it('loads platform-specific removal paths', async () => {
    const context = {
      platform: 'windows' as const,
      cleanupPaths: ['C:\\Users\\Test\\AppData\\Roaming\\mbur.project-daystrom'],
      modLibraryPath: 'C:\\Games\\STFC\\version.dll',
    };
    mockInvoke.mockResolvedValue(context);

    await expect(getSafetyNoticeContext()).resolves.toEqual(context);
    expect(mockInvoke).toHaveBeenCalledWith('get_safety_notice_context');
  });

  it('acknowledges the current revision', async () => {
    mockInvoke.mockResolvedValue(undefined);

    await acknowledgeSafetyNotice();
    expect(mockInvoke).toHaveBeenCalledWith('acknowledge_safety_notice');
  });
});
