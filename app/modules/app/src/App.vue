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
import {computed, onMounted, onUnmounted, ref} from 'vue';

/** Dialogues that can replace the main interaction layer. */
type ActiveDialog = 'settings' | 'update' | 'rollback' | 'new-account' | null;

const activeDialog = ref<ActiveDialog>(null);

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
    <AppHeader :version="version" @open-settings="openDialog('settings')" />

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

    <AccountTabs v-if="!error"
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

    <AppDialog v-if="activeDialog === 'new-account'" title="Add account" @close="closeDialog">
      <NewAccountDialog @confirm="confirmNewAccount" @cancel="closeDialog" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'settings'" title="Settings" @close="closeDialog">
      <SettingsView :rollback-version="daystromRollback.version"
          @open-rollback="openDialog('rollback')" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'update'"
        :title="daystromUpdate.version
          ? `Project Daystrom ${daystromUpdate.version} is available`
          : 'Daystrom update'"
        @close="closeDialog">
      <UpdateDialog :status="daystromUpdate"
          :rollback-busy="rollbackBusy"
          @install="installDaystromUpdate"
          @later="handleUpdateLater" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'rollback'" title="Daystrom recovery" @close="closeDialog">
      <RollbackDialog :status="daystromRollback"
          :game-running="status.game_running"
          :update-busy="updateBusy"
          @restore="restoreDaystrom" />
    </AppDialog>
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

button,
input {
  font: inherit;
}
</style>
