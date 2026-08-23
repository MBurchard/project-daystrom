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
import SafetyNoticeDialog from '@app/components/SafetyNoticeDialog.vue';
import SettingsView from '@app/components/SettingsView.vue';
import StatusBar from '@app/components/StatusBar.vue';
import UpdateDialog from '@app/components/UpdateDialog.vue';
import ZoomOverlay from '@app/components/ZoomOverlay.vue';
import {useDaystromRollback} from '@app/composables/useDaystromRollback';
import {useDaystromUpdate} from '@app/composables/useDaystromUpdate';
import {DIALOG_PRIORITY, useDialogQueue} from '@app/composables/useDialogQueue';
import {useGameState} from '@app/composables/useGameState';
import {useProfileState} from '@app/composables/useProfileState';
import {useSafetyNotice} from '@app/composables/useSafetyNotice';
import {useSettings} from '@app/composables/useSettings';
import {normalizeUiError} from '@app/composables/useUiError';
import {useI18n} from '@app/i18n';
import safetyDefaults from '@app/locales/en/safety.json';
import shellDefaults from '@app/locales/en/shell.json';
import {getLogger} from '@app/log';
import {NEW_ACCOUNT_PROFILE_STEM} from '@app/profileProtocol';
import {computed, onMounted, onUnmounted, ref, watch} from 'vue';

/** Main application views shown below the persistent status bar. */
type ActiveView = 'accounts' | 'settings';

/** Dialogues that can temporarily cover the main interaction layer. */
type ActiveDialog = 'update' | 'rollback' | 'new-account' | 'delete-account' | 'mod-connection' |
  'game-start-failed' | 'game-status-unclear' | 'safety-notice';

const activeView = ref<ActiveView>('accounts');
const dialogQueue = useDialogQueue<ActiveDialog>();
const {activeDialog} = dialogQueue;
const safetyNoticeReview = ref(false);
const accountToDelete = ref<ProfileInfo | null>(null);
const accountDeletionPending = ref(false);
const accountDeletionError = ref<UiErrorCode | null>(null);
const {t} = useI18n('shell', shellDefaults);
const {t: safetyText} = useI18n('safety', safetyDefaults);
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
  terminateFailedGameStarts,
  terminateUnconfirmedGameStarts,
  init: initGameState,
  destroy: destroyGameState,
} = useGameState();

const {
  profiles,
  isProfileRunning,
  isProfileStarting,
  isProfileStartFailed,
  isProfileStatusUnclear,
  init: initProfileState,
  destroy: destroyProfileState,
} = useProfileState();

/** Whether at least one Daystrom-tracked game is waiting for its first ready UI frame. */
const trackedGameStarting = computed(() => profiles.value.starting_profiles.length > 0);

/** Whether at least one Daystrom-tracked game missed its game UI startup deadline. */
const trackedGameFailed = computed(() => profiles.value.failed_profiles.length > 0);

/** Whether at least one tracked game's UI readiness cannot be observed. */
const trackedGameStatusUnclear = computed(() => profiles.value.unclear_profiles.length > 0);

/** Whether at least one Daystrom-tracked game process is running. */
const trackedGameRunning = computed(() => profiles.value.running_profiles.length > 0);

/** Whether at least one tracked game has a ready UI. */
const trackedGameEstablished = computed(() => profiles.value.ready_profiles.length > 0);

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

const {
  required: safetyNoticeRequired,
  pending: safetyNoticePending,
  failed: safetyNoticeFailed,
  context: safetyNoticeContext,
  init: initSafetyNotice,
  acknowledge: acknowledgeSafetyNotice,
} = useSafetyNotice();

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

watch(trackedGameFailed, (failed) => {
  if (!failed) {
    dialogQueue.cancel('game-start-failed');
  }
});

watch(trackedGameStatusUnclear, (unclear) => {
  if (!unclear) {
    dialogQueue.cancel('game-status-unclear');
  }
});

watch(safetyNoticeRequired, (required) => {
  if (required) {
    openDialog('safety-notice');
  } else if (!safetyNoticeReview.value) {
    dialogQueue.cancel('safety-notice');
  }
});

/** Build the live interruption rule for a dialogue whose state may change after opening. */
function dialogInterruptibility(dialog: ActiveDialog): (() => boolean) | undefined {
  switch (dialog) {
    case 'delete-account':
      return () => false;
    case 'safety-notice':
      return () => !safetyNoticeRequired.value;
    case 'update':
      return () => !updateBusy.value;
    case 'rollback':
      return () => !rollbackBusy.value;
    default:
      return undefined;
  }
}

/** Build the live validity rule for a dialogue backed by changing application state. */
function dialogValidity(dialog: ActiveDialog): (() => boolean) | undefined {
  switch (dialog) {
    case 'mod-connection':
      return () => profiles.value.mod_connection_missing;
    case 'game-start-failed':
      return () => trackedGameFailed.value;
    case 'game-status-unclear':
      return () => trackedGameStatusUnclear.value;
    case 'safety-notice':
      return () => safetyNoticeRequired.value || safetyNoticeReview.value;
    default:
      return undefined;
  }
}

/** Queue one application-level dialogue without displacing the current user flow. */
function openDialog(dialog: ActiveDialog): void {
  const mandatorySafetyNotice = dialog === 'safety-notice' && safetyNoticeRequired.value;
  const highPriority = dialog === 'mod-connection' || dialog === 'game-start-failed';
  dialogQueue.request({
    id: dialog,
    priority: mandatorySafetyNotice ?
      DIALOG_PRIORITY.critical :
      highPriority ?
        DIALOG_PRIORITY.high :
        DIALOG_PRIORITY.normal,
    canInterrupt: dialog === 'mod-connection' || dialog === 'game-start-failed' || mandatorySafetyNotice,
    isInterruptible: dialogInterruptibility(dialog),
    isValid: dialogValidity(dialog),
  });
}

/** Close only the named application-level dialogue. */
function closeDialog(dialog: ActiveDialog): void {
  dialogQueue.close(dialog);
}

/** Open the safety notice for voluntary review without changing acknowledgement state. */
function openSafetyNoticeReview(): void {
  safetyNoticeReview.value = true;
  openDialog('safety-notice');
}

/** Close a voluntary safety-notice review while keeping mandatory notices locked. */
function closeSafetyNotice(): void {
  if (safetyNoticeRequired.value) {
    return;
  }
  safetyNoticeReview.value = false;
  closeDialog('safety-notice');
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
  handleLaunch(NEW_ACCOUNT_PROFILE_STEM);
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
  initSafetyNotice();
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
          :tracked-game-starting="trackedGameStarting"
          :tracked-game-running="trackedGameRunning"
          :tracked-game-established="trackedGameEstablished"
          :tracked-game-failed="trackedGameFailed"
          :tracked-game-status-unclear="trackedGameStatusUnclear"
          @open-update="openDialog('update')"
          @open-rollback="openDialog('rollback')"
          @check-update="checkDaystromUpdate"
          @install-mod="installMod"
          @remove-mod="removeMod"
          @open-game-updater="openUpdater"
          @open-mod-warning="openDialog('mod-connection')"
          @open-game-start-warning="openDialog('game-start-failed')"
          @open-game-status-unclear="openDialog('game-status-unclear')" />

      <SettingsView v-if="activeView === 'settings'"
          :rollback-version="daystromRollback.version"
          @close="showAccounts"
          @open-safety-notice="openSafetyNoticeReview"
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
          :is-profile-starting="isProfileStarting"
          :is-profile-start-failed="isProfileStartFailed"
          :is-profile-status-unclear="isProfileStatusUnclear"
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
      <button v-if="profiles.can_terminate_unconfirmed_start"
          :disabled="actionPending"
          @click="terminateUnconfirmedGameStarts">
        {{ t('terminateFailedGame') }}
      </button>
    </AppDialog>

    <AppDialog v-if="activeDialog === 'game-start-failed'"
        :title="t('gameStartFailedTitle')"
        @close="closeDialog('game-start-failed')">
      <p>{{ t('gameStartFailedBody') }}</p>
      <div class="game-start-failed-actions">
        <button :disabled="actionPending" @click="terminateFailedGameStarts">
          {{ t('terminateFailedGame') }}
        </button>
        <button :disabled="actionPending" @click="closeDialog('game-start-failed')">
          {{ t('closeGameStartHelp') }}
        </button>
      </div>
    </AppDialog>

    <AppDialog v-if="activeDialog === 'game-status-unclear'"
        :title="t('gameStatusUnclearTitle')"
        @close="closeDialog('game-status-unclear')">
      <p>{{ t('gameStatusUnclearBody') }}</p>
      <button :disabled="actionPending" @click="closeDialog('game-status-unclear')">
        {{ t('closeGameStartHelp') }}
      </button>
    </AppDialog>

    <AppDialog v-if="activeDialog === 'safety-notice'"
        :title="safetyText('title')"
        :dismissible="!safetyNoticeRequired"
        @close="closeSafetyNotice">
      <SafetyNoticeDialog :pending="safetyNoticePending"
          :failed="safetyNoticeFailed"
          :context="safetyNoticeContext"
          :acknowledgement-required="safetyNoticeRequired"
          @acknowledge="acknowledgeSafetyNotice" />
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

.game-start-failed-actions {
  display: flex;
  gap: 0.75rem;
  justify-content: flex-end;
  margin-top: 1rem;
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
