import type {GameSettings} from '@generated/GameSettings';

import {beforeEach, describe, expect, it, vi} from 'vitest';
import {useSettings} from '../useSettings';

const mockGetGameSettings = vi.hoisted(() => vi.fn());
const mockSetGameSettings = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const mockLog = vi.hoisted(() => ({debug: vi.fn(), error: vi.fn()}));

vi.mock('@app/commands/settings', () => ({
  getGameSettings: mockGetGameSettings,
  setGameSettings: mockSetGameSettings,
}));

vi.mock('@app/log', () => ({
  getLogger: vi.fn().mockReturnValue(mockLog),
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

  it('loads persisted settings and logs the snapshot', async () => {
    const loaded = makeSettings();
    loaded.ui.scale = 140;
    mockGetGameSettings.mockResolvedValue(loaded);
    const {settings, init} = useSettings();

    init();

    await vi.waitFor(() => expect(settings.value).toEqual(loaded));
    expect(mockLog.debug).toHaveBeenCalledWith(`Loaded game settings: ${JSON.stringify(loaded)}`);
  });

  it('logs settings load and save failures', async () => {
    mockGetGameSettings.mockRejectedValue(new Error('load failed'));
    mockSetGameSettings.mockRejectedValue(new Error('save failed'));
    const {init, update} = useSettings();

    init();
    update(value => value.ui.scale = 110);

    await vi.waitFor(() => expect(mockLog.error).toHaveBeenCalledTimes(2));
  });

  it('stores disabled shortcuts through the shared mutation API', () => {
    const {settings, setShortcut} = useSettings();

    setShortcut('trigger_main_action', '');

    expect(settings.value.shortcuts).toEqual({trigger_main_action: ''});
    expect(mockSetGameSettings).toHaveBeenCalledWith(settings.value);
  });

  it('creates a missing shortcut map before assigning a shortcut', () => {
    const {settings, setShortcut} = useSettings();
    settings.value.shortcuts = undefined as unknown as GameSettings['shortcuts'];

    setShortcut('trigger_main_action', 'Space');

    expect(settings.value.shortcuts).toEqual({trigger_main_action: 'Space'});
  });

  it('maintains a sorted set of disabled banner types', () => {
    const {settings, setBannerTypeEnabled} = useSettings();
    settings.value.banners.disabled_types = ['Victory'];

    setBannerTypeEnabled('Defeat', false);
    setBannerTypeEnabled('Victory', true);

    expect(settings.value.banners.disabled_types).toEqual(['Defeat']);
    expect(mockSetGameSettings).toHaveBeenCalledTimes(2);
  });

  it('creates the disabled banner set when suppressing the first type', () => {
    const {settings, setBannerTypeEnabled} = useSettings();

    setBannerTypeEnabled('Victory', false);

    expect(settings.value.banners.disabled_types).toEqual(['Victory']);
  });
});
