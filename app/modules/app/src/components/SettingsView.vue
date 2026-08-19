<script setup lang="ts">
import {
  GAME_DEFAULT_SLIDER_MAX,
  MAX_CONFIGURED_SLIDER_LIMIT,
  shortcutActions,
  STANDARD_RECRUIT_MAX,
  useSettingsView,
} from '@app/composables/useSettingsView';

import bannerCategories from './toast-banner-categories.json';

const props = defineProps<{
  /** Verified predecessor release available for recovery, if any. */
  rollbackVersion: string | null;
}>();

const emit = defineEmits<{
  openRollback: [];
}>();

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
</script>

<template>
  <div class="settings">
    <section class="settings-category">
      <h3>Game UI</h3>

      <div class="setting-row">
        <label for="ui-scale">UI Scale</label>
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
        <label for="system-zoom">System Zoom</label>
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
        <label for="ship-names-visible">Ship Names Visible</label>
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
        <label for="auto-open-sidebar">Auto-open Chat Sidebar</label>
        <input id="auto-open-sidebar"
            type="checkbox"
            :checked="settings.ui.auto_open_sidebar ?? false"
            @change="onUiToggle('auto_open_sidebar', $event)">
      </div>

      <div class="setting-row">
        <label for="auto-expand-job-queue">Auto-expand Job Queue</label>
        <input id="auto-expand-job-queue"
            type="checkbox"
            :checked="settings.ui.auto_expand_job_queue ?? false"
            @change="onUiToggle('auto_expand_job_queue', $event)">
      </div>

      <div class="setting-row">
        <label for="skip-reveal-sequence">Skip Loot Box Animation</label>
        <input id="skip-reveal-sequence"
            type="checkbox"
            :checked="settings.ui.skip_reveal_sequence ?? true"
            @change="onUiToggle('skip_reveal_sequence', $event)">
      </div>

      <div class="setting-row">
        <label for="skip-first-popup">Skip First Popup</label>
        <input id="skip-first-popup"
            type="checkbox"
            :checked="settings.ui.skip_first_popup ?? true"
            @change="onUiToggle('skip_first_popup', $event)">
      </div>
    </section>

    <section class="settings-category">
      <h3>Slider Limits</h3>

      <div class="setting-row">
        <label for="standard-recruit-max">Standard Recruit</label>
        <input id="standard-recruit-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="STANDARD_RECRUIT_MAX"
            step="1"
            :value="effectiveStandardRecruitMax"
            @change="onSliderLimitChange('standard_recruit_max', $event)">
        <span class="setting-hint">Game default: 50</span>
      </div>

      <div class="setting-row">
        <label for="alliance-donation-max">Alliance Donation</label>
        <input id="alliance-donation-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="MAX_CONFIGURED_SLIDER_LIMIT"
            step="1"
            :value="effectiveAllianceDonationMax"
            @change="onSliderLimitChange('alliance_donation_max', $event)">
        <span class="setting-hint">Game default: 50</span>
      </div>

      <div class="setting-row">
        <label for="transporter-pattern-max">Transporter Patterns*</label>
        <input id="transporter-pattern-max"
            class="limit-input"
            type="number"
            :min="GAME_DEFAULT_SLIDER_MAX"
            :max="MAX_CONFIGURED_SLIDER_LIMIT"
            step="1"
            :value="effectiveTransporterPatternMax"
            @change="onSliderLimitChange('transporter_pattern_max', $event)">
        <span class="setting-hint">Game default: 50</span>
      </div>

      <p class="setting-caution">
        * No safe maximum is known. Increase this value cautiously and test in small steps.
      </p>
    </section>

    <section class="settings-category">
      <h3>Cargo View</h3>

      <div class="setting-row">
        <label for="cargo-view-enabled">Auto-open Cargo</label>
        <input id="cargo-view-enabled"
            type="checkbox"
            :checked="settings.cargo_view.enabled ?? false"
            @change="onCargoViewEnabledChange">
      </div>

      <div class="cargo-targets" :class="{ disabled: !(settings.cargo_view.enabled ?? false) }">
        <div class="setting-row">
          <label for="cargo-view-hostiles">Hostiles</label>
          <input id="cargo-view-hostiles"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_hostiles ?? true"
              @change="onCargoViewTargetChange('show_for_hostiles', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-armadas">Armadas</label>
          <input id="cargo-view-armadas"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_armadas ?? true"
              @change="onCargoViewTargetChange('show_for_armadas', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-stations">Stations</label>
          <input id="cargo-view-stations"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_stations ?? true"
              @change="onCargoViewTargetChange('show_for_stations', $event)">
        </div>

        <div class="setting-row">
          <label for="cargo-view-players">Player Ships</label>
          <input id="cargo-view-players"
              type="checkbox"
              :disabled="!(settings.cargo_view.enabled ?? false)"
              :checked="settings.cargo_view.show_for_players ?? false"
              @change="onCargoViewTargetChange('show_for_players', $event)">
        </div>
      </div>
    </section>

    <section class="settings-category">
      <h3>Shortcuts</h3>

      <div v-for="action in shortcutActions" :key="action.key" class="setting-row">
        <label>{{ action.label }}</label>
        <span class="shortcut-key"
            :class="{ disabled: isShortcutDisabled(action.key), capturing: capturingKey === action.key }"
            tabindex="0"
            @click="startCapture(action.key)">
          {{ capturingKey === action.key ? '...' : isShortcutDisabled(action.key) ? '—'
            : shortcutDisplayLabel(action.key, action.defaultCode) }}
        </span>
        <button v-if="!isShortcutDisabled(action.key) && capturingKey !== action.key"
            class="clear-btn shortcut-clear" title="Disable shortcut"
            @click="clearShortcut(action.key)">
          ✕
        </button>
      </div>
    </section>

    <section class="settings-category">
      <h3>Toast Banners</h3>

      <div class="setting-row">
        <label for="disable-all-banners">Disable All Banners</label>
        <input id="disable-all-banners"
            type="checkbox"
            :checked="allBannersDisabled"
            @change="onDisableAllBannersChange">
      </div>

      <div class="banner-categories" :class="{ disabled: allBannersDisabled }">
        <details v-for="(types, category) in bannerCategories" :key="category"
            class="banner-category">
          <summary>{{ category }}</summary>
          <div class="banner-type-list">
            <label v-for="name in types" :key="name" class="banner-type">
              <input type="checkbox"
                  :checked="!disabledBannerSet.has(name)"
                  :disabled="allBannersDisabled"
                  @change="onBannerTypeToggle(name, ($event.target as HTMLInputElement).checked)">
              {{ name }}
            </label>
          </div>
        </details>
      </div>
    </section>

    <section class="settings-category">
      <h3>Recovery</h3>
      <p class="recovery-description">
        Restore the previous verified Daystrom release only if the current release causes problems.
      </p>
      <button :disabled="!props.rollbackVersion" @click="emit('openRollback')">
        {{ props.rollbackVersion ? `Recovery options for ${props.rollbackVersion}` : 'No recovery version available' }}
      </button>
    </section>
  </div>
</template>

<style scoped>
.settings {
  padding: 0 0.25rem;
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

.recovery-description {
  max-width: 34rem;
  user-select: text;
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
