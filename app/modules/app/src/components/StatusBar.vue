<script setup lang="ts">
import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {GameStatus} from '@generated/GameStatus';

const props = defineProps<{
  /** Backend-owned game and mod status. */
  status: GameStatus;
  /** Whether the first game-status snapshot is still loading. */
  loading: boolean;
  /** Fatal error from initial game-status loading. */
  error: string | null;
  /** Error from the most recent game action. */
  actionError: string | null;
  /** Whether a game or mod action is running. */
  actionPending: boolean;
  /** Backend-owned Daystrom update status. */
  update: DaystromUpdateStatus;
  /** Backend-owned Daystrom rollback status. */
  rollback: DaystromRollbackStatus;
}>();

const emit = defineEmits<{
  checkUpdate: [];
  openUpdate: [];
  openRollback: [];
  installMod: [];
  removeMod: [];
  openGameUpdater: [];
}>();

/** Whether update or rollback work currently blocks another update check. */
function maintenanceBusy(): boolean {
  return ['checking', 'confirming', 'retaining_rollback', 'downloading', 'installing'].includes(props.update.phase) ||
    ['preparing', 'installing'].includes(props.rollback.phase);
}
</script>

<template>
  <section class="status-bar" aria-label="Status">
    <span v-if="props.loading" class="status-item neutral">Detecting STFC…</span>
    <span v-else-if="props.error" class="status-item fail">Game status unavailable</span>
    <span v-else-if="props.status.installed" class="status-item ok">
      STFC<span v-if="props.status.game_version"> v{{ props.status.game_version }}</span>
    </span>
    <span v-else class="status-item fail">STFC not installed</span>

    <button v-if="props.status.installed && props.status.update_available"
        class="status-item warn interactive"
        :disabled="!props.status.can_launch_updater || props.actionPending"
        @click="emit('openGameUpdater')">
      STFC v{{ props.status.remote_version }} available
    </button>
    <span v-else-if="props.status.installed && props.status.update_check_failed" class="status-item warn">
      STFC update check failed
    </span>

    <span v-if="props.status.installed && props.status.mod_deployed" class="status-item ok">Mod ready</span>
    <span v-else-if="props.status.installed && !props.status.mod_available" class="status-item fail">
      Mod unavailable
    </span>
    <button v-if="props.status.installed && props.status.mod_available"
        :class="props.status.mod_deployed ? 'status-action' : 'status-item warn interactive'"
        :disabled="!props.status.can_install_mod || props.actionPending"
        @click="emit('installMod')">
      {{ props.status.mod_deployed ? 'Reinstall mod' : props.status.mod_outdated ? 'Update mod' : 'Install mod' }}
    </button>
    <button v-if="props.status.mod_removable"
        class="status-action"
        :disabled="!props.status.can_remove_mod || props.actionPending"
        @click="emit('removeMod')">
      Remove mod
    </button>

    <span v-if="props.status.game_running" class="status-item ok">Game running</span>
    <span v-else-if="props.status.installed" class="status-item neutral">Game not running</span>

    <span v-if="props.status.launcher_running" class="status-item warn">Scopely Launcher running</span>

    <button v-if="props.update.version && !props.update.dismissed"
        class="status-item warn interactive"
        @click="emit('openUpdate')">
      Daystrom {{ props.update.version }} available
    </button>
    <span v-else-if="props.update.phase === 'checking'" class="status-item neutral">
      Checking Daystrom updates…
    </span>
    <span v-else-if="props.update.error" class="status-item fail" :title="props.update.error">
      Daystrom update check failed
    </span>

    <button v-if="props.rollback.mod_restore_pending"
        class="status-item warn interactive"
        @click="emit('openRollback')">
      Mod restore pending
    </button>

    <button class="check-button" :disabled="maintenanceBusy()" @click="emit('checkUpdate')">
      Check Daystrom
    </button>
  </section>

  <p v-if="props.actionError" class="message error">
    {{ props.actionError }}
  </p>
  <p v-if="props.status.launcher_started_by_us" class="message info">
    The Scopely Launcher has been started. Update the game there, then close the launcher. Do not start the game from
    the Scopely Launcher; use Daystrom instead.
  </p>
  <p v-else-if="props.status.launcher_running" class="message info">
    Close the Scopely Launcher to continue. Do not start the game from there; use Daystrom instead.
  </p>
</template>

<style scoped>
.status-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
  padding: 0.65rem;
  border: 1px solid rgb(127 127 127 / 30%);
  border-radius: 0.35rem;
}

.status-item {
  padding: 0.25rem 0.5rem;
  border: 1px solid currentcolor;
  border-radius: 999px;
  background: none;
  color: inherit;
  font-size: 0.85rem;
}

.status-item::before {
  margin-right: 0.3rem;
}

.status-item.ok {
  color: #4caf50;
}

.status-item.ok::before {
  content: "✓";
}

.status-item.warn {
  color: #ff9800;
}

.status-item.warn::before {
  content: "!";
}

.status-item.fail {
  color: #f44336;
}

.status-item.fail::before {
  content: "✕";
}

.status-item.neutral {
  color: #777;
}

.interactive {
  cursor: pointer;
}

.interactive:disabled {
  cursor: default;
  opacity: 0.5;
}

.check-button {
  margin-left: auto;
}

.status-action {
  font-size: 0.8rem;
}

.message {
  margin: 0.65rem 0 0;
  user-select: text;
}

.error {
  color: #f44336;
}

.info {
  color: #2196f3;
}
</style>
