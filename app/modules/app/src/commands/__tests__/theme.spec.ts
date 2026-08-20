import {beforeEach, describe, expect, it, vi} from 'vitest';

const mockInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({invoke: mockInvoke}));

describe('theme commands', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('loads the backend-selected application theme', async () => {
    mockInvoke.mockResolvedValue('classic');
    const {getAppTheme} = await import('../theme');

    await expect(getAppTheme()).resolves.toBe('classic');
    expect(mockInvoke).toHaveBeenCalledWith('get_app_theme');
  });

  it('persists an explicit application theme selection', async () => {
    mockInvoke.mockResolvedValue(undefined);
    const {setAppTheme} = await import('../theme');

    await setAppTheme('omega');
    expect(mockInvoke).toHaveBeenCalledWith('set_app_theme', {theme: 'omega'});
  });
});
