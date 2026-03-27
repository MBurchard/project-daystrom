<script setup lang="ts">
import {useSettings} from '@app/composables/useSettings';
import {computed} from 'vue';

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

/** Effective UI scale, defaulting to 100% when not set. */
const effectiveScale = computed(() => settings.value.ui.scale ?? 100);
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
        <label for="auto-open-sidebar">Auto-open Chat Sidebar</label>
        <input id="auto-open-sidebar"
            type="checkbox"
            :checked="settings.ui.auto_open_sidebar ?? false"
            @change="onAutoOpenSidebarChange">
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
  min-width: 5rem;
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
</style>
