<script setup lang="ts">
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import {useUiError} from '@app/composables/useUiError';
import {useI18n} from '@app/i18n';
import globalDefaults from '@app/locales/en/global.json';
import updateDefaults from '@app/locales/en/update.json';
import {computed} from 'vue';
import {releaseNotesForLanguage} from './updateDialog';

const props = defineProps<{
  /** Backend-owned update state rendered without frontend policy decisions. */
  status: DaystromUpdateStatus;
  /** Whether rollback work currently blocks installation. */
  rollbackBusy: boolean;
}>();

const emit = defineEmits<{
  install: [];
  later: [];
}>();

const {language, t} = useI18n('update', {...updateDefaults, later: globalDefaults.later});
const {errorText} = useUiError();
const releaseNotes = computed(() => releaseNotesForLanguage(props.status.notes, language.value));
</script>

<template>
  <div class="update-dialog">
    <section v-if="releaseNotes" class="release-notes">
      <h3>{{ t('releaseNotes') }}</h3>
      <p>{{ releaseNotes }}</p>
    </section>
    <p>
      {{ t('restart') }}
    </p>
    <p v-if="props.status.phase === 'confirming'" class="progress-row">
      {{ t('confirming') }}
    </p>
    <p v-else-if="props.status.phase === 'retaining_rollback'" class="progress-row">
      {{ t('retaining') }}
      <progress v-if="props.status.download_progress !== null"
          :value="props.status.download_progress"
          max="100" />
      <span v-if="props.status.download_progress !== null">{{ props.status.download_progress }}%</span>
    </p>
    <p v-else-if="props.status.phase === 'downloading'" class="progress-row">
      {{ t('downloading') }}
      <progress v-if="props.status.download_progress !== null"
          :value="props.status.download_progress"
          max="100" />
      <span v-if="props.status.download_progress !== null">{{ props.status.download_progress }}%</span>
    </p>
    <p v-else-if="props.status.phase === 'installing'" class="progress-row">
      {{ t('installing') }}
    </p>
    <p v-else-if="props.status.phase === 'available' && !props.status.can_install" class="info">
      {{ t('devDisabled') }}
    </p>
    <p v-if="props.status.error" class="error">
      {{ errorText(props.status.error) }}
    </p>
    <div v-if="props.status.phase === 'available'" class="actions">
      <button :disabled="!props.status.can_install || props.rollbackBusy" @click="emit('install')">
        {{ t('install') }}
      </button>
      <button @click="emit('later')">
        {{ t('later') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.update-dialog > :first-child {
  margin-top: 0;
}

.release-notes {
  overflow-wrap: anywhere;
  user-select: text;
}

.release-notes h3 {
  margin: 0 0 0.5rem;
  font-size: 1rem;
}

.release-notes p {
  max-height: 12rem;
  margin: 0;
  overflow-y: auto;
  white-space: pre-wrap;
}

.progress-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.progress-row progress {
  width: 10rem;
}

.actions {
  display: flex;
  gap: 0.5rem;
}

.error {
  color: var(--status-danger);
  user-select: text;
}

.info {
  color: var(--status-info);
  user-select: text;
}
</style>
