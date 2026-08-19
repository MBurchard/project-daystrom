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
  border: 1px solid rgb(92 221 255 / 65%);
  border-radius: 0.75rem;
  background: rgb(0 18 42 / 80%);
  box-shadow: 0 0 1rem rgb(0 183 255 / 45%);
  color: #fff;
  font-size: 1.75rem;
  font-weight: 600;
  pointer-events: none;
  text-align: center;
  text-shadow: 0 0 0.5rem rgb(92 221 255 / 80%);
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
