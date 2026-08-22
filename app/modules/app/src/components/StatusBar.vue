<script setup lang="ts">
import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {GameStatus} from '@generated/GameStatus';
import type {UiErrorCode} from '@generated/UiErrorCode';
import {useUiError} from '@app/composables/useUiError';
import {useI18n} from '@app/i18n';
import statusDefaults from '@app/locales/en/status.json';

const props = defineProps<{
  /** Backend-owned game and mod status. */
  status: GameStatus;
  /** Whether the first game-status snapshot is still loading. */
  loading: boolean;
  /** Fatal error from initial game-status loading. */
  error: string | null;
  /** Error from the most recent game action. */
  actionError: UiErrorCode | null;
  /** Whether a game or mod action is running. */
  actionPending: boolean;
  /** Whether Daystrom update maintenance currently blocks a manual check. */
  updateCheckBusy: boolean;
  /** Backend-owned Daystrom update status. */
  update: DaystromUpdateStatus;
  /** Backend-owned Daystrom rollback status. */
  rollback: DaystromRollbackStatus;
  /** Whether a running game has no active Daystrom mod connection. */
  modConnectionMissing: boolean;
  /** Whether a Daystrom-tracked game is completing its first mod handshake. */
  trackedGameStarting: boolean;
  /** Whether any Daystrom-tracked game process is running. */
  trackedGameRunning: boolean;
  /** Whether any tracked game is running outside its initial handshake grace period. */
  trackedGameEstablished: boolean;
}>();

const emit = defineEmits<{
  openUpdate: [];
  openRollback: [];
  checkUpdate: [];
  installMod: [];
  removeMod: [];
  openGameUpdater: [];
  openModWarning: [];
}>();

const {t} = useI18n('status', statusDefaults);
const {errorText} = useUiError();
</script>

<template>
  <section class="status-bar" :aria-label="t('label')">
    <span v-if="props.loading" class="status-item neutral">{{ t('detecting') }}</span>
    <span v-else-if="props.error" class="status-item fail">{{ t('unavailable') }}</span>
    <span v-else-if="props.status.installed" class="status-item ok">
      STFC<span v-if="props.status.game_version"> v{{ props.status.game_version }}</span>
    </span>
    <span v-else class="status-item fail">{{ t('notInstalled') }}</span>

    <button v-if="props.update.version && !props.update.dismissed"
        class="status-item warn interactive segmented-status segmented-action"
        @click="emit('openUpdate')">
      <span class="segmented-status-label">
        {{ t('daystromAvailable', { version: props.update.version }) }}
      </span>
      <span class="status-segment" aria-hidden="true">›</span>
    </button>
    <span v-else-if="props.update.phase === 'checking'" class="status-item neutral">
      {{ t('checkingDaystrom') }}
    </span>
    <div v-else-if="props.update.error" class="status-item fail segmented-status">
      <span class="segmented-status-label" :title="errorText(props.update.error)">
        {{ t('daystromCheckFailed') }}
      </span>
      <button class="status-segment has-tooltip"
          :data-tooltip="t('checkDaystromUpdates')"
          :aria-label="t('checkDaystromUpdates')"
          :disabled="props.updateCheckBusy"
          @click="emit('checkUpdate')">
        ↻
      </button>
    </div>
    <div v-else-if="props.update.phase === 'up_to_date' || props.update.dismissed"
        class="status-item segmented-status"
        :class="props.update.dismissed ? 'neutral' : 'ok'">
      <span class="segmented-status-label">
        {{ props.update.dismissed ? t('daystromDeferred') : t('daystromCurrent') }}
      </span>
      <button class="status-segment has-tooltip"
          :data-tooltip="t('checkDaystromUpdates')"
          :aria-label="t('checkDaystromUpdates')"
          :disabled="props.updateCheckBusy"
          @click="emit('checkUpdate')">
        ↻
      </button>
    </div>

    <button v-if="props.status.installed && props.status.update_available"
        class="status-item warn interactive segmented-status segmented-action"
        :disabled="!props.status.can_launch_updater || props.actionPending"
        @click="emit('openGameUpdater')">
      <span class="segmented-status-label">
        {{ t('gameUpdateAvailable', { version: props.status.remote_version ?? '' }) }}
      </span>
      <span class="status-segment" aria-hidden="true">›</span>
    </button>
    <span v-else-if="props.status.installed && props.status.update_check_failed" class="status-item warn">
      {{ t('gameUpdateFailed') }}
    </span>

    <button v-if="props.status.installed && props.status.mod_deployed && props.modConnectionMissing"
        class="status-item warn interactive segmented-status segmented-action"
        @click="emit('openModWarning')">
      <span class="segmented-status-label">{{ t('modNotActive') }}</span>
      <span class="status-segment" aria-hidden="true">›</span>
    </button>
    <div v-else-if="props.status.installed && props.status.mod_deployed"
        class="status-item ok segmented-status">
      <span class="segmented-status-label">{{ t('modReady') }}</span>
      <button v-if="props.status.mod_available"
          class="status-segment has-tooltip mod-reinstall"
          :data-tooltip="t('reinstallMod')"
          :aria-label="t('reinstallMod')"
          :disabled="!props.status.can_install_mod || props.actionPending"
          @click="emit('installMod')">
        ↻
      </button>
    </div>
    <span v-else-if="props.status.installed && !props.status.mod_available" class="status-item fail">
      {{ t('modUnavailable') }}
    </span>
    <button v-if="props.status.installed && props.status.mod_available && !props.status.mod_deployed"
        class="status-item warn interactive segmented-status segmented-action"
        :disabled="!props.status.can_install_mod || props.actionPending"
        @click="emit('installMod')">
      <span class="segmented-status-label">
        {{ props.status.mod_outdated ? t('updateMod') : t('installMod') }}
      </span>
      <span class="status-segment" aria-hidden="true">›</span>
    </button>
    <button v-if="props.status.mod_removable"
        class="status-item neutral interactive segmented-status segmented-action"
        :disabled="!props.status.can_remove_mod || props.actionPending"
        @click="emit('removeMod')">
      <span class="segmented-status-label">{{ t('removeMod') }}</span>
      <span class="status-segment" aria-hidden="true">✗</span>
    </button>

    <span v-if="props.trackedGameEstablished || (props.status.game_running && !props.trackedGameRunning)"
        class="status-item ok">
      {{ t('gameRunning') }}
    </span>
    <span v-else-if="props.trackedGameStarting" class="status-item warn">{{ t('gameStarting') }}</span>
    <span v-else-if="props.status.installed" class="status-item neutral">
      {{ t('gameNotRunning') }}
    </span>

    <span v-if="props.status.launcher_running" class="status-item warn">{{ t('launcherRunning') }}</span>

    <button v-if="props.rollback.mod_restore_pending"
        class="status-item warn interactive segmented-status segmented-action"
        @click="emit('openRollback')">
      <span class="segmented-status-label">{{ t('modRestorePending') }}</span>
      <span class="status-segment" aria-hidden="true">›</span>
    </button>
  </section>

  <p v-if="props.actionError" class="message error">
    {{ errorText(props.actionError) }}
  </p>
  <p v-if="props.status.launcher_started_by_us" class="message info">
    {{ t('launcherStarted') }}
  </p>
  <p v-else-if="props.status.launcher_running" class="message info">
    {{ t('closeLauncher') }}
  </p>
</template>

<style scoped>
.status-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
  padding: 0.65rem;
  border: 1px solid var(--border-soft);
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
  color: var(--status-success);
}

.status-item.ok::before {
  content: "✓";
}

.segmented-status {
  display: flex;
  align-items: stretch;
  padding: 0;
}

.segmented-status::before {
  align-self: center;
  margin-left: 0.5rem;
}

.segmented-status-label {
  align-self: center;
  padding: 0.25rem 0.45rem 0.25rem 0;
}

.segmented-status.neutral .segmented-status-label {
  padding-left: 0.5rem;
}

.status-segment {
  display: grid;
  box-sizing: border-box;
  min-width: 1.9rem;
  padding: 0 0.45rem;
  border: 0;
  border-left: 1px solid currentcolor;
  background: color-mix(in srgb, currentcolor 8%, transparent);
  color: inherit;
  font: inherit;
  font-size: 1rem;
  place-items: center;
}

/* The pill cannot clip its children without cutting off tooltips, so the trailing segment rounds itself. */
.status-segment:last-child {
  border-radius: 0 999px 999px 0;
}

button.status-segment:disabled {
  opacity: 0.45;
}

.segmented-action:hover:not(:disabled) .status-segment,
button.status-segment:hover:not(:disabled) {
  background: color-mix(in srgb, currentcolor 22%, transparent);
}

.has-tooltip {
  position: relative;
}

.has-tooltip::after {
  position: absolute;
  z-index: 1;
  top: calc(100% + 0.45rem);
  right: 0;
  padding: 0.3rem 0.5rem;
  border-radius: 0.3rem;
  background: var(--tooltip-surface);
  color: var(--tooltip-text);
  content: attr(data-tooltip);
  font-size: 0.75rem;
  line-height: 1.2;
  opacity: 0;
  pointer-events: none;
  transform: translateY(-0.2rem);
  transition: opacity 120ms ease, transform 120ms ease;
  white-space: nowrap;
}

.has-tooltip:focus-visible::after,
.has-tooltip:hover:not(:disabled)::after {
  opacity: 1;
  transform: translateY(0);
}

.status-item.warn {
  color: var(--status-warning);
}

.status-item.warn::before {
  content: "!";
}

.status-item.fail {
  color: var(--status-danger);
}

.status-item.fail::before {
  content: "✗";
}

.status-item.neutral {
  color: var(--text-subtle);
}

.status-bar button {
  cursor: pointer;
}

.status-bar button:disabled {
  cursor: default;
}

.interactive:disabled {
  opacity: 0.5;
}

.message {
  margin: 0.65rem 0 0;
  user-select: text;
}

.error {
  color: var(--status-danger);
}

.info {
  color: var(--status-info);
}
</style>
