<script setup lang="ts">
import {useI18n} from '@app/i18n';
import globalDefaults from '@app/locales/en/global.json';
import {onBeforeUnmount, onMounted, ref} from 'vue';

const props = defineProps<{
  /** Accessible title shown at the top of the dialogue. */
  title: string;
}>();

const emit = defineEmits<{
  close: [];
}>();

const {t} = useI18n('global', globalDefaults);

const dialog = ref<HTMLDialogElement | null>(null);
let previouslyFocused: HTMLElement | null = null;

/** Keep native Escape handling under Vue's control so the parent owns the dialogue lifecycle. */
function handleCancel(event: Event): void {
  event.preventDefault();
  emit('close');
}

onMounted(() => {
  previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  dialog.value?.showModal();
  (dialog.value?.querySelector<HTMLElement>('[autofocus]') ?? dialog.value)?.focus();
});

onBeforeUnmount(() => {
  if (dialog.value?.open) {
    dialog.value.close();
  }
  previouslyFocused?.focus();
});
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog"
        class="dialog-shell"
        :aria-label="props.title"
        tabindex="-1"
        @cancel="handleCancel"
        @click.self="emit('close')">
      <section class="dialog">
        <header class="dialog-header">
          <h2>{{ props.title }}</h2>
          <button class="dialog-close"
              :title="t('close')"
              :aria-label="t('close')"
              @click="emit('close')">
            ✕
          </button>
        </header>
        <div class="dialog-content">
          <slot />
        </div>
      </section>
    </dialog>
  </Teleport>
</template>

<style scoped>
.dialog-shell {
  width: min(44rem, calc(100% - 3rem));
  max-width: none;
  max-height: none;
  padding: 0;
  border: 0;
  overflow: visible;
  background: transparent;
  color: inherit;
}

.dialog-shell::backdrop {
  background: var(--surface-overlay);
}

.dialog {
  width: 100%;
  max-height: calc(100vh - 3rem);
  overflow: hidden;
  border: 1px solid var(--border-strong);
  border-radius: 0.5rem;
  background: var(--surface-canvas);
  color: var(--text-primary);
  box-shadow: var(--dialog-shadow);
}

.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1rem 1.25rem;
  border-bottom: 1px solid var(--border-soft);
}

.dialog-header h2 {
  margin: 0;
}

.dialog-close {
  padding: 0.25rem 0.5rem;
  border: 0;
  background: none;
  color: inherit;
  font-size: 1.25rem;
  cursor: pointer;
  opacity: 0.65;
}

.dialog-close:hover {
  opacity: 1;
}

.dialog-content {
  max-height: calc(100vh - 8rem);
  padding: 1.25rem;
  overflow-y: auto;
}
</style>
