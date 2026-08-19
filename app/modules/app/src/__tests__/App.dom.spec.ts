import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {GameStatus} from '@generated/GameStatus';
import type {ProfileState} from '@generated/ProfileState';
import {shallowMount} from '@vue/test-utils';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {nextTick, ref} from 'vue';
import App from '../App.vue';
import AccountTabs from '../components/AccountTabs.vue';
import AppDialog from '../components/AppDialog.vue';
import AppHeader from '../components/AppHeader.vue';
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

vi.mock('@app/composables/useGameState', () => ({useGameState: mockUseGameState}));
vi.mock('@app/composables/useProfileState', () => ({useProfileState: mockUseProfileState}));
vi.mock('@app/composables/useSettings', () => ({useSettings: mockUseSettings}));
vi.mock('@app/composables/useDaystromUpdate', () => ({useDaystromUpdate: mockUseDaystromUpdate}));
vi.mock('@app/composables/useDaystromRollback', () => ({useDaystromRollback: mockUseDaystromRollback}));

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
    markLaunched: vi.fn(),
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

  beforeEach(() => {
    vi.clearAllMocks();
    status.value = gameStatus();
    error.value = null;
    update.value = updateStatus();
    rollback.value = rollbackStatus();
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
      profiles: ref(profileState()),
      isProfileRunning: actions.isProfileRunning,
      markLaunched: actions.markLaunched,
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

    statusBar.vm.$emit('checkUpdate');
    statusBar.vm.$emit('installMod');
    statusBar.vm.$emit('removeMod');
    statusBar.vm.$emit('openGameUpdater');

    expect(actions.checkDaystromUpdate).toHaveBeenCalledOnce();
    expect(actions.installMod).toHaveBeenCalledOnce();
    expect(actions.removeMod).toHaveBeenCalledOnce();
    expect(actions.openUpdater).toHaveBeenCalledOnce();
  });

  it('marks known profiles before launch but leaves special launches unmarked', () => {
    const wrapper = shallowMount(App);
    const tabs = wrapper.findComponent(AccountTabs);

    tabs.vm.$emit('launch', 'test-profile');
    tabs.vm.$emit('launch', 'initial');

    expect(actions.markLaunched).toHaveBeenCalledOnce();
    expect(actions.markLaunched).toHaveBeenCalledWith('test-profile');
    expect(actions.launchGame).toHaveBeenNthCalledWith(1, 'test-profile');
    expect(actions.launchGame).toHaveBeenNthCalledWith(2, 'initial');
  });

  it('opens and closes settings and recovery dialogs', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AppHeader).vm.$emit('openSettings');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Settings');

    wrapper.findComponent(SettingsView).vm.$emit('openRollback');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Daystrom recovery');

    wrapper.findComponent(RollbackDialog).vm.$emit('restore');
    expect(actions.restoreDaystrom).toHaveBeenCalledOnce();
    wrapper.findComponent(AppDialog).vm.$emit('close');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);
  });

  it('opens update details, installs, and dismisses the offer', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(StatusBar).vm.$emit('openUpdate');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Project Daystrom 0.10.1 is available');

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

  it('confirms new accounts without marking a known profile', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    wrapper.findComponent(AccountTabs).vm.$emit('addAccount');
    await nextTick();
    expect(wrapper.findComponent(AppDialog).props('title')).toBe('Add account');
    wrapper.findComponent(NewAccountDialog).vm.$emit('confirm');
    await nextTick();

    expect(actions.markLaunched).not.toHaveBeenCalled();
    expect(actions.launchGame).toHaveBeenCalledWith('new_account');
    expect(wrapper.findComponent(AppDialog).exists()).toBe(false);
  });

  it('passes maintenance states to the opposite recovery action', async () => {
    const wrapper = shallowMount(App, {global: {renderStubDefaultSlot: true}});

    update.value.phase = 'downloading';
    wrapper.findComponent(StatusBar).vm.$emit('openRollback');
    await nextTick();
    expect(wrapper.findComponent(RollbackDialog).props('updateBusy')).toBe(true);

    wrapper.findComponent(AppDialog).vm.$emit('close');
    rollback.value.phase = 'preparing';
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
