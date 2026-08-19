<script setup lang="ts">
const props = defineProps<{
  /** Installed Daystrom version. */
  version: string;
}>();

const emit = defineEmits<{
  openSettings: [];
}>();
</script>

<template>
  <header class="app-header">
    <h1>
      Project Daystrom <small v-if="props.version">{{ props.version }}</small>
    </h1>
    <button class="settings-button" title="Settings" aria-label="Settings" @click="emit('openSettings')">
      ⚙
    </button>
  </header>
</template>

<style scoped>
.app-header {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

h1 {
  display: flex;
  align-items: baseline;
}

h1 small {
  margin-left: 0.25em;
  color: #888;
  font-size: 0.5em;
  font-weight: 400;
}

.settings-button {
  padding: 0.25rem 0.5rem;
  border: 0;
  background: none;
  color: inherit;
  font-size: 1.25rem;
  cursor: pointer;
  opacity: 0.5;
}

.settings-button:hover {
  opacity: 1;
}
</style>
