<script setup lang="ts">
import type {SafetyNoticeContext} from '@generated/SafetyNoticeContext';
import {useI18n} from '@app/i18n';
import safetyDefaults from '@app/locales/en/safety.json';
import {ref} from 'vue';

defineProps<{
  /** Whether the backend is currently persisting acknowledgement. */
  pending: boolean;
  /** Whether the latest acknowledgement attempt failed. */
  failed: boolean;
  /** Platform and absolute removal paths supplied by the backend. */
  context: SafetyNoticeContext | null;
  /** Whether the user must explicitly acknowledge this notice revision. */
  acknowledgementRequired: boolean;
}>();

const emit = defineEmits<{
  acknowledge: [];
}>();

const {t} = useI18n('safety', safetyDefaults);
const understood = ref(false);
</script>

<template>
  <div class="safety-notice">
    <p>
      {{ t('independence') }}
    </p>

    <p>
      {{ t('operationAndRisk') }}
    </p>

    <p class="account-protection">
      {{ t('accountProtection') }}
    </p>

    <section v-if="context?.platform === 'windows'">
      <p>
        {{ t('windowsRemoval') }}
      </p>
      <p v-if="context.modLibraryPath">
        {{ t('windowsManualRemoval') }}
      </p>
      <ul v-if="context.modLibraryPath" class="path-list">
        <li>
          <code>{{ context.modLibraryPath }}</code>
        </li>
      </ul>
      <p v-if="context.cleanupPaths.length > 0">
        {{ t('cleanup') }}
      </p>
      <ul class="path-list">
        <li v-for="path in context.cleanupPaths" :key="path">
          <code>{{ path }}</code>
        </li>
      </ul>
    </section>

    <section v-else-if="context?.platform === 'macos'">
      <p>
        {{ t('macosRemoval') }}
      </p>
      <p v-if="context.cleanupPaths.length > 0">
        {{ t('cleanup') }}
      </p>
      <ul class="path-list">
        <li v-for="path in context.cleanupPaths" :key="path">
          <code>{{ path }}</code>
        </li>
      </ul>
    </section>

    <p v-if="failed" class="safety-error" role="alert">
      {{ t('acknowledgementFailed') }}
    </p>

    <footer v-if="acknowledgementRequired" class="safety-actions">
      <label>
        <input v-model="understood" type="checkbox" autofocus :disabled="pending">
        <span>{{ t('understood') }}</span>
      </label>
      <button class="continue-button"
          :disabled="!understood || pending"
          @click="emit('acknowledge')">
        {{ pending ? t('saving') : t('continue') }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
.safety-notice {
  display: grid;
  gap: 1rem;
}

.safety-notice p,
.safety-notice ul {
  margin: 0;
}

.safety-notice section {
  display: grid;
  gap: 0.35rem;
}

.path-list {
  display: grid;
  gap: 0.25rem;
  padding-left: 1.4rem;
}

.path-list code {
  overflow-wrap: anywhere;
}

.account-protection {
  padding: 0.85rem 1rem;
  border: 1px solid var(--warning-border);
  border-radius: 0.4rem;
  background: var(--warning-surface);
}

.safety-error {
  color: var(--status-danger);
}

.safety-actions {
  position: sticky;
  bottom: -1.25rem;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  margin: 0 -1.25rem -1.25rem;
  padding: 1rem 1.25rem;
  border-top: 1px solid var(--border-soft);
  background: var(--surface-canvas);
}

.safety-actions label {
  display: flex;
  align-items: center;
  gap: 0.55rem;
}

.continue-button {
  flex: 0 0 auto;
}
</style>
