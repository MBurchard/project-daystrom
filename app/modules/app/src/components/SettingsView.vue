<script setup lang="ts">
import {useSettings} from '@app/composables/useSettings';
import {computed, onBeforeUnmount, ref} from 'vue';

import bannerCategories from './toast-banner-categories.json';

const emit = defineEmits<{
  close: [];
}>();

const {settings, save} = useSettings();

/**
 * Handle slider input: update the settings ref and send to backend.
 *
 * @param event - Native input event from the range slider.
 */
function onSliderInput(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.scale = Number(target.value);
  save();
}

/**
 * Toggle the "Auto-open Chat Sidebar" checkbox and send to backend.
 *
 * @param event - Native change event from the checkbox.
 */
function onAutoOpenSidebarChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.auto_open_sidebar = target.checked;
  save();
}

/**
 * Toggle the "Auto-expand Job Queue" checkbox and send to backend.
 *
 * @param event - Native change event from the checkbox.
 */
function onAutoExpandJobQueueChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.auto_expand_job_queue = target.checked;
  save();
}

/**
 * Toggle the "Skip Reveal Sequence" checkbox and send to backend.
 *
 * @param event - Native change event from the checkbox.
 */
function onSkipRevealSequenceChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.skip_reveal_sequence = target.checked;
  save();
}

/**
 * Toggle the "Skip First Popup" checkbox and send to backend.
 *
 * @param event - Native change event from the checkbox.
 */
function onSkipFirstPopupChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.skip_first_popup = target.checked;
  save();
}

/**
 * Toggle the cargo auto-open master switch and send to backend.
 *
 * @param event - Native change event from the checkbox.
 */
function onCargoViewEnabledChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.cargo_view.enabled = target.checked;
  save();
}

/**
 * Toggle one target type in the cargo auto-open settings.
 *
 * @param key - Cargo view setting key to patch.
 * @param event - Native change event from the checkbox.
 */
function onCargoViewTargetChange(
  key: 'show_for_hostiles' | 'show_for_armadas' | 'show_for_stations' | 'show_for_players',
  event: Event,
) {
  const target = event.target as HTMLInputElement;
  settings.value.cargo_view[key] = target.checked;
  save();
}

/** Effective UI scale, defaulting to 100% when not set. */
const effectiveScale = computed(() => settings.value.ui.scale ?? 100);

/** Effective system zoom distance, defaulting to 1000 when not set. */
const effectiveSystemZoom = computed(() => settings.value.ui.system_zoom ?? 1000);

/** Effective ship names visibility distance, defaulting to 1800 when not set. */
const effectiveShipNamesVisible = computed(() => settings.value.ui.ship_names_visible ?? 1800);

/**
 * Handle system zoom slider input: update the settings ref and send to backend.
 *
 * @param event - Native input event from the range slider.
 */
function onSystemZoomInput(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.system_zoom = Number(target.value);
  save();
}

/**
 * Handle ship names visible slider input: update the settings ref and send to backend.
 *
 * @param event - Native input event from the range slider.
 */
function onShipNamesVisibleInput(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.ui.ship_names_visible = Number(target.value);
  save();
}

// ---- Shortcut handlers ------------------------------------------------------

/** Known shortcut actions with display labels and default bindings (as `event.code` values). */
const shortcutActions = [
  {key: 'trigger_main_action', label: 'Trigger Main Action', defaultCode: 'Space'},
];

/**
 * Display labels for key codes, populated by key capture events.
 * Maps `event.code` (e.g. "Slash") to event.key (e.g. "-" on German layout).
 */
const keyDisplayLabels: Record<string, string> = {Space: 'Space'};

/**
 * Get the display label for a shortcut action (localized key name or code fallback).
 *
 * @param key - The action identifier.
 * @param defaultCode - The default key code.
 */
function shortcutDisplayLabel(key: string, defaultCode: string): string {
  const code = settings.value.shortcuts?.[key] ?? defaultCode;
  return keyDisplayLabels[code] ?? code;
}

/**
 * Whether a shortcut is explicitly disabled (empty string).
 *
 * @param key - The action identifier.
 */
function isShortcutDisabled(key: string): boolean {
  return settings.value.shortcuts?.[key] === '';
}

/** The action key currently waiting for a keypress, or null if not capturing. */
const capturingKey = ref<string | null>(null);

/** Remove the shortcut capture listeners and reset the current capture state. */
function stopShortcutCapture() {
  capturingKey.value = null;
  window.removeEventListener('keydown', onCaptureKey);
  window.removeEventListener('mousedown', onCaptureMouse);
}

/**
 * Disable a shortcut by setting it to an empty string.
 *
 * @param key - The action identifier.
 */
function clearShortcut(key: string) {
  const shortcuts = settings.value.shortcuts ??= {};
  shortcuts[key] = '';
  save();
}

/**
 * Complete a capture with the given code and display label.
 *
 * @param code - The physical key/button identifier (e.g. "Space", "Mouse3").
 * @param label - The display label shown in the UI.
 */
function finishCapture(code: string, label: string) {
  const key = capturingKey.value;
  stopShortcutCapture();
  if (!key) {
    return;
  }

  keyDisplayLabels[code] = label;
  const shortcuts = settings.value.shortcuts ??= {};
  shortcuts[key] = code;
  save();
}

/**
 * Start capturing a keypress or mouse button for a shortcut action.
 *
 * Listens for both keyboard and mouse events. Mouse buttons 0-2 (left, right, middle) are
 * ignored because the game needs them. Buttons 3+ (side/extra) are accepted.
 *
 * @param key - The action identifier.
 */
function startCapture(key: string) {
  stopShortcutCapture();
  capturingKey.value = key;
  window.addEventListener('keydown', onCaptureKey);
  window.addEventListener('mousedown', onCaptureMouse);
}

/**
 * Handle a captured keypress. Stores `event.code` (physical key) and caches event.key (display label).
 *
 * @param event - The keyboard event.
 */
function onCaptureKey(event: KeyboardEvent) {
  event.preventDefault();
  event.stopPropagation();

  if (event.code === 'Escape') {
    stopShortcutCapture();
    return;
  }

  const label = event.code === 'Space' ? 'Space' : event.key;
  finishCapture(event.code, label);
}

/**
 * Handle a captured mouse button. Only accepts buttons 3+ (side/extra buttons on gaming mice).
 * Buttons 0-2 (left, right, middle) are ignored because the game needs them.
 *
 * @param event - The mouse event.
 */
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

// ---- Toast banner handlers --------------------------------------------------

/** Whether all banners are disabled (convenience toggle). */
const allBannersDisabled = computed(() => settings.value.banners.disable_all ?? false);

/** Set of currently disabled banner type names. */
const disabledBannerSet = computed(
  () => new Set(settings.value.banners.disabled_types ?? []),
);

/**
 * Toggle the "Disable All Banners" kill switch.
 *
 * @param event - Native change event from the checkbox.
 */
function onDisableAllBannersChange(event: Event) {
  const target = event.target as HTMLInputElement;
  settings.value.banners.disable_all = target.checked;
  save();
}

/**
 * Toggle an individual banner type.
 *
 * Checked means "show this banner", unchecked means "suppress it".
 *
 * @param name - The ToastState variant name.
 * @param checked - Whether the banner should be shown.
 */
function onBannerTypeToggle(name: string, checked: boolean) {
  const current = new Set(settings.value.banners.disabled_types ?? []);
  if (checked) {
    current.delete(name);
  } else {
    current.add(name);
  }
  settings.value.banners.disabled_types = [...current].sort();
  save();
}
</script>

<template>
  <div class="settings">
    <header class="settings-header">
      <h2>Settings</h2>
      <button class="close-btn" title="Close" @click="emit('close')">
        ✕
      </button>
    </header>

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
            @change="onAutoOpenSidebarChange">
      </div>

      <div class="setting-row">
        <label for="auto-expand-job-queue">Auto-expand Job Queue</label>
        <input id="auto-expand-job-queue"
            type="checkbox"
            :checked="settings.ui.auto_expand_job_queue ?? false"
            @change="onAutoExpandJobQueueChange">
      </div>

      <div class="setting-row">
        <label for="skip-reveal-sequence">Skip Loot Box Animation</label>
        <input id="skip-reveal-sequence"
            type="checkbox"
            :checked="settings.ui.skip_reveal_sequence ?? true"
            @change="onSkipRevealSequenceChange">
      </div>

      <div class="setting-row">
        <label for="skip-first-popup">Skip First Popup</label>
        <input id="skip-first-popup"
            type="checkbox"
            :checked="settings.ui.skip_first_popup ?? true"
            @change="onSkipFirstPopupChange">
      </div>
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
  </div>
</template>

<style scoped>
.settings {
  padding: 0 0.5rem;
}

.settings-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 1rem;
}

.settings-header h2 {
  margin: 0;
}

.close-btn {
  background: none;
  border: none;
  font-size: 1.4rem;
  cursor: pointer;
  padding: 0.25rem 0.5rem;
  line-height: 1;
  color: inherit;
  opacity: 0.6;
}

.close-btn:hover {
  opacity: 1;
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

.scale-value {
  min-width: 3.5rem;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.shortcut-key {
  font-family: monospace;
  padding: 0.2rem 0.5rem;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 0.25rem;
  min-width: 4rem;
  text-align: center;
  cursor: pointer;
}

.shortcut-key:hover {
  border-color: rgba(255, 255, 255, 0.4);
}

.shortcut-key.disabled {
  opacity: 0.4;
  font-style: italic;
}

.shortcut-key.capturing {
  border-color: rgba(100, 180, 255, 0.6);
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
