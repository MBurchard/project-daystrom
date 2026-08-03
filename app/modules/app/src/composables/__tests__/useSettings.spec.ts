import type {GameSettings} from '@generated/GameSettings';

import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useSettings} from '../useSettings';

const mockGetGameSettings = vi.hoisted(() => vi.fn());
const mockSetGameSettings = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock('@app/commands/settings', () => ({
  getGameSettings: mockGetGameSettings,
  setGameSettings: mockSetGameSettings,
}));

vi.mock('@app/log', () => ({
  getLogger: vi.fn().mockReturnValue({
    debug: vi.fn(),
    error: vi.fn(),
  }),
}));

function makeSettings(): GameSettings {
  return {
    ui: {},
    banners: {},
    cargo_view: {},
    slider_limits: {},
    shortcuts: {},
  };
}

describe('useSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSetGameSettings.mockResolvedValue(undefined);
    useSettings().settings.value = makeSettings();
  });

  it('persists the complete settings after an update', () => {
    const {settings, update} = useSettings();

    update((value) => {
      value.ui.scale = 125;
    });

    expect(settings.value.ui.scale).toBe(125);
    expect(mockSetGameSettings).toHaveBeenCalledWith(settings.value);
  });

  it('stores disabled shortcuts through the shared mutation API', () => {
    const {settings, setShortcut} = useSettings();

    setShortcut('trigger_main_action', '');

    expect(settings.value.shortcuts).toEqual({trigger_main_action: ''});
    expect(mockSetGameSettings).toHaveBeenCalledWith(settings.value);
  });

  it('maintains a sorted set of disabled banner types', () => {
    const {settings, setBannerTypeEnabled} = useSettings();
    settings.value.banners.disabled_types = ['Victory'];

    setBannerTypeEnabled('Defeat', false);
    setBannerTypeEnabled('Victory', true);

    expect(settings.value.banners.disabled_types).toEqual(['Defeat']);
    expect(mockSetGameSettings).toHaveBeenCalledTimes(2);
  });
});
