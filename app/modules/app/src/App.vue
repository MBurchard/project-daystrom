<script setup lang="ts">
import AccountTabs from '@app/components/AccountTabs.vue';
import AppDialog from '@app/components/AppDialog.vue';
import AppHeader from '@app/components/AppHeader.vue';
import NewAccountDialog from '@app/components/NewAccountDialog.vue';
import RollbackDialog from '@app/components/RollbackDialog.vue';
import SettingsView from '@app/components/SettingsView.vue';
import StatusBar from '@app/components/StatusBar.vue';
import UpdateDialog from '@app/components/UpdateDialog.vue';
import {useDaystromRollback} from '@app/composables/useDaystromRollback';
import {useDaystromUpdate} from '@app/composables/useDaystromUpdate';
import {useGameState} from '@app/composables/useGameState';
import {useProfileState} from '@app/composables/useProfileState';
import {useSettings} from '@app/composables/useSettings';
import {useI18n} from '@app/i18n';
import shellDefaults from '@app/locales/en/shell.json';
import {computed, onMounted, onUnmounted, ref} from 'vue';

/** Main application views shown below the persistent status bar. */
type ActiveView = 'accounts' | 'settings';

/** Dialogues that can temporarily cover the main interaction layer. */
type ActiveDialog = 'update' | 'rollback' | 'new-account' | null;

const activeView = ref<ActiveView>('accounts');
const activeDialog = ref<ActiveDialog>(null);
const {t} = useI18n('shell', shellDefaults);

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
  isProfileRunning,
  markLaunched,
  init: initProfileState,
  destroy: destroyProfileState,
} = useProfileState();

const {init: initSettings} = useSettings();

const {
  status: daystromUpdate,
  check: checkDaystromUpdate,
  dismiss: dismissDaystromUpdate,
  install: installDaystromUpdate,
  init: initDaystromUpdate,
  destroy: destroyDaystromUpdate,
} = useDaystromUpdate();

const {
  status: daystromRollback,
  restore: restoreDaystrom,
  init: initDaystromRollback,
  destroy: destroyDaystromRollback,
} = useDaystromRollback();

const updateBusy = computed(() =>
  ['confirming', 'retaining_rollback', 'downloading', 'installing'].includes(daystromUpdate.value.phase));

const rollbackBusy = computed(() =>
  ['preparing', 'installing'].includes(daystromRollback.value.phase));

/** Open one application-level dialog. */
function openDialog(dialog: Exclude<ActiveDialog, null>): void {
  activeDialog.value = dialog;
}

/** Close the active application-level dialog. */
function closeDialog(): void {
  activeDialog.value = null;
}

/** Replace the accounts view with application settings. */
function showSettings(): void {
  activeView.value = 'settings';
}

/** Return from application settings to the accounts view. */
function showAccounts(): void {
  activeView.value = 'accounts';
}

/** Launch one account and apply the existing launch cooldown to known profiles. */
function handleLaunch(profile: string): void {
  if (profile !== 'initial' && profile !== 'new_account') {
    markLaunched(profile);
  }
  launchGame(profile);
}

/** Dismiss the offered update and close its details. */
function handleUpdateLater(): void {
  dismissDaystromUpdate();
  closeDialog();
}

/** Confirm a new account launch and close its confirmation dialog. */
function confirmNewAccount(): void {
  closeDialog();
  handleLaunch('new_account');
}

onMounted(() => {
  initGameState();
  initProfileState();
  initSettings();
  initDaystromUpdate();
  initDaystromRollback();
});

onUnmounted(() => {
  destroyGameState();
  destroyProfileState();
  destroyDaystromUpdate();
  destroyDaystromRollback();
});
</script>

<template>
  <main>
    <AppHeader :version="version" @open-settings="showSettings" />

    <StatusBar :status="status"
        :loading="loading"
        :error="error"
        :action-error="actionError"
        :action-pending="actionPending"
        :update="daystromUpdate"
        :rollback="daystromRollback"
        @check-update="checkDaystromUpdate"
        @open-update="openDialog('update')"
        @open-rollback="openDialog('rollback')"
        @install-mod="installMod"
        @remove-mod="removeMod"
        @open-game-updater="openUpdater" />

    <SettingsView v-if="activeView === 'settings'"
        :rollback-version="daystromRollback.version"
        @close="showAccounts"
        @open-rollback="openDialog('rollback')" />

    <AccountTabs v-else-if="!error"
        :installed="status.installed"
        :mod-deployed="status.mod_deployed"
        :can-launch-initial="status.can_launch"
        :action-pending="actionPending"
        :external-game-running="profiles.external_game_running"
        :game-origin-pending="profiles.game_origin_pending"
        :profiles="profiles.profiles"
        :is-profile-running="isProfileRunning"
        @launch="handleLaunch"
        @add-account="openDialog('new-account')" />

    <AppDialog v-if="activeDialog === 'new-account'" :title="t('addAccount')" @close="closeDialog">
      <NewAccountDialog @confirm="confirmNewAccount" @cancel="closeDialog" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'update'"
        :title="daystromUpdate.version
          ? t('updateAvailable', { version: daystromUpdate.version })
          : t('update')"
        @close="closeDialog">
      <UpdateDialog :status="daystromUpdate"
          :rollback-busy="rollbackBusy"
          @install="installDaystromUpdate"
          @later="handleUpdateLater" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'rollback'" :title="t('recovery')" @close="closeDialog">
      <RollbackDialog :status="daystromRollback"
          :game-running="status.game_running"
          :update-busy="updateBusy"
          @restore="restoreDaystrom" />
    </AppDialog>
  </main>
</template>

<style>
html,
body,
#app {
  height: 100%;
}

body {
  box-sizing: border-box;
  margin: 0;
  padding: 0.5rem;
  overflow: hidden;
  font-family: system-ui, -apple-system, sans-serif;
}

main {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

*,
*::before,
*::after {
  user-select: none;
}

button,
input {
  font: inherit;
}
</style>
