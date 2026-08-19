import {useSettings} from '@app/composables/useSettings';
import {computed, onBeforeUnmount, ref} from 'vue';

export const GAME_DEFAULT_SLIDER_MAX = 50;
export const STANDARD_RECRUIT_MAX = 150;
export const MAX_CONFIGURED_SLIDER_LIMIT = 4_294_967_295;

/** Known shortcut actions with display labels and default bindings. */
export const shortcutActions = [
  {key: 'trigger_main_action', labelKey: 'triggerMainAction' as const, defaultCode: 'Space'},
];

type CargoViewTargetKey =
  'show_for_hostiles' |
  'show_for_armadas' |
  'show_for_stations' |
  'show_for_players';

type SliderLimitKey = 'standard_recruit_max' | 'alliance_donation_max' | 'transporter_pattern_max';

/**
 * Normalize a configured slider maximum.
 *
 * The game default is represented as null so it remains distinguishable from an explicit override.
 *
 * @param rawValue - Numeric input value to normalize.
 * @param upperBound - Largest supported value for this slider.
 * @returns The configured maximum, or null to preserve the game default.
 */
export function normalizeSliderLimit(rawValue: string, upperBound: number): number | null {
  const parsed = Number(rawValue);
  const maximum = Number.isFinite(parsed) ?
      Math.min(upperBound, Math.max(GAME_DEFAULT_SLIDER_MAX, Math.trunc(parsed))) :
    GAME_DEFAULT_SLIDER_MAX;

  return maximum === GAME_DEFAULT_SLIDER_MAX ? null : maximum;
}

/**
 * Adapt the settings state and mutations for the settings view.
 *
 * DOM event handling and transient shortcut-capture state live here, while persistence and reusable
 * settings mutations remain in useSettings.
 */
export function useSettingsView() {
  const {settings, update, setShortcut, setBannerTypeEnabled} = useSettings();

  const effectiveScale = computed(() => settings.value.ui.scale ?? 100);
  const effectiveSystemZoom = computed(() => settings.value.ui.system_zoom ?? 1000);
  const effectiveShipNamesVisible = computed(() => settings.value.ui.ship_names_visible ?? 1800);
  const effectiveStandardRecruitMax = computed(
    () => Math.min(
      settings.value.slider_limits.standard_recruit_max ?? GAME_DEFAULT_SLIDER_MAX,
      STANDARD_RECRUIT_MAX,
    ),
  );
  const effectiveAllianceDonationMax = computed(
    () => settings.value.slider_limits.alliance_donation_max ?? GAME_DEFAULT_SLIDER_MAX,
  );
  const effectiveTransporterPatternMax = computed(
    () => settings.value.slider_limits.transporter_pattern_max ?? GAME_DEFAULT_SLIDER_MAX,
  );

  /** Update the UI scale from its range input. */
  function onSliderInput(event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.ui.scale = Number(target.value);
    });
  }

  /** Update the system zoom distance from its range input. */
  function onSystemZoomInput(event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.ui.system_zoom = Number(target.value);
    });
  }

  /** Update the ship-name visibility distance from its range input. */
  function onShipNamesVisibleInput(event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.ui.ship_names_visible = Number(target.value);
    });
  }

  /** Update a boolean UI setting from a checkbox. */
  function onUiToggle(
    key: 'auto_open_sidebar' | 'auto_expand_job_queue' | 'skip_reveal_sequence' | 'skip_first_popup',
    event: Event,
  ) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.ui[key] = target.checked;
    });
  }

  /** Update the cargo auto-open master switch. */
  function onCargoViewEnabledChange(event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.cargo_view.enabled = target.checked;
    });
  }

  /** Update one cargo auto-open target type. */
  function onCargoViewTargetChange(key: CargoViewTargetKey, event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.cargo_view[key] = target.checked;
    });
  }

  /** Normalize and store a configured in-game slider maximum. */
  function onSliderLimitChange(key: SliderLimitKey, event: Event) {
    const target = event.target as HTMLInputElement;
    const upperBound = key === 'standard_recruit_max' ? STANDARD_RECRUIT_MAX : MAX_CONFIGURED_SLIDER_LIMIT;
    const maximum = normalizeSliderLimit(target.value, upperBound);

    update((value) => {
      value.slider_limits[key] = maximum;
    });
    target.value = String(maximum ?? GAME_DEFAULT_SLIDER_MAX);
  }

  // ---- Shortcut capture -------------------------------------------------------------

  /** Display labels learned from keyboard events for the active keyboard layout. */
  const keyDisplayLabels: Record<string, string> = {Space: 'Space'};
  const capturingKey = ref<string | null>(null);

  function shortcutDisplayLabel(key: string, defaultCode: string): string {
    const code = settings.value.shortcuts?.[key] ?? defaultCode;
    return keyDisplayLabels[code] ?? code;
  }

  function isShortcutDisabled(key: string): boolean {
    return settings.value.shortcuts?.[key] === '';
  }

  function stopShortcutCapture() {
    capturingKey.value = null;
    window.removeEventListener('keydown', onCaptureKey, true);
    window.removeEventListener('mousedown', onCaptureMouse);
  }

  function clearShortcut(key: string) {
    setShortcut(key, '');
  }

  function finishCapture(code: string, label: string) {
    const key = capturingKey.value!;
    stopShortcutCapture();
    keyDisplayLabels[code] = label;
    setShortcut(key, code);
  }

  function startCapture(key: string) {
    stopShortcutCapture();
    capturingKey.value = key;
    window.addEventListener('keydown', onCaptureKey, true);
    window.addEventListener('mousedown', onCaptureMouse);
  }

  function onCaptureKey(event: KeyboardEvent) {
    event.preventDefault();
    event.stopImmediatePropagation();

    if (event.code === 'Escape') {
      stopShortcutCapture();
      return;
    }

    finishCapture(event.code, event.code === 'Space' ? 'Space' : event.key);
  }

  function onCaptureMouse(event: MouseEvent) {
    if (event.button < 3) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();

    const code = `Mouse${event.button}`;
    finishCapture(code, code);
  }

  onBeforeUnmount(stopShortcutCapture);

  // ---- Toast banners ---------------------------------------------------------------

  const allBannersDisabled = computed(() => settings.value.banners.disable_all ?? false);
  const disabledBannerSet = computed(() => new Set(settings.value.banners.disabled_types ?? []));

  function onDisableAllBannersChange(event: Event) {
    const target = event.target as HTMLInputElement;
    update((value) => {
      value.banners.disable_all = target.checked;
    });
  }

  function onBannerTypeToggle(name: string, checked: boolean) {
    setBannerTypeEnabled(name, checked);
  }

  return {
    settings,
    effectiveScale,
    effectiveSystemZoom,
    effectiveShipNamesVisible,
    effectiveStandardRecruitMax,
    effectiveAllianceDonationMax,
    effectiveTransporterPatternMax,
    onSliderInput,
    onSystemZoomInput,
    onShipNamesVisibleInput,
    onUiToggle,
    onCargoViewEnabledChange,
    onCargoViewTargetChange,
    onSliderLimitChange,
    capturingKey,
    shortcutDisplayLabel,
    isShortcutDisabled,
    clearShortcut,
    startCapture,
    allBannersDisabled,
    disabledBannerSet,
    onDisableAllBannersChange,
    onBannerTypeToggle,
  };
}
