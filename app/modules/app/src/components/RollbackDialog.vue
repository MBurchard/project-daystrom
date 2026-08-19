<script setup lang="ts">
import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import {useI18n} from '@app/i18n';
import rollbackDefaults from '@app/locales/en/rollback.json';

const props = defineProps<{
  /** Backend-owned rollback status. */
  status: DaystromRollbackStatus;
  /** Whether the game is currently running. */
  gameRunning: boolean;
  /** Whether update work currently blocks restoration. */
  updateBusy: boolean;
}>();

const emit = defineEmits<{
  restore: [];
}>();

const {t} = useI18n('rollback', rollbackDefaults);
</script>

<template>
  <div class="rollback-dialog">
    <template v-if="props.status.mod_restore_pending">
      <p v-if="props.gameRunning">
        {{ t('closeGame') }}
      </p>
      <p v-else>
        {{ t('finishingMod') }}
      </p>
    </template>

    <template v-else-if="props.status.version">
      <p>
        {{ t('description', { version: props.status.version }) }}
      </p>
      <p>
        {{ t('runningGame') }}
      </p>
      <p v-if="props.status.phase === 'preparing'">
        {{ t('verifying') }}
      </p>
      <p v-else-if="props.status.phase === 'installing'">
        {{ t('installing') }}
      </p>
      <button v-if="props.status.phase === 'available' || props.status.phase === 'failed'"
          :disabled="!props.status.can_restore || props.updateBusy"
          @click="emit('restore')">
        {{ t('restore', { version: props.status.version }) }}
      </button>
    </template>

    <p v-else>
      {{ t('none') }}
    </p>
    <p v-if="props.status.error" class="error">
      {{ props.status.error }}
    </p>
  </div>
</template>

<style scoped>
.rollback-dialog > :first-child {
  margin-top: 0;
}

.error {
  color: #f44336;
  user-select: text;
}
</style>
