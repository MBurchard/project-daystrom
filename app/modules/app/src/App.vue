<script setup lang="ts">
import {useGameState} from '@app/composables/useGameState';
import {onMounted, onUnmounted} from 'vue';

const {
  version,
  status,
  loading,
  installed,
  error,
  actionError,
  actionPending,
  remoteVersion,
  updateCheckFailed,
  launcherRunning,
  gameRunning,
  updaterStartedByUs,
  updateAvailable,
  canLaunch,
  versionCheckClass,
  canInstallMod,
  canRemoveMod,
  canLaunchUpdater,
  installMod,
  removeMod,
  openUpdater,
  launchGame,
  init,
  destroy,
} = useGameState();

onMounted(() => init());
onUnmounted(() => destroy());
</script>

<template>
  <main>
    <h1>Project Daystrom <small v-if="version">{{ version }}</small></h1>

    <p v-if="error">
      Failed to load game status: {{ error }}
    </p>

    <section v-else>
      <h2>Status</h2>

      <ul class="checklist">
        <li v-if="loading" class="neutral">
          Detecting STFC...
        </li>
        <li v-else :class="installed ? 'ok' : 'fail'">
          STFC installed
          <template v-if="installed && status.game_version">
            (v{{ status.game_version }})
          </template>
        </li>

        <li v-if="installed" :class="versionCheckClass">
          <template v-if="updateAvailable">
            v{{ remoteVersion }} available
            <button :disabled="!canLaunchUpdater || actionPending" @click="openUpdater">
              Update
            </button>
          </template>
          <template v-else-if="updateCheckFailed">
            Version check not available
          </template>
          <template v-else-if="remoteVersion != null">
            Version check: up to date
          </template>
          <template v-else>
            Checking for updates...
          </template>
        </li>

        <li v-if="launcherRunning" class="warn">
          Scopely Launcher running
        </li>

        <li v-if="installed" :class="status.mod_deployed ? 'ok' : status.mod_available ? 'warn' : 'fail'">
          Community Mod
          <button v-if="status.mod_available" :disabled="!canInstallMod || actionPending" @click="installMod">
            {{ status.mod_deployed ? 'Reinstall' : status.mod_outdated ? 'Update' : 'Install' }}
          </button>
          <!-- eslint-disable-next-line style/max-len -->
          <button v-if="status.mod_deployed || status.mod_outdated" :disabled="!canRemoveMod || actionPending"
              @click="removeMod">
            Remove
          </button>
        </li>

        <li v-if="installed" :class="gameRunning ? 'ok' : 'fail'">
          Game running
        </li>
      </ul>

      <button v-if="installed" :disabled="!canLaunch || actionPending" class="launch-btn" @click="launchGame">
        Launch Game
      </button>

      <p v-if="actionError" class="error">
        {{ actionError }}
      </p>

      <p v-if="updaterStartedByUs" class="info-message">
        The Scopely Launcher has been started. Update the game there, then close the launcher.
        Do not start the game from the Scopely Launcher. Use Daystrom instead.
      </p>

      <p v-else-if="launcherRunning" class="info-message">
        Close the Scopely Launcher to continue. Do not start the game from there, use Daystrom instead.
      </p>
    </section>
  </main>
</template>

<style>
body {
  font-family: system-ui, -apple-system, sans-serif;
  user-select: none;
}

.error,
.info-message {
  user-select: text;
}
</style>

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

.checklist li.warn::before {
  content: '!';
  color: #ff9800;
}

.checklist li.neutral::before {
  content: '';
  width: 0.85rem;
  height: 0.85rem;
  margin-right: 0.6rem;
  vertical-align: middle;
  position: relative;
  top: -2px;
  border-radius: 50%;
  border: 2px solid #1a8acf;
  background: conic-gradient(from 0deg, transparent 240deg, #1a8acf 360deg);
  animation: radar-sweep 1.2s linear infinite;
}

@keyframes radar-sweep {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.checklist button {
  margin-left: 0.5rem;
  font-size: 0.85rem;
  position: relative;
  top: -2px;
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

.info-message {
  color: #2196f3;
  margin-top: 0.5rem;
}

h1 small {
  font-size: 0.5em;
  font-weight: 400;
  color: #888;
  margin-left: 0.25em;
}
</style>
