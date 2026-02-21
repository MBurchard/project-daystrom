<script setup lang="ts">
import type {GameStatus} from '@generated/GameStatus';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {computed, onMounted, onUnmounted, ref} from 'vue';

const log = getLogger('App');

const status = ref<GameStatus | null>(null);
const error = ref<string | null>(null);
const actionError = ref<string | null>(null);
const actionPending = ref(false);

let pollTimer: ReturnType<typeof setInterval> | null = null;

/**
 * Fetch the current game status from the backend.
 */
function refreshStatus(): void {
  invoke<GameStatus>('get_game_status')
    .then((result) => {
      status.value = result;
      log.debug('Game status received:', result.installed ? 'installed' : 'not found');
      if (!result.game_running && pollTimer) {
        stopPolling();
      }
    })
    .catch((err) => {
      error.value = String(err);
      log.error(`Failed to get game status: ${err}`);
    });
}

/**
 * Start polling game status every 5 seconds.
 */
function startPolling(): void {
  if (pollTimer) {
    return;
  }
  pollTimer = setInterval(refreshStatus, 5000);
}

/**
 * Stop polling game status.
 */
function stopPolling(): void {
  if (!pollTimer) {
    return;
  }
  clearInterval(pollTimer);
  pollTimer = null;
  log.debug('Game process ended — polling stopped');
}

/**
 * Whether all conditions for launching the game are met.
 * @returns true when installed, entitlements OK, mod available, and game not running
 */
const canLaunch = computed(() => {
  const s = status.value;
  return s?.installed && s.entitlements_ok && s.mod_available && !s.game_running;
});

/**
 * Whether the entitlements button should be enabled.
 * @returns true when game is installed and not running
 */
const canPatchEntitlements = computed(() => {
  const s = status.value;
  return s?.installed && !s.game_running;
});

/**
 * Patch the game executable's entitlements, then refresh status.
 */
function fixEntitlements(): void {
  actionPending.value = true;
  actionError.value = null;
  invoke('patch_entitlements')
    .then(() => {
      refreshStatus();
    })
    .catch((err) => {
      actionError.value = String(err);
      log.error(`Failed to patch entitlements: ${err}`);
    })
    .finally(() => {
      actionPending.value = false;
    });
}

/**
 * Launch the game with the mod injected, then refresh status.
 */
function launchGame(): void {
  actionPending.value = true;
  actionError.value = null;
  invoke('launch_game')
    .then(() => {
      // Small delay to let the process appear in pgrep, then start polling
      setTimeout(() => {
        refreshStatus();
        startPolling();
      }, 1000);
    })
    .catch((err) => {
      actionError.value = String(err);
      log.error(`Failed to launch game: ${err}`);
    })
    .finally(() => {
      actionPending.value = false;
    });
}

onMounted(() => {
  log.debug('App.vue mounted');
  refreshStatus();
});

onUnmounted(() => {
  stopPolling();
});
</script>

<template>
  <main>
    <h1>Project Daystrom</h1>

    <p v-if="error">
      Failed to load game status: {{ error }}
    </p>

    <section v-else-if="status">
      <h2>Status</h2>

      <ul class="checklist">
        <li :class="status.installed ? 'ok' : 'fail'">
          STFC installed
        </li>
        <li v-if="status.installed" :class="status.entitlements_ok ? 'ok' : 'fail'">
          Entitlements
          <button
            v-if="canPatchEntitlements"
            :disabled="actionPending"
            @click="fixEntitlements"
          >
            {{ status.entitlements_ok ? 'Re-apply' : 'Fix' }}
          </button>
        </li>
        <li v-if="status.installed" :class="status.mod_available ? 'ok' : 'fail'">
          Mod loaded
        </li>
        <li v-if="status.installed" :class="status.game_running ? 'ok' : 'fail'">
          Game running
        </li>
      </ul>

      <button
        v-if="status.installed"
        :disabled="!canLaunch || actionPending"
        class="launch-btn"
        @click="launchGame"
      >
        Launch Game
      </button>

      <p v-if="actionError" class="error">
        {{ actionError }}
      </p>
    </section>

    <p v-else>
      Loading...
    </p>
  </main>
</template>

<style scoped>
.checklist {
  list-style: none;
  padding: 0;
}

.checklist li {
  padding: 0.25rem 0;
}

.checklist li::before {
  display: inline-block;
  width: 1.5rem;
  font-weight: bold;
}

.checklist li.ok::before {
  content: '✓';
  color: #4caf50;
}

.checklist li.fail::before {
  content: '✗';
  color: #f44336;
}

.checklist button {
  margin-left: 0.5rem;
  font-size: 0.85rem;
}

.launch-btn {
  margin-top: 1rem;
  padding: 0.5rem 1.5rem;
  font-size: 1rem;
}

.error {
  color: #f44336;
  margin-top: 0.5rem;
}
</style>
