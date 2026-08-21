<script setup lang="ts">
import type {ProfileInfo} from '@generated/ProfileInfo';
import type {UiErrorCode} from '@generated/UiErrorCode';
import {deleteLocalProfile} from '@app/commands/profiles';
import {closeMainWindow} from '@app/commands/window';
import AccountTabs from '@app/components/AccountTabs.vue';
import AppDialog from '@app/components/AppDialog.vue';
import AppHeader from '@app/components/AppHeader.vue';
import DeleteAccountDialog from '@app/components/DeleteAccountDialog.vue';
import NewAccountDialog from '@app/components/NewAccountDialog.vue';
import RollbackDialog from '@app/components/RollbackDialog.vue';
import SettingsView from '@app/components/SettingsView.vue';
import StatusBar from '@app/components/StatusBar.vue';
import UpdateDialog from '@app/components/UpdateDialog.vue';
import ZoomOverlay from '@app/components/ZoomOverlay.vue';
import {useDaystromRollback} from '@app/composables/useDaystromRollback';
import {useDaystromUpdate} from '@app/composables/useDaystromUpdate';
import {DIALOG_PRIORITY, useDialogQueue} from '@app/composables/useDialogQueue';
import {useGameState} from '@app/composables/useGameState';
import {useProfileState} from '@app/composables/useProfileState';
import {useSettings} from '@app/composables/useSettings';
import {normalizeUiError} from '@app/composables/useUiError';
import {useI18n} from '@app/i18n';
import shellDefaults from '@app/locales/en/shell.json';
import {getLogger} from '@app/log';
import {computed, onMounted, onUnmounted, ref, watch} from 'vue';

/** Main application views shown below the persistent status bar. */
type ActiveView = 'accounts' | 'settings';

/** Dialogues that can temporarily cover the main interaction layer. */
type ActiveDialog = 'update' | 'rollback' | 'new-account' | 'delete-account' | 'mod-connection';

const activeView = ref<ActiveView>('accounts');
const dialogQueue = useDialogQueue<ActiveDialog>();
const {activeDialog} = dialogQueue;
const accountToDelete = ref<ProfileInfo | null>(null);
const accountDeletionPending = ref(false);
const accountDeletionError = ref<UiErrorCode | null>(null);
const {t} = useI18n('shell', shellDefaults);
const log = getLogger('Window');

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

const updateBusy = computed(() => daystromUpdate.value.busy);

const rollbackBusy = computed(() => daystromRollback.value.busy);

const updateCheckBusy = computed(() => updateBusy.value || rollbackBusy.value);

watch(
  () => profiles.value.mod_connection_missing,
  (missing) => {
    if (missing) {
      openDialog('mod-connection');
    } else {
      dialogQueue.cancel('mod-connection');
    }
  },
);

/** Build the live interruption rule for a dialogue whose state may change after opening. */
function dialogInterruptibility(dialog: ActiveDialog): (() => boolean) | undefined {
  switch (dialog) {
    case 'delete-account':
      return () => false;
    case 'update':
      return () => !updateBusy.value;
    case 'rollback':
      return () => !rollbackBusy.value;
    default:
      return undefined;
  }
}

/** Queue one application-level dialogue without displacing the current user flow. */
function openDialog(dialog: ActiveDialog): void {
  dialogQueue.request({
    id: dialog,
    priority: dialog === 'mod-connection' ? DIALOG_PRIORITY.high : DIALOG_PRIORITY.normal,
    canInterrupt: dialog === 'mod-connection',
    isInterruptible: dialogInterruptibility(dialog),
    isValid: dialog === 'mod-connection' ? () => profiles.value.mod_connection_missing : undefined,
  });
}

/** Close only the named application-level dialogue. */
function closeDialog(dialog: ActiveDialog): void {
  dialogQueue.close(dialog);
}

/** Toggle between the accounts and application settings views. */
function toggleSettings(): void {
  activeView.value = activeView.value === 'settings' ? 'accounts' : 'settings';
}

/** Return from application settings to the accounts view. */
function showAccounts(): void {
  activeView.value = 'accounts';
}

/** Request the backend to launch one account. */
function handleLaunch(profile: string): void {
  launchGame(profile);
}

/** Dismiss the offered update and close its details. */
function handleUpdateLater(): void {
  dismissDaystromUpdate();
  closeDialog('update');
}

/** Confirm a new account launch and close its confirmation dialogue. */
function confirmNewAccount(): void {
  closeDialog('new-account');
  handleLaunch('new_account');
}

/** Open the destructive confirmation flow for one known local account profile. */
function openDeleteAccount(profile: ProfileInfo): void {
  accountToDelete.value = profile;
  accountDeletionError.value = null;
  openDialog('delete-account');
}

/** Close the local account deletion flow unless its backend operation is still running. */
function closeDeleteAccount(): void {
  if (accountDeletionPending.value) {
    return;
  }
  accountToDelete.value = null;
  accountDeletionError.value = null;
  closeDialog('delete-account');
}

/** Delete the confirmed local account data and leave Scopely's remote account untouched. */
function confirmDeleteAccount(): void {
  const profile = accountToDelete.value;
  if (!profile || accountDeletionPending.value) {
    return;
  }

  accountDeletionPending.value = true;
  accountDeletionError.value = null;
  deleteLocalProfile(profile.stem)
    .then(() => {
      accountDeletionPending.value = false;
      accountToDelete.value = null;
      closeDialog('delete-account');
    })
    .catch((reason) => {
      accountDeletionPending.value = false;
      accountDeletionError.value = normalizeUiError(reason);
    });
}

/** Delegate the custom title-bar close action to backend-owned process policy. */
function handleCloseWindow(): void {
  closeMainWindow().catch(reason => log.error('Failed to close the main window:', reason));
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
    <AppHeader :version="version" @open-settings="toggleSettings" @close-window="handleCloseWindow" />

    <div class="app-content">
      <StatusBar :status="status"
          :loading="loading"
          :error="error"
          :action-error="actionError"
          :action-pending="actionPending"
          :update-check-busy="updateCheckBusy"
          :update="daystromUpdate"
          :rollback="daystromRollback"
          :mod-connection-missing="profiles.mod_connection_missing"
          @open-update="openDialog('update')"
          @open-rollback="openDialog('rollback')"
          @check-update="checkDaystromUpdate"
          @install-mod="installMod"
          @remove-mod="removeMod"
          @open-game-updater="openUpdater"
          @open-mod-warning="openDialog('mod-connection')" />

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
          @add-account="openDialog('new-account')"
          @delete-account="openDeleteAccount" />
    </div>

    <AppDialog v-if="activeDialog === 'new-account'"
        :title="t('addAccount')"
        @close="closeDialog('new-account')">
      <NewAccountDialog @confirm="confirmNewAccount" @cancel="closeDialog('new-account')" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'delete-account' && accountToDelete"
        :title="t('removeAccount')"
        @close="closeDeleteAccount">
      <DeleteAccountDialog :profile="accountToDelete"
          :pending="accountDeletionPending"
          :error="accountDeletionError"
          @confirm="confirmDeleteAccount"
          @cancel="closeDeleteAccount" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'update'"
        :title="daystromUpdate.version
          ? t('updateAvailable', { version: daystromUpdate.version })
          : t('update')"
        @close="closeDialog('update')">
      <UpdateDialog :status="daystromUpdate"
          :rollback-busy="rollbackBusy"
          @install="installDaystromUpdate"
          @later="handleUpdateLater" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'rollback'"
        :title="t('recovery')"
        @close="closeDialog('rollback')">
      <RollbackDialog :status="daystromRollback"
          :game-running="status.game_running"
          :update-busy="updateBusy"
          @restore="restoreDaystrom" />
    </AppDialog>

    <AppDialog v-if="activeDialog === 'mod-connection'"
        :title="t('modConnectionTitle')"
        @close="closeDialog('mod-connection')">
      <p>{{ t('modConnectionBody') }}</p>
    </AppDialog>

    <ZoomOverlay />
  </main>
</template>

<style>
html,
body,
#app {
  height: 100%;
  background: transparent;
}

body {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
  overflow: hidden;
  color: var(--text-primary);
  font-family: var(--font-interface);
}

.app-content {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  padding: 0.5rem;
}

main {
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--border-default);
  border-radius: 0.75rem;
  background: var(--surface-canvas);
}

*,
*::before,
*::after {
  /* stylelint-disable-next-line property-no-vendor-prefix -- Required by macOS WKWebView. */
  -webkit-user-select: none;
  user-select: none;
}

button,
input {
  font: inherit;
}

input,
textarea,
pre,
code {
  /* stylelint-disable-next-line property-no-vendor-prefix -- Required by macOS WKWebView. */
  -webkit-user-select: text;
  user-select: text;
}
</style>
