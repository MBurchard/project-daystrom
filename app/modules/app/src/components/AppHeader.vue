<script setup lang="ts">
import {useI18n} from '@app/i18n';
import shellDefaults from '@app/locales/en/shell.json';
import {ref} from 'vue';

const props = defineProps<{
  /** Installed Daystrom version. */
  version: string;
}>();
const emit = defineEmits<{
  openSettings: [];
  closeWindow: [];
}>();
const {t} = useI18n('shell', shellDefaults);
const closeHoverSuppressed = ref(false);

/** Suppress the close hover until the pointer leaves after hiding the window. */
function requestClose(): void {
  closeHoverSuppressed.value = true;
  emit('closeWindow');
}
</script>

<template>
  <header class="app-header" data-tauri-drag-region>
    <div class="app-drag-region" data-tauri-drag-region>
      <h1 data-tauri-drag-region>
        <span data-tauri-drag-region>Project Daystrom</span>
        <small v-if="props.version" data-tauri-drag-region>{{ props.version }}</small>
      </h1>
    </div>
    <div class="window-actions">
      <button class="settings-button"
          :title="t('settings')"
          :aria-label="t('settings')"
          @click="emit('openSettings')">
        ⚙
      </button>
      <button class="close-button"
          :class="{ 'hover-suppressed': closeHoverSuppressed }"
          :title="t('closeWindow')"
          :aria-label="t('closeWindow')"
          @click="requestClose"
          @pointerleave="closeHoverSuppressed = false">
        ✕
      </button>
    </div>
  </header>
</template>

<style scoped>
.app-header {
  display: flex;
  min-height: 3.25rem;
  align-items: stretch;
  border-bottom: 1px solid rgb(127 127 127 / 25%);
  background: rgb(127 127 127 / 8%);
}

.app-drag-region {
  display: flex;
  flex: 1;
  min-width: 0;
  align-items: center;
  padding-left: 0.85rem;
}

h1 {
  display: flex;
  align-items: baseline;
  margin: 0;
  font-size: 1.5rem;
}

h1 small {
  margin-left: 0.25em;
  color: #888;
  font-size: 0.5em;
  font-weight: 400;
}

.window-actions {
  display: flex;
}

.settings-button,
.close-button {
  width: 2rem;
  height: 2rem;
  align-self: center;
  margin: 0 0.625rem;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 50%;
  background: none;
  color: inherit;
  cursor: pointer;
}

.settings-button {
  font-size: 1.15rem;
  opacity: 0.5;
}

.close-button {
  font-size: 1rem;
}

.settings-button:focus-visible,
.close-button:focus-visible {
  outline: 2px solid #2196f3;
  outline-offset: -3px;
}

.settings-button:hover,
.close-button:hover:not(.hover-suppressed) {
  border-color: #5cddff;
  background: linear-gradient(180deg, #159fe8 0%, #0769c8 100%);
  box-shadow:
    0 0 0.55rem rgb(0 203 255 / 80%),
    inset 0 0 0.35rem rgb(184 246 255 / 65%);
  color: #fff;
  opacity: 1;
  text-shadow: 0 1px 1px rgb(0 29 75 / 70%);
}
</style>
