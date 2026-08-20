import type {GameSettings} from '@generated/GameSettings';
import {mount} from '@vue/test-utils';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {nextTick, ref} from 'vue';
import SettingsView from '../SettingsView.vue';

const mocks = vi.hoisted(() => ({
  clearShortcut: vi.fn(),
  isShortcutDisabled: vi.fn(),
  language: {value: 'en'},
  languageError: vi.fn(),
  onBannerTypeToggle: vi.fn(),
  onCargoViewEnabledChange: vi.fn(),
  onCargoViewTargetChange: vi.fn(),
  onDisableAllBannersChange: vi.fn(),
  onShipNamesVisibleInput: vi.fn(),
  onSliderInput: vi.fn(),
  onSliderLimitChange: vi.fn(),
  onSystemZoomInput: vi.fn(),
  onUiToggle: vi.fn(),
  shortcutDisplayLabel: vi.fn(),
  startCapture: vi.fn(),
  setLanguage: vi.fn(),
  setTheme: vi.fn(),
  theme: {value: 'omega'},
}));

let state: Record<string, unknown>;

vi.mock('@app/composables/useSettingsView', () => ({
  GAME_DEFAULT_SLIDER_MAX: 50,
  MAX_CONFIGURED_SLIDER_LIMIT: 4_294_967_295,
  STANDARD_RECRUIT_MAX: 150,
  shortcutActions: [{key: 'trigger_main_action', labelKey: 'triggerMainAction', defaultCode: 'Space'}],
  useSettingsView: () => state,
}));
vi.mock('@app/i18n', () => ({
  useI18n: () => ({
    language: mocks.language,
    setLanguage: mocks.setLanguage,
    t: (key: string, values?: Record<string, string>) => {
      const translations: Record<string, string> = {
        noPreviousVersion: 'No previous version available',
        returnToVersion: 'Return to version {{version}}',
        triggerMainAction: 'Trigger Main Action',
      };
      return (translations[key] ?? key).replace('{{version}}', values?.version ?? '');
    },
  }),
}));
vi.mock('@app/log', () => ({getLogger: () => ({error: mocks.languageError})}));
vi.mock('@app/theme', () => ({
  useTheme: () => ({theme: mocks.theme, setTheme: mocks.setTheme}),
}));

/** Build complete settings for component rendering. */
function settings(): GameSettings {
  return {
    ui: {},
    banners: {},
    cargo_view: {},
    slider_limits: {},
    shortcuts: {},
  };
}

describe('settingsView', () => {
  const currentSettings = ref(settings());
  const capturingKey = ref<string | null>(null);
  const allBannersDisabled = ref(false);
  const disabledBannerSet = ref(new Set<string>());

  beforeEach(() => {
    vi.clearAllMocks();
    currentSettings.value = settings();
    capturingKey.value = null;
    allBannersDisabled.value = false;
    disabledBannerSet.value = new Set();
    mocks.isShortcutDisabled.mockReturnValue(false);
    mocks.shortcutDisplayLabel.mockReturnValue('Space');
    mocks.setLanguage.mockResolvedValue(undefined);
    mocks.setTheme.mockResolvedValue(undefined);
    state = {
      settings: currentSettings,
      effectiveScale: ref(100),
      effectiveSystemZoom: ref(1000),
      effectiveShipNamesVisible: ref(1800),
      effectiveStandardRecruitMax: ref(50),
      effectiveAllianceDonationMax: ref(50),
      effectiveTransporterPatternMax: ref(50),
      onSliderInput: mocks.onSliderInput,
      onSystemZoomInput: mocks.onSystemZoomInput,
      onShipNamesVisibleInput: mocks.onShipNamesVisibleInput,
      onUiToggle: mocks.onUiToggle,
      onCargoViewEnabledChange: mocks.onCargoViewEnabledChange,
      onCargoViewTargetChange: mocks.onCargoViewTargetChange,
      onSliderLimitChange: mocks.onSliderLimitChange,
      capturingKey,
      shortcutDisplayLabel: mocks.shortcutDisplayLabel,
      isShortcutDisabled: mocks.isShortcutDisabled,
      clearShortcut: mocks.clearShortcut,
      startCapture: mocks.startCapture,
      allBannersDisabled,
      disabledBannerSet,
      onDisableAllBannersChange: mocks.onDisableAllBannersChange,
      onBannerTypeToggle: mocks.onBannerTypeToggle,
    };
  });

  it('persists language selections and reports unexpected change failures', async () => {
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});
    await wrapper.get('.settings-back').trigger('click');
    expect(wrapper.emitted('close')).toHaveLength(1);

    await wrapper.get('#app-language').setValue('de');
    expect(mocks.setLanguage).toHaveBeenCalledWith('de');

    await wrapper.get('#app-language').setValue('tlh');
    expect(mocks.setLanguage).toHaveBeenCalledWith('tlh');

    const reason = new Error('translation failure');
    mocks.setLanguage.mockRejectedValue(reason);
    await wrapper.get('#app-language').setValue('en');
    await Promise.resolve();
    expect(mocks.languageError).toHaveBeenCalledWith('Failed to change application language:', reason);
  });

  it('persists theme selections and reports unexpected change failures', async () => {
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});

    await wrapper.get('#app-theme').setValue('classic');
    expect(mocks.setTheme).toHaveBeenCalledWith('classic');

    const reason = new Error('theme failure');
    mocks.setTheme.mockRejectedValue(reason);
    await wrapper.get('#app-theme').setValue('omega');
    await Promise.resolve();
    expect(mocks.languageError).toHaveBeenCalledWith('Failed to change application theme:', reason);
  });

  it('moves focus into settings, handles Escape, and restores the previous focus', async () => {
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    const wrapper = mount(SettingsView, {
      attachTo: document.body,
      props: {rollbackVersion: null},
    });

    expect(document.activeElement).toBe(wrapper.get('.settings').element);

    capturingKey.value = 'trigger_main_action';
    await wrapper.get('.settings').trigger('keydown', {code: 'Escape', key: 'Escape'});
    expect(wrapper.emitted('close')).toBeUndefined();

    capturingKey.value = null;
    await wrapper.get('.settings').trigger('keydown', {code: 'Escape', key: 'Escape'});
    expect(wrapper.emitted('close')).toHaveLength(1);

    wrapper.unmount();
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it('mounts without a previously focused HTML element', () => {
    const activeElement = vi.spyOn(document, 'activeElement', 'get').mockReturnValue(null);
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});

    wrapper.unmount();
    activeElement.mockRestore();
  });

  it('dispatches all settings input events through the composable', async () => {
    currentSettings.value.cargo_view.enabled = true;
    const wrapper = mount(SettingsView, {props: {rollbackVersion: '0.9.1'}});

    await wrapper.get('#ui-scale').trigger('input');
    await wrapper.get('#system-zoom').trigger('input');
    await wrapper.get('#ship-names-visible').trigger('input');
    await wrapper.get('#auto-open-sidebar').trigger('change');
    await wrapper.get('#auto-expand-job-queue').trigger('change');
    await wrapper.get('#skip-reveal-sequence').trigger('change');
    await wrapper.get('#skip-first-popup').trigger('change');
    await wrapper.get('#standard-recruit-max').trigger('change');
    await wrapper.get('#alliance-donation-max').trigger('change');
    await wrapper.get('#transporter-pattern-max').trigger('change');
    await wrapper.get('#cargo-view-enabled').trigger('change');
    await wrapper.get('#cargo-view-hostiles').trigger('change');
    await wrapper.get('#cargo-view-armadas').trigger('change');
    await wrapper.get('#cargo-view-stations').trigger('change');
    await wrapper.get('#cargo-view-players').trigger('change');

    expect(mocks.onSliderInput).toHaveBeenCalledOnce();
    expect(mocks.onSystemZoomInput).toHaveBeenCalledOnce();
    expect(mocks.onShipNamesVisibleInput).toHaveBeenCalledOnce();
    expect(mocks.onUiToggle).toHaveBeenCalledTimes(4);
    expect(mocks.onSliderLimitChange).toHaveBeenCalledTimes(3);
    expect(mocks.onCargoViewEnabledChange).toHaveBeenCalledOnce();
    expect(mocks.onCargoViewTargetChange).toHaveBeenCalledTimes(4);
  });

  it('renders enabled and disabled cargo targets', async () => {
    currentSettings.value.cargo_view.enabled = true;
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});
    expect(wrapper.get('.cargo-targets').classes()).not.toContain('disabled');
    expect(wrapper.get('#cargo-view-hostiles').attributes('disabled')).toBeUndefined();

    currentSettings.value.cargo_view.enabled = false;
    await nextTick();

    expect(wrapper.get('.cargo-targets').classes()).toContain('disabled');
    expect(wrapper.get('#cargo-view-hostiles').attributes('disabled')).toBeDefined();
  });

  it('handles shortcut display, capture, disablement, and clearing', async () => {
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});
    const shortcut = wrapper.get('.shortcut-key');
    expect(shortcut.text()).toBe('Space');
    await shortcut.trigger('click');
    await wrapper.get('.shortcut-clear').trigger('click');
    expect(mocks.startCapture).toHaveBeenCalledWith('trigger_main_action');
    expect(mocks.clearShortcut).toHaveBeenCalledWith('trigger_main_action');

    capturingKey.value = 'trigger_main_action';
    await nextTick();
    expect(wrapper.get('.shortcut-key').text()).toBe('...');
    expect(wrapper.find('.shortcut-clear').exists()).toBe(false);

    capturingKey.value = null;
    mocks.isShortcutDisabled.mockReturnValue(true);
    currentSettings.value.ui.scale = 101;
    await nextTick();
    expect(wrapper.get('.shortcut-key').text()).toBe('—');
    expect(wrapper.get('.shortcut-key').classes()).toContain('disabled');
  });

  it('handles banner controls in enabled and disabled states', async () => {
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});
    await wrapper.get('#disable-all-banners').trigger('change');
    const banner = wrapper.get('.banner-type input');
    await banner.setValue(false);

    expect(mocks.onDisableAllBannersChange).toHaveBeenCalledOnce();
    expect(mocks.onBannerTypeToggle).toHaveBeenCalledWith(expect.any(String), false);

    disabledBannerSet.value = new Set(['AllianceLevelUp']);
    allBannersDisabled.value = true;
    await nextTick();
    expect(wrapper.get('.banner-categories').classes()).toContain('disabled');
    expect(wrapper.get('.banner-type input').attributes('disabled')).toBeDefined();
  });

  it('offers a return only when a verified predecessor exists', async () => {
    const wrapper = mount(SettingsView, {props: {rollbackVersion: null}});
    expect(wrapper.get('.version-return-unavailable').text()).toContain('No previous version');
    expect(wrapper.find('.version-return-action').exists()).toBe(false);

    await wrapper.setProps({rollbackVersion: '0.9.1'});
    const versionReturn = wrapper.get('.version-return-action');
    expect(versionReturn.text()).toContain('0.9.1');
    await versionReturn.trigger('click');
    expect(wrapper.emitted('openRollback')).toHaveLength(1);
  });
});
