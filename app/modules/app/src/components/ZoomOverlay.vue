<script setup lang="ts">
import {useUiZoom} from '@app/composables/useUiZoom';

const {percent, visible} = useUiZoom();
</script>

<template>
  <Transition name="zoom-overlay">
    <output v-if="visible" class="zoom-overlay" aria-live="polite">
      {{ percent }}
    </output>
  </Transition>
</template>

<style scoped>
.zoom-overlay {
  position: fixed;
  z-index: 2000;
  top: 50%;
  left: 50%;
  min-width: 7rem;
  padding: 0.75rem 1.5rem;
  border: 1px solid var(--zoom-border);
  border-radius: 0.75rem;
  background: var(--zoom-surface);
  box-shadow: var(--zoom-shadow);
  color: var(--text-on-emphasis);
  font-size: 1.75rem;
  font-weight: 600;
  pointer-events: none;
  text-align: center;
  text-shadow: var(--zoom-text-shadow);
  transform: translate(-50%, -50%);
}

.zoom-overlay-enter-active {
  transition: opacity 150ms ease;
}

.zoom-overlay-leave-active {
  transition: opacity 500ms ease;
}

.zoom-overlay-enter-from,
.zoom-overlay-leave-to {
  opacity: 0;
}
</style>
