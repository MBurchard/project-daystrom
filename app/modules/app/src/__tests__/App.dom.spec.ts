import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {GameStatus} from '@generated/GameStatus';
import type {ProfileState} from '@generated/ProfileState';
import {flushPromises, shallowMount} from '@vue/test-utils';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {nextTick, ref} from 'vue';
import App from '../App.vue';
import AccountTabs from '../components/AccountTabs.vue';
import AppDialog from '../components/AppDialog.vue';
import AppHeader from '../components/AppHeader.vue';
import DeleteAccountDialog from '../components/DeleteAccountDialog.vue';
import NewAccountDialog from '../components/NewAccountDialog.vue';
import RollbackDialog from '../components/RollbackDialog.vue';
import SettingsView from '../components/SettingsView.vue';
import StatusBar from '../components/StatusBar.vue';
import UpdateDialog from '../components/UpdateDialog.vue';

const mockUseGameState = vi.hoisted(() => vi.fn());
const mockUseProfileState = vi.hoisted(() => vi.fn());
const mockUseSettings = vi.hoisted(() => vi.fn());
const mockUseDaystromUpdate = vi.hoisted(() => vi.fn());
const mockUseDaystromRollback = vi.hoisted(() => vi.fn());
const mockDeleteLocalProfile = vi.hoisted(() => vi.fn());
const mockCloseMainWindow = vi.hoisted(() => vi.fn());

vi.mock('@app/composables/useGameState', () => ({useGameState: mockUseGameState}));
vi.mock('@app/composables/useProfileState', () => ({useProfileState: mockUseProfileState}));
vi.mock('@app/composables/useSettings', () => ({useSettings: mockUseSettings}));
vi.mock('@app/composables/useDaystromUpdate', () => ({useDaystromUpdate: mockUseDaystromUpdate}));
vi.mock('@app/composables/useDaystromRollback', () => ({useDaystromRollback: mockUseDaystromRollback}));
vi.mock('@app/commands/profiles', () => ({deleteLocalProfile: mockDeleteLocalProfile}));
vi.mock('@app/commands/window', () => ({closeMainWindow: mockCloseMainWindow}));

/** Build a complete neutral game status. */
function gameStatus(): GameStatus {
  return {
    installed: true,
    game_version: 188,
    mod_available: true,
    mod_installable: true,
    mod_deployed: true,
    mod_outdated: false,
    mod_removable: true,
    game_running: false,
    launcher_running: false,
    remote_version: 188,
    update_check_failed: false,
    game_started_by_us: false,
    launcher_started_by_us: false,
    update_available: false,
    can_launch: true,
    can_install_mod: true,
    can_remove_mod: true,
    can_launch_updater: true,
    should_block_quit: false,
    version_check_class: 'ok',
  };
}

/** Build a complete neutral profile status. */
function profileState(): ProfileState {
  return {
    profiles: [],
    running_profiles: [],
    external_game_running: false,
    game_origin_pending: false,
    mod_connection_missing: false,
  };
}

/** Build a complete neutral update status. */
function updateStatus(): DaystromUpdateStatus {
  return {
    phase: 'available',
    version: '0.10.1',
    notes: null,
    download_progress: null,
    error: null,
    dismissed: false,
    can_install: true,
    busy: false,
  };
}

/** Build a complete neutral rollback status. */
function rollbackStatus(): DaystromRollbackStatus {
  return {
    phase: 'available',
    version: '0.9.1',
    error: null,
    can_restore: true,
    mod_restore_pending: false,
    busy: false,
  };
}

describe('app', () => {
  const actions = {
    installMod: vi.fn(),
    removeMod: vi.fn(),
    openUpdater: vi.fn(),
    launchGame: vi.fn(),
    initGameState: vi.fn(),
    destroyGameState: vi.fn(),
    isProfileRunning: vi.fn(() => false),
    initProfileState: vi.fn(),
    destroyProfileState: vi.fn(),
    initSettings: vi.fn(),
    checkDaystromUpdate: vi.fn(),
    dismissDaystromUpdate: vi.fn(),
    installDaystromUpdate: vi.fn(),
    initDaystromUpdate: vi.fn(),
    destroyDaystromUpdate: vi.fn(),
    restoreDaystrom: vi.fn(),
    initDaystromRollback: vi.fn(),
    destroyDaystromRollback: vi.fn(),
  };
  const status = ref(gameStatus());
  const error = ref<string | null>(null);
  const update = ref(updateStatus());
  const rollback = ref(rollbackStatus());
  const profiles = ref(profileState());

  beforeEach(() => {
    vi.clearAllMocks();
    status.value = gameStatus();
    error.value = null;
    update.value = updateStatus();
    rollback.value = rollbackStatus();
    profiles.value = profileState();
    mockDeleteLocalProfile.mockResolvedValue(undefined);
    mockCloseMainWindow.mockResolvedValue(undefined);
    mockUseGameState.mockReturnValue({
      version: ref('0.10.0'),
      status,
      loading: ref(false),
      error,
      actionError: ref(null),
      actionPending: ref(false),
      installMod: actions.installMod,
      removeMod: actions.removeMod,
      openUpdater: actions.openUpdater,
      launchGame: actions.launchGame,
      init: actions.initGameState,
      destroy: actions.destroyGameState,
    });
    mockUseProfileState.mockReturnValue({
      profiles,
      isProfileRunning: actions.isProfileRunning,
      init: actions.initProfileState,
      destroy: actions.destroyProfileState,
    });
    mockUseSettings.mockReturnValue({init: actions.initSettings});
    mockUseDaystromUpdate.mockReturnValue({
      status: update,
      check: actions.checkDaystromUpdate,
      dismiss: actions.dismissDaystromUpdate,
      install: actions.installDaystromUpdate,
      init: actions.initDaystromUpdate,
      destroy: actions.destroyDaystromUpdate,
    });
    mockUseDaystromRollback.mockReturnValue({
      status: rollback,
      restore: actions.restoreDaystrom,
      init: actions.initDaystromRollback,
      destroy: actions.destroyDaystromRollback,
    });
  });

  it('initializes and destroys every application state owner', () => {
    const wrapper = shallowMount(App);

    expect(actions.initGameState).toHaveBeenCalledOnce();
    expect(actions.initProfileState).toHaveBeenCalledOnce();
    expect(actions.initSettings).toHaveBeenCalledOnce();
    expect(actions.initDaystromUpdate).toHaveBeenCalledOnce();
    expect(actions.initDaystromRollback).toHaveBeenCalledOnce();

    wrapper.unmount();

    expect(actions.destroyGameState).toHaveBeenCalledOnce();
    expect(actions.destroyProfileState).toHaveBeenCalledOnce();
    expect(actions.destroyDaystromUpdate).toHaveBeenCalledOnce();
    expect(actions.destroyDaystromRollback).toHaveBeenCalledOnce();
  });

  it('forwards status actions to their backend-owned composables', () => {
    const wrapper = shallowMount(App);
    const statusBar = wrapper.findComponent(StatusBar);

    statusBar.vm.$emit('installMod');
    statusBar.vm.$emit('removeMod');
    statusBar.vm.$emit('openGameUpdater');
    statusBar.vm.$emit('checkUpdate');

    expect(actions.installMod).toHaveBeenCalledOnce();
    expect(actions.removeMod).toHaveBeenCalledOnce();
    expect(actions.openUpdater).toHaveBeenCalledOnce();
    expect(actions.checkDaystromUpdate).toHaveBeenCalledOnce();
  });

  it('delegates custom title-bar close requests and handles IPC rejection', async () => {
    const wrapper = shallowMount(App);
    const header = wrapper.findComponent(AppHeader);

    header.vm.$emit('closeWindow');
    await flushPromises();
    mockCloseMainWindow.mockRejectedValueOnce('IPC unavailable');
    header.vm.$emit('closeWindow');
    await flushPromises();

    expect(mockCloseMainWindow).toHaveBeenCalledTimes(2);
  });

  it('delegates profile launches to the backend', () => {
    const wrapper = shallowMount(App);
    const tabs = wrapper.findComponent(AccountTabs);

    tabs.vm.$emit('launch', 'test-profile');
    tabs.vm.$emit('launch', 'initial');

    expect(actions.launchGame).toHaveBeenNthCalledWith(1, 'test-profile');
    expect(actions.launchGame).toHaveBeenNthCalledWith(2, 'initial');
  });

  it('switches between accounts and settings while recovery remains a dialog', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    expect(wrapper.findComponent(AccountTabs).exists()).toBe(true);
    wrapper.findComponent(AppHeader).vm.$emit('openSettings');
    await nextTick();
    expect(wrapper.findComponent(AccountTabs).exists()).toBe(false);
    expect(wrapper.findComponent(SettingsView).exists()).toBe(true);
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);
    wrapper.findComponent(AppHeader).vm.$emit('openSettings');
    await nextTick();
    expect(wrapper.findComponent(SettingsView).exists()).toBe(false);
    expect(wrapper.findComponent(AccountTabs).exists()).toBe(true);

    wrapper.findComponent(AppHeader).vm.$emit('openSettings');
    await nextTick();

    wrapper.findComponent(SettingsView).vm.$emit('openRollback');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Return to the previous version');

    wrapper.findComponent(RollbackDialog).vm.$emit('restore');
    expect(actions.restoreDaystrom).toHaveBeenCalledOnce();
    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    wrapper.findComponent(SettingsView).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(SettingsView).exists()).toBe(false);
    expect(wrapper.findComponent(AccountTabs).exists()).toBe(true);
  });

  it('opens update details, installs, and dismisses the offer', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Project Daystrom 0.10.1 is available');
    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    wrapper.findComponent(UpdateDialog).vm.$emit('install');
    expect(actions.installDaystromUpdate).toHaveBeenCalledOnce();
    wrapper.findComponent(UpdateDialog).vm.$emit('later');
    await nextTick();
    expect(actions.dismissDaystromUpdate).toHaveBeenCalledOnce();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    update.value.version = null;
    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Daystrom update');
  });

  it('opens missing-mod guidance automatically and from the status bar', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    profiles.value.mod_connection_missing = true;
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
    expect(wrapper.findComponent(AppDialog).text()).toContain('There is no connection to the Daystrom mod');

    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    wrapper.findComponent(StatusBar).vm.$emit('openModWarning');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');

    wrapper.findComponent(AppDialog).vm.$emit('close');
    profiles.value.mod_connection_missing = false;
    await nextTick();
    profiles.value.mod_connection_missing = true;
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
  });

  it('interrupts and resumes an ordinary dialogue for missing-mod guidance', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    expect(wrapper.findComponent(UpdateDialog).exists()).toBe(true);

    profiles.value.mod_connection_missing = true;
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
    expect(wrapper.findComponent(UpdateDialog).exists()).toBe(false);

    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(UpdateDialog).exists()).toBe(true);
  });

  it('queues missing-mod guidance behind active update and rollback operations', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    update.value.busy = true;
    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    profiles.value.mod_connection_missing = true;
    await nextTick();
    expect(wrapper.findComponent(UpdateDialog).exists()).toBe(true);

    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
    profiles.value.mod_connection_missing = false;
    await nextTick();

    rollback.value.busy = true;
    wrapper.findComponent(StatusBar).vm.$emit('openRollback');
    await nextTick();
    profiles.value.mod_connection_missing = true;
    await nextTick();
    expect(wrapper.findComponent(RollbackDialog).exists()).toBe(true);

    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
  });

  it('confirms new account launches', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AccountTabs).vm.$emit('addAccount');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Add account');
    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    wrapper.findComponent(AccountTabs).vm.$emit('addAccount');
    await nextTick();
    wrapper.findComponent(NewAccountDialog).vm.$emit('cancel');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);

    wrapper.findComponent(AccountTabs).vm.$emit('addAccount');
    await nextTick();
    wrapper.findComponent(NewAccountDialog).vm.$emit('confirm');
    await nextTick();

    expect(actions.launchGame).toHaveBeenCalledWith('new_account');
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);
  });

  it('confirms local account deletion and closes after backend success', async () => {
    const profile = {name: 'Test Account', server: 1, stem: '1_TestAccount', primary: false};
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AccountTabs).vm.$emit('deleteAccount', profile);
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Remove account from Daystrom');
    expect(wrapper.findComponent(DeleteAccountDialog).props('profile')).toEqual(profile);
    wrapper.findComponent(DeleteAccountDialog).vm.$emit('confirm');
    await flushPromises();

    expect(mockDeleteLocalProfile).toHaveBeenCalledWith('1_TestAccount');
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);
  });

  it('keeps deletion errors visible before showing a queued mod warning', async () => {
    const profile = {name: 'Test Account', server: 1, stem: '1_TestAccount', primary: false};
    let rejectDeletion!: (reason: unknown) => void;
    mockDeleteLocalProfile.mockReturnValue(new Promise<void>((_resolve, reject) => {
      rejectDeletion = reject;
    }));
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AccountTabs).vm.$emit('deleteAccount', profile);
    await nextTick();
    wrapper.findComponent(DeleteAccountDialog).vm.$emit('confirm');
    profiles.value.mod_connection_missing = true;
    await nextTick();

    expect(wrapper.findComponent(DeleteAccountDialog).exists()).toBe(true);
    rejectDeletion('profile_deletion_failed');
    await flushPromises();

    expect(wrapper.findComponent(DeleteAccountDialog).props('error')).toBe('profile_deletion_failed');
    wrapper.findComponent(DeleteAccountDialog).vm.$emit('cancel');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
  });

  it('prevents duplicate deletion and closing while deletion is pending', async () => {
    const profile = {name: 'Test Account', server: 1, stem: '1_TestAccount', primary: false};
    let resolveDeletion!: () => void;
    mockDeleteLocalProfile.mockReturnValue(new Promise<void>((resolve) => {
      resolveDeletion = resolve;
    }));
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AccountTabs).vm.$emit('deleteAccount', profile);
    await nextTick();
    wrapper.findComponent(DeleteAccountDialog).vm.$emit('confirm');
    await nextTick();
    wrapper.findComponent(DeleteAccountDialog).vm.$emit('confirm');
    wrapper.findComponent(AppDialog).vm.$emit('close');
    profiles.value.mod_connection_missing = true;
    await nextTick();

    expect(mockDeleteLocalProfile).toHaveBeenCalledOnce();
    expect(wrapper.findComponent(DeleteAccountDialog).exists()).toBe(true);
    resolveDeletion();
    await flushPromises();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('STFC is running without the Daystrom mod');
  });

  it('passes maintenance states to the opposite recovery action', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    update.value.busy = true;
    wrapper.findComponent(StatusBar).vm.$emit('openRollback');
    await nextTick();
    expect(wrapper.findComponent(RollbackDialog).props('updateBusy')).toBe(true);

    wrapper.findComponent(AppDialog).vm.$emit('close');
    rollback.value.busy = true;
    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    expect(wrapper.findComponent(UpdateDialog).props('rollbackBusy')).toBe(true);
  });

  it('hides account controls when game-state detection failed', async () => {
    const wrapper = shallowMount(App);
    expect(wrapper.findComponent(AccountTabs).exists()).toBe(true);

    error.value = 'detection failed';
    await nextTick();

    expect(wrapper.findComponent(AccountTabs).exists()).toBe(false);
  });
});
