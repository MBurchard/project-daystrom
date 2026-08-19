<script setup lang="ts">
import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';

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
</script>

<template>
  <div class="rollback-dialog">
    <template v-if="props.status.mod_restore_pending">
      <p v-if="props.gameRunning">
        Close STFC when convenient. Daystrom will finish restoring the previous mod automatically, and it will take
        effect the next time you start the game.
      </p>
      <p v-else>
        Daystrom is finishing the restored mod for the next game start.
      </p>
    </template>

    <template v-else-if="props.status.version">
      <p>
        Use recovery only if the current Daystrom release causes problems. This restores Daystrom and its bundled mod
        to the previous verified release, {{ props.status.version }}.
      </p>
      <p>
        STFC stays open. A mod already loaded by the running game changes only after the game is closed and started
        again.
      </p>
      <p v-if="props.status.phase === 'preparing'">
        Verifying rollback package and settings…
      </p>
      <p v-else-if="props.status.phase === 'installing'">
        Restoring the previous release and restarting Daystrom…
      </p>
      <button v-if="props.status.phase === 'available' || props.status.phase === 'failed'"
          :disabled="!props.status.can_restore || props.updateBusy"
          @click="emit('restore')">
        Restore Daystrom {{ props.status.version }}
      </button>
    </template>

    <p v-else>
      No verified previous Daystrom release is available.
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
