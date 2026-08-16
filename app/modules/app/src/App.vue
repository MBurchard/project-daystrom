<script setup lang="ts">
import SettingsView from '@app/components/SettingsView.vue';
import {useGameState} from '@app/composables/useGameState';
import {useProfileState} from '@app/composables/useProfileState';
import {useSettings} from '@app/composables/useSettings';
import {onMounted, onUnmounted, ref} from 'vue';

const showSettings = ref(false);

const {
  version,
  status,
  loading,
  error,
  actionError,
  actionPending,
  installMod,
  removeMod,
  openUpdater,
  launchGame,
  init: initGameState,
  destroy: destroyGameState,
} = useGameState();

const {
  profiles,
  hasProfiles,
  externalGameRunning,
  gameOriginPending,
  isProfileRunning,
  markLaunched,
  init: initProfileState,
  destroy: destroyProfileState,
} = useProfileState();

const {init: initSettings} = useSettings();

onMounted(() => {
  initGameState();
  initProfileState();
  initSettings();
});
onUnmounted(() => {
  destroyGameState();
  destroyProfileState();
});
</script>

<template>
  <main>
    <h1>
      Project Daystrom <small v-if="version">{{ version }}</small>
      <button class="settings-btn" title="Settings" @click="showSettings = !showSettings">
        ⚙
      </button>
    </h1>

    <SettingsView v-if="showSettings" @close="showSettings = false" />

    <template v-else>
      <p v-if="error">
        Failed to load game status: {{ error }}
      </p>

      <section v-else>
        <h2>Status</h2>

        <ul class="checklist">
          <li v-if="loading" class="neutral">
            Detecting STFC...
          </li>
          <li v-else :class="status.installed ? 'ok' : 'fail'">
            STFC installed
            <template v-if="status.installed && status.game_version">
              (v{{ status.game_version }})
            </template>
          </li>

          <li v-if="status.installed" :class="status.version_check_class">
            <template v-if="status.update_available">
              v{{ status.remote_version }} available
              <button :disabled="!status.can_launch_updater || actionPending" @click="openUpdater">
                Update
              </button>
            </template>
            <template v-else-if="status.update_check_failed">
              Version check failed
            </template>
            <template v-else-if="status.remote_version != null">
              Version check: up to date
            </template>
            <template v-else>
              Checking for updates...
            </template>
          </li>

          <li v-if="status.launcher_running" class="warn">
            Scopely Launcher running
          </li>

          <li v-if="status.installed" :class="status.mod_deployed ? 'ok' : status.mod_available ? 'warn' : 'fail'">
            Daystrom Mod
            <button v-if="status.mod_available"
                :disabled="!status.can_install_mod || actionPending"
                @click="installMod">
              {{ status.mod_deployed ? 'Reinstall' : status.mod_outdated ? 'Update' : 'Install' }}
            </button>
            <button v-if="status.mod_removable" :disabled="!status.can_remove_mod || actionPending" @click="removeMod">
              Remove
            </button>
          </li>

          <li v-if="status.installed" class="game-status">
            {{ status.game_running ? '🚀 Game is running' : '💤 Game is not running' }}
          </li>
        </ul>

        <button v-if="status.installed && !hasProfiles"
            :disabled="!status.can_launch || actionPending || externalGameRunning || gameOriginPending"
            class="launch-btn"
            @click="launchGame('initial')">
          Launch Game
        </button>

        <template v-if="status.installed && hasProfiles && !externalGameRunning && !gameOriginPending">
          <button v-for="p in profiles.profiles" :key="p.stem"
              :disabled="!status.mod_deployed || actionPending || isProfileRunning(p.stem)"
              class="launch-btn"
              @click="markLaunched(p.stem); launchGame(p.stem)">
            {{ p.name }} (Server {{ p.server }})
          </button>

          <button :disabled="!status.mod_deployed || actionPending"
              class="launch-btn add-account-btn"
              title="Add Account"
              @click="launchGame('new_account')">
            +
          </button>
        </template>

        <p v-if="gameOriginPending" class="info-message">
          Reconnecting to the running game…
        </p>

        <p v-else-if="externalGameRunning" class="info-message">
          The game was started externally. Close it to use Daystrom.
        </p>

        <p v-if="actionError" class="error">
          {{ actionError }}
        </p>

        <p v-if="status.launcher_started_by_us" class="info-message">
          The Scopely Launcher has been started. Update the game there, then close the launcher.
          Do not start the game from the Scopely Launcher. Use Daystrom instead.
        </p>

        <p v-else-if="status.launcher_running" class="info-message">
          Close the Scopely Launcher to continue. Do not start the game from there, use Daystrom instead.
        </p>
      </section>
    </template>
  </main>
</template>

<style>
body {
  font-family: system-ui, -apple-system, sans-serif;
}

*,
*::before,
*::after {
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
  content: "✓";
  color: #4caf50;
}

.checklist li.fail::before {
  content: "✗";
  color: #f44336;
}

.checklist li.warn::before {
  content: "!";
  color: #ff9800;
}

.checklist li.neutral::before {
  content: "";
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

.launch-btn + .launch-btn {
  margin-left: 6px;
}

.add-account-btn {
  padding: 0.5rem 0.75rem;
  font-size: 1.2rem;
  font-weight: bold;
  line-height: 1;
}

.error {
  color: #f44336;
  margin-top: 0.5rem;
}

.info-message {
  color: #2196f3;
  margin-top: 0.5rem;
}

h1 {
  display: flex;
  align-items: baseline;
}

h1 small {
  font-size: 0.5em;
  font-weight: 400;
  color: #888;
  margin-left: 0.25em;
}

.settings-btn {
  margin-left: auto;
  background: none;
  border: none;
  font-size: 1.25rem;
  cursor: pointer;
  padding: 0.25rem 0.5rem;
  color: inherit;
  opacity: 0.5;
}

.settings-btn:hover {
  opacity: 1;
}
</style>
