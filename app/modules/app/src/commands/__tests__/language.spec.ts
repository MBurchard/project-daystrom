import {beforeEach, describe, expect, it, vi} from 'vitest';

const mockInvoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({invoke: mockInvoke}));

describe('language commands', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('supplies the raw system locale when resolving the language', async () => {
    mockInvoke.mockResolvedValue('de');
    const {getAppLanguage} = await import('../language');

    await expect(getAppLanguage('de-AT')).resolves.toBe('de');
    expect(mockInvoke).toHaveBeenCalledWith('get_app_language', {systemLocale: 'de-AT'});
  });

  it('persists an explicit language selection', async () => {
    mockInvoke.mockResolvedValue(undefined);
    const {setAppLanguage} = await import('../language');

    await setAppLanguage('en');
    expect(mockInvoke).toHaveBeenCalledWith('set_app_language', {language: 'en'});
  });
});
