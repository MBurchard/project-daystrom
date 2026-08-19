<script setup lang="ts">
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import {useI18n} from '@app/i18n';
import globalDefaults from '@app/locales/en/global.json';
import updateDefaults from '@app/locales/en/update.json';

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

const {t} = useI18n('update', {...updateDefaults, later: globalDefaults.later});
</script>

<template>
  <div class="update-dialog">
    <p v-if="props.status.notes" class="release-notes">
      {{ props.status.notes }}
    </p>
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
      {{ props.status.error }}
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
  max-height: 12rem;
  overflow-wrap: anywhere;
  overflow-y: auto;
  white-space: pre-wrap;
  user-select: text;
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
  color: #f44336;
  user-select: text;
}

.info {
  color: #2196f3;
  user-select: text;
}
</style>
