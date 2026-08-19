import type {GameSettings} from '@generated/GameSettings';
import type {VueWrapper} from '@vue/test-utils';
import {mount} from '@vue/test-utils';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {defineComponent, h, ref} from 'vue';
import {
  GAME_DEFAULT_SLIDER_MAX,
  MAX_CONFIGURED_SLIDER_LIMIT,
  normalizeSliderLimit,
  STANDARD_RECRUIT_MAX,
  useSettingsView,
} from '../useSettingsView';

const mocks = vi.hoisted(() => ({
  setBannerTypeEnabled: vi.fn(),
  setShortcut: vi.fn(),
  update: vi.fn(),
  useSettings: vi.fn(),
}));

vi.mock('../useSettings', () => ({useSettings: mocks.useSettings}));

/** Build complete settings with optional nested overrides. */
function makeSettings(): GameSettings {
  return {
    ui: {},
    banners: {},
    cargo_view: {},
    slider_limits: {},
    shortcuts: {},
  };
}

/** Create a minimal input event carrying string and checkbox values. */
function inputEvent(value: string, checked = false): Event {
  return {target: {checked, value}} as unknown as Event;
}

/** Mount the composable inside a Vue lifecycle owner. */
function mountSettingsView(): {state: ReturnType<typeof useSettingsView>; wrapper: VueWrapper} {
  let state!: ReturnType<typeof useSettingsView>;
  const wrapper = mount(defineComponent({
    setup() {
      state = useSettingsView();
      return () => h('div');
    },
  }));
  return {state, wrapper};
}

describe('normalizeSliderLimit', () => {
  it('caps Standard Recruit at its supported maximum', () => {
    expect(normalizeSliderLimit('500', STANDARD_RECRUIT_MAX)).toBe(150);
  });

  it('caps alliance donations at the largest supported setting value', () => {
    expect(normalizeSliderLimit('4294967296', MAX_CONFIGURED_SLIDER_LIMIT))
      .toBe(MAX_CONFIGURED_SLIDER_LIMIT);
  });

  it('truncates fractional values', () => {
    expect(normalizeSliderLimit('87.9', STANDARD_RECRUIT_MAX)).toBe(87);
  });

  it.each(['50', '20', 'not-a-number'])('maps %s to the unchanged game default', (value) => {
    expect(normalizeSliderLimit(value, STANDARD_RECRUIT_MAX)).toBeNull();
  });
});

describe('useSettingsView', () => {
  let settings = ref(makeSettings());

  beforeEach(() => {
    vi.clearAllMocks();
    settings = ref(makeSettings());
    mocks.update.mockImplementation((updater: (value: GameSettings) => void) => updater(settings.value));
    mocks.setShortcut.mockImplementation((key: string, code: string) => {
      settings.value.shortcuts![key] = code;
    });
    mocks.useSettings.mockReturnValue({
      settings,
      update: mocks.update,
      setShortcut: mocks.setShortcut,
      setBannerTypeEnabled: mocks.setBannerTypeEnabled,
    });
  });

  it('exposes game defaults and configured values', () => {
    const first = mountSettingsView();

    expect(first.state.effectiveScale.value).toBe(100);
    expect(first.state.effectiveSystemZoom.value).toBe(1000);
    expect(first.state.effectiveShipNamesVisible.value).toBe(1800);
    expect(first.state.effectiveStandardRecruitMax.value).toBe(GAME_DEFAULT_SLIDER_MAX);
    expect(first.state.effectiveAllianceDonationMax.value).toBe(GAME_DEFAULT_SLIDER_MAX);
    expect(first.state.effectiveTransporterPatternMax.value).toBe(GAME_DEFAULT_SLIDER_MAX);
    expect(first.state.allBannersDisabled.value).toBe(false);
    expect(first.state.disabledBannerSet.value).toEqual(new Set());
    first.wrapper.unmount();

    settings.value = {
      ui: {scale: 125, system_zoom: 1500, ship_names_visible: 2100},
      banners: {disable_all: true, disabled_types: ['Victory']},
      cargo_view: {},
      slider_limits: {
        standard_recruit_max: 999,
        alliance_donation_max: 75,
        transporter_pattern_max: 80,
      },
      shortcuts: {},
    };
    const second = mountSettingsView();

    expect(second.state.effectiveScale.value).toBe(125);
    expect(second.state.effectiveSystemZoom.value).toBe(1500);
    expect(second.state.effectiveShipNamesVisible.value).toBe(2100);
    expect(second.state.effectiveStandardRecruitMax.value).toBe(STANDARD_RECRUIT_MAX);
    expect(second.state.effectiveAllianceDonationMax.value).toBe(75);
    expect(second.state.effectiveTransporterPatternMax.value).toBe(80);
    expect(second.state.allBannersDisabled.value).toBe(true);
    expect(second.state.disabledBannerSet.value).toEqual(new Set(['Victory']));
    second.wrapper.unmount();
  });

  it('updates numeric and boolean UI settings', () => {
    const {state, wrapper} = mountSettingsView();

    state.onSliderInput(inputEvent('125'));
    state.onSystemZoomInput(inputEvent('1750'));
    state.onShipNamesVisibleInput(inputEvent('2200'));
    state.onUiToggle('auto_open_sidebar', inputEvent('', true));

    expect(settings.value.ui).toMatchObject({
      scale: 125,
      system_zoom: 1750,
      ship_names_visible: 2200,
      auto_open_sidebar: true,
    });
    wrapper.unmount();
  });

  it('updates cargo settings and slider limits', () => {
    const {state, wrapper} = mountSettingsView();
    const defaultEvent = inputEvent('20');

    state.onCargoViewEnabledChange(inputEvent('', true));
    state.onCargoViewTargetChange('show_for_players', inputEvent('', true));
    state.onSliderLimitChange('standard_recruit_max', inputEvent('500'));
    state.onSliderLimitChange('transporter_pattern_max', defaultEvent);

    expect(settings.value.cargo_view.enabled).toBe(true);
    expect(settings.value.cargo_view.show_for_players).toBe(true);
    expect(settings.value.slider_limits.standard_recruit_max).toBe(STANDARD_RECRUIT_MAX);
    expect(settings.value.slider_limits.transporter_pattern_max).toBeNull();
    expect((defaultEvent.target as HTMLInputElement).value).toBe(String(GAME_DEFAULT_SLIDER_MAX));
    wrapper.unmount();
  });

  it('displays, disables, and clears shortcuts', () => {
    const {state, wrapper} = mountSettingsView();

    expect(state.shortcutDisplayLabel('trigger_main_action', 'Space')).toBe('Space');
    expect(state.isShortcutDisabled('trigger_main_action')).toBe(false);
    settings.value.shortcuts = {trigger_main_action: 'KeyZ'};
    expect(state.shortcutDisplayLabel('trigger_main_action', 'Space')).toBe('KeyZ');
    settings.value.shortcuts = {trigger_main_action: ''};
    expect(state.isShortcutDisabled('trigger_main_action')).toBe(true);
    state.clearShortcut('trigger_main_action');

    expect(mocks.setShortcut).toHaveBeenCalledWith('trigger_main_action', '');
    wrapper.unmount();
  });

  it('captures keyboard shortcuts and cancels capture with Escape', () => {
    const {state, wrapper} = mountSettingsView();

    state.startCapture('trigger_main_action');
    window.dispatchEvent(new KeyboardEvent('keydown', {code: 'KeyK', key: 'k'}));
    expect(mocks.setShortcut).toHaveBeenLastCalledWith('trigger_main_action', 'KeyK');
    expect(state.shortcutDisplayLabel('trigger_main_action', 'Space')).toBe('k');

    state.startCapture('trigger_main_action');
    window.dispatchEvent(new KeyboardEvent('keydown', {code: 'Space', key: ' '}));
    expect(mocks.setShortcut).toHaveBeenLastCalledWith('trigger_main_action', 'Space');

    state.startCapture('trigger_main_action');
    window.dispatchEvent(new KeyboardEvent('keydown', {code: 'Escape', key: 'Escape'}));
    expect(state.capturingKey.value).toBeNull();
    wrapper.unmount();
  });

  it('consumes captured shortcuts before application zoom handles them', () => {
    const zoomListener = vi.fn();
    window.addEventListener('keydown', zoomListener);
    const {state, wrapper} = mountSettingsView();

    state.startCapture('trigger_main_action');
    window.dispatchEvent(new KeyboardEvent('keydown', {
      bubbles: true,
      code: 'Equal',
      ctrlKey: true,
      key: '+',
    }));

    expect(mocks.setShortcut).toHaveBeenCalledWith('trigger_main_action', 'Equal');
    expect(zoomListener).not.toHaveBeenCalled();
    wrapper.unmount();
    window.removeEventListener('keydown', zoomListener);
  });

  it('ignores ordinary mouse buttons and captures auxiliary buttons', () => {
    const {state, wrapper} = mountSettingsView();

    state.startCapture('trigger_main_action');
    window.dispatchEvent(new MouseEvent('mousedown', {button: 1}));
    expect(mocks.setShortcut).not.toHaveBeenCalled();
    window.dispatchEvent(new MouseEvent('mousedown', {button: 3}));

    expect(mocks.setShortcut).toHaveBeenCalledWith('trigger_main_action', 'Mouse3');
    expect(state.shortcutDisplayLabel('trigger_main_action', 'Space')).toBe('Mouse3');
    wrapper.unmount();
  });

  it('updates banner settings and removes capture listeners on unmount', () => {
    const removeEventListener = vi.spyOn(window, 'removeEventListener');
    const {state, wrapper} = mountSettingsView();

    state.onDisableAllBannersChange(inputEvent('', true));
    state.onBannerTypeToggle('Victory', false);
    state.startCapture('trigger_main_action');
    wrapper.unmount();

    expect(settings.value.banners.disable_all).toBe(true);
    expect(mocks.setBannerTypeEnabled).toHaveBeenCalledWith('Victory', false);
    expect(removeEventListener).toHaveBeenCalledWith('keydown', expect.any(Function), true);
    expect(removeEventListener).toHaveBeenCalledWith('mousedown', expect.any(Function));
  });
});
