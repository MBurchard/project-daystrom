<script setup lang="ts">
import type {AppLanguage} from '@generated/AppLanguage';
import {
  GAME_DEFAULT_SLIDER_MAX,
  MAX_CONFIGURED_SLIDER_LIMIT,
  shortcutActions,
  STANDARD_RECRUIT_MAX,
  useSettingsView,
} from '@app/composables/useSettingsView';
import {useI18n} from '@app/i18n';
import settingsDefaults from '@app/locales/en/settings.json';
import toastDefaults from '@app/locales/en/toast.json';
import {getLogger} from '@app/log';
import {onBeforeUnmount, onMounted, ref} from 'vue';

import bannerCategories from './toast-banner-categories.json';

const props = defineProps<{
  /** Verified predecessor release available for recovery, if any. */
  rollbackVersion: string | null;
}>();
const emit = defineEmits<{
  close: [];
  openRollback: [];
}>();
const log = getLogger('Settings');
const {language, setLanguage, t} = useI18n('settings', settingsDefaults);
const {t: tToast} = useI18n('toast', toastDefaults);
const view = ref<HTMLElement | null>(null);
let previouslyFocused: HTMLElement | null = null;
const bannerCategoryLabels: Record<string, keyof typeof settingsDefaults> = {
  Armada: 'bannerArmada',
  Combat: 'bannerCombat',
  Economy: 'bannerEconomy',
  'Events & Challenges': 'bannerEvents',
  Faction: 'bannerFaction',
  Other: 'bannerOther',
  Station: 'bannerStation',
  Territory: 'bannerTerritory',
};

const {
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
} = useSettingsView();

/** Return to the accounts view unless Escape is cancelling an active shortcut capture. */
function handleEscape(event: KeyboardEvent): void {
  if (capturingKey.value !== null) {
    return;
  }
  event.preventDefault();
  emit('close');
}

/** Persist a language selected through the application settings. */
function onLanguageChange(event: Event): void {
  const nextLanguage = (event.target as HTMLSelectElement).value as AppLanguage;
  setLanguage(nextLanguage).catch(reason => log.error('Failed to change application language:', reason));
}

onMounted(() => {
  previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  view.value?.focus();
});

onBeforeUnmount(() => {
  previouslyFocused?.focus();
});
</script>

<template>
  <section ref="view"
      class="settings"
      aria-labelledby="settings-heading"
      tabindex="-1"
      @keydown.esc="handleEscape">
    <header class="settings-header">
      <h2 id="settings-heading">
        {{ t('heading') }}
      </h2>
      <button class="settings-back" @click="emit('close')">
        {{ t('backToAccounts') }}
      </button>
    </header>

    <section class="settings-category">
      <h3>{{ t('application') }}</h3>

      <div class="setting-row">
        <label for="app-language">{{ t('language') }}</label>
        <select id="app-language" :value="language" @change="onLanguageChange">
          <option value="en">
            {{ t('languageEnglish') }}
          </option>
          <option value="de">
            {{ t('languageGerman') }}
          </option>
          <option value="tlh">
            {{ t('languageKlingon') }}
          </option>
        </select>
      </div>
    </section>

    <section class="settings-category">
      <h3>{{ t('gameUi') }}</h3>

      <div class="setting-row">
        <label for="ui-scale">{{ t('uiScale') }}</label>
        <input id="ui-scale"
            type="range"
            min="50"
            max="200"
            step="5"
            :value="effectiveScale"
            @input="onSliderInput">
        <span class="scale-value">{{ effectiveScale }}%</span>
      </div>

      <div class="setting-row">
        <label for="system-zoom">{{ t('systemZoom') }}</label>
        <input id="system-zoom"
            type="range"
            min="1000"
            max="3000"
            step="50"
            :value="effectiveSystemZoom"
            @input="onSystemZoomInput">
        <span class="scale-value">{{ effectiveSystemZoom }}</span>
      </div>

      <div class="setting-row">
        <label for="ship-names-visible">{{ t('shipNames') }}</label>
        <input id="ship-names-visible"
            type="range"
            min="1000"
            max="3000"
            step="50"
            :value="effectiveShipNamesVisible"
            @input="onShipNamesVisibleInput">
        <span class="scale-value">{{ effectiveShipNamesVisible }}</span>
      </div>

      <div class="setting-row">
        <label for="auto-open-sidebar">{{ t('autoOpenChat') }}</label>
        <input id="auto-open-sidebar"
            type="checkbox"
            :checked="settings.ui.auto_open_sidebar ?? false"
            @change="onUiToggle('auto_open_sidebar', $event)">
      </div>

      <div class="setting-row">
        <label for="auto-expand-job-queue">{{ t('autoExpandQueue') }}</label>
        <input id="auto-expand-job-queue"
            type="checkbox"
            :checked="settings.ui.auto_expand_job_queue ?? false"
            @change="onUiToggle('auto_expand_job_queue', $event)">
      </div>

      <div class="setting-row">
        <label for="skip-reveal-sequence">{{ t('skipLootAnimation') }}</label>
        <input id="skip-reveal-sequence"
            type="checkbox"
            :checked="settings.ui.skip_reveal_sequence ?? true"
            @change="onUiToggle('skip_reveal_sequence', $event)">
      </div>

      <div class="setting-row">
        <label for="skip-first-popup">{{ t('skipFirstPopup') }}</label>
        <input id="skip-first-popup"
            type="checkbox"
            :checked="settings.ui.skip_first_popup ?? true"
            @change="onUiToggle('skip_first_popup', $event)">
      </div>
    </section>

    <section class="settings-category">
      <h3>{{ t('sliderLimits') }}</h3>

      <div class="setting-row">
        <label for="standard-recruit-max">{{ t('standardRecruit') }}</label>
        <input id="standard-recruit-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="STANDARD_RECRUIT_MAX"
            step="1"
            :value="effectiveStandardRecruitMax"
            @change="onSliderLimitChange('standard_recruit_max', $event)">
        <span class="setting-hint">{{ t('gameDefault') }}</span>
      </div>

      <div class="setting-row">
        <label for="alliance-donation-max">{{ t('allianceDonation') }}</label>
        <input id="alliance-donation-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="MAX_CONFIGURED_SLIDER_LIMIT"
            step="1"
            :value="effectiveAllianceDonationMax"
            @change="onSliderLimitChange('alliance_donation_max', $event)">
        <span class="setting-hint">{{ t('gameDefault') }}</span>
      </div>

      <div class="setting-row">
        <label for="transporter-pattern-max">{{ t('transporterPatterns') }}</label>
        <input id="transporter-pattern-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="MAX_CONFIGURED_SLIDER_LIMIT"
            step="1"
            :value="effectiveTransporterPatternMax"
            @change="onSliderLimitChange('transporter_pattern_max', $event)">
        <span class="setting-hint">{{ t('gameDefault') }}</span>
      </div>

      <p class="setting-caution">
        {{ t('sliderCaution') }}
      </p>
    </section>

    <section class="settings-category">
      <h3>{{ t('cargoView') }}</h3>

      <div class="setting-row">
        <label for="cargo-view-enabled">{{ t('autoOpenCargo') }}</label>
        <input id="cargo-view-enabled"
            type="checkbox"
            :checked="settings.cargo_view.enabled ?? false"
            @change="onCargoViewEnabledChange">
      </div>

      <div class="cargo-targets" :class="{ disabled: !(settings.cargo_view.enabled ?? false) }">
        <div class="setting-row">
          <label for="cargo-view-hostiles">{{ t('hostiles') }}</label>
          <input id="cargo-view-hostiles"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_hostiles ?? true"
              @change="onCargoViewTargetChange('show_for_hostiles', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-armadas">{{ t('armadas') }}</label>
          <input id="cargo-view-armadas"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_armadas ?? true"
              @change="onCargoViewTargetChange('show_for_armadas', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-stations">{{ t('stations') }}</label>
          <input id="cargo-view-stations"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_stations ?? true"
              @change="onCargoViewTargetChange('show_for_stations', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-players">{{ t('playerShips') }}</label>
          <input id="cargo-view-players"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_players ?? false"
              @change="onCargoViewTargetChange('show_for_players', $event)">
        </div>
      </div>
    </section>

    <section class="settings-category">
      <h3>{{ t('shortcuts') }}</h3>

      <div v-for="action in shortcutActions" :key="action.key" class="setting-row">
        <label>{{ t(action.labelKey) }}</label>
        <span class="shortcut-key"
            :class="{ disabled: isShortcutDisabled(action.key), capturing: capturingKey === action.key }"
            tabindex="0"
            @click="startCapture(action.key)">
          {{ capturingKey === action.key ? '...' : isShortcutDisabled(action.key) ? '—'
            : shortcutDisplayLabel(action.key, action.defaultCode) }}
        </span>
        <button v-if="!isShortcutDisabled(action.key) && capturingKey !== action.key"
            class="clear-btn shortcut-clear" :title="t('disableShortcut')"
            @click="clearShortcut(action.key)">
          ✕
        </button>
      </div>
    </section>

    <section class="settings-category">
      <h3>{{ t('toastBanners') }}</h3>

      <div class="setting-row">
        <label for="disable-all-banners">{{ t('disableAllBanners') }}</label>
        <input id="disable-all-banners"
            type="checkbox"
            :checked="allBannersDisabled"
            @change="onDisableAllBannersChange">
      </div>

      <div class="banner-categories" :class="{ disabled: allBannersDisabled }">
        <details v-for="(types, category) in bannerCategories" :key="category"
            class="banner-category">
          <summary>{{ t(bannerCategoryLabels[category]!) }}</summary>
          <div class="banner-type-list">
            <label v-for="name in types" :key="name" class="banner-type">
              <input type="checkbox"
                  :checked="!disabledBannerSet.has(name)"
                  :disabled="allBannersDisabled"
                  @change="onBannerTypeToggle(name, ($event.target as HTMLInputElement).checked)">
              {{ tToast(name as keyof typeof toastDefaults) }}
            </label>
          </div>
        </details>
      </div>
    </section>

    <section class="settings-category version-return-zone">
      <h3>{{ t('previousVersion') }}</h3>
      <p class="version-return-description">
        {{ t('previousVersionDescription') }}
      </p>
      <button v-if="props.rollbackVersion"
          class="version-return-action"
          @click="emit('openRollback')">
        {{ t('returnToVersion', { version: props.rollbackVersion }) }}
      </button>
      <span v-else class="version-return-unavailable">{{ t('noPreviousVersion') }}</span>
    </section>
  </section>
</template>

<style scoped>
.settings {
  flex: 1;
  min-height: 0;
  margin-top: 1.5rem;
  padding: 0 0.5rem 1rem 0.25rem;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 0.75rem;
}

.settings-header h2 {
  margin: 0;
}

.settings-back {
  cursor: pointer;
}

.settings-category h3 {
  margin: 1rem 0 0.5rem;
  font-size: 0.95rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  opacity: 0.7;
}

.setting-row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem 0;
}

.setting-row label {
  min-width: 12rem;
}

.setting-row input[type="range"] {
  flex: 1;
  cursor: pointer;
}

.setting-row input[type="checkbox"] {
  cursor: pointer;
}

.limit-input {
  width: 8rem;
  box-sizing: border-box;
}

.setting-hint {
  min-width: 8.5rem;
  font-size: 0.8rem;
  opacity: 0.6;
}

.setting-caution {
  margin: 0.25rem 0 0;
  font-size: 0.8rem;
  opacity: 0.7;
}

.version-return-zone {
  margin-top: 1.5rem;
  padding: 1rem;
  border: 1px solid #d29922;
  border-radius: 0.5rem;
  background: rgb(210 153 34 / 8%);
}

.version-return-zone h3 {
  margin-top: 0;
  color: #b77900;
  opacity: 1;
}

.version-return-description {
  user-select: text;
}

.version-return-action {
  padding: 0.4rem 0.85rem;
  border: 1px solid #b77900;
  border-radius: 999px;
  background: rgb(210 153 34 / 14%);
  color: #8a5a00;
  cursor: pointer;
  font-weight: 600;
}

.version-return-action:hover {
  background: rgb(210 153 34 / 24%);
}

.version-return-action:focus-visible {
  outline: 2px solid #d29922;
  outline-offset: 2px;
}

.version-return-unavailable {
  color: #777;
  font-size: 0.85rem;
}

.scale-value {
  min-width: 3.5rem;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.shortcut-key {
  font-family: monospace;
  padding: 0.2rem 0.5rem;
  border: 1px solid rgb(255 255 255 / 20%);
  border-radius: 0.25rem;
  min-width: 4rem;
  text-align: center;
  cursor: pointer;
}

.shortcut-key:hover {
  border-color: rgb(255 255 255 / 40%);
}

.shortcut-key.disabled {
  opacity: 0.4;
  font-style: italic;
}

.shortcut-key.capturing {
  border-color: rgb(100 180 255 / 60%);
  animation: pulse 1s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.shortcut-clear {
  background: none;
  border: none;
  cursor: pointer;
  padding: 0.25rem 0.4rem;
  line-height: 1;
  color: inherit;
  opacity: 0.6;
  font-size: 1rem;
}

.shortcut-clear:hover {
  opacity: 1;
}

.cargo-targets.disabled {
  opacity: 0.55;
}

.banner-categories {
  margin-top: 0.25rem;
}

.banner-categories.disabled {
  opacity: 0.4;
  pointer-events: none;
}

.banner-category {
  margin-bottom: 0.25rem;
}

.banner-category summary {
  cursor: pointer;
  padding: 0.25rem 0;
  font-weight: 500;
}

.banner-type-list {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  padding: 0.25rem 0 0.5rem 1.25rem;
}

.banner-type {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
  font-size: 0.9rem;
}

.banner-type input[type="checkbox"] {
  cursor: pointer;
}
</style>
