import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {GameStatus} from '@generated/GameStatus';
import {mount} from '@vue/test-utils';
import {describe, expect, it} from 'vitest';
import StatusBar from '../StatusBar.vue';

/** Build a complete neutral game status. */
function gameStatus(overrides: Partial<GameStatus> = {}): GameStatus {
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
    ...overrides,
  };
}

/** Build a complete neutral Daystrom update status. */
function updateStatus(overrides: Partial<DaystromUpdateStatus> = {}): DaystromUpdateStatus {
  return {
    phase: 'up_to_date',
    version: null,
    notes: null,
    download_progress: null,
    error: null,
    dismissed: false,
    can_install: false,
    ...overrides,
  };
}

/** Build a complete neutral rollback status. */
function rollbackStatus(overrides: Partial<DaystromRollbackStatus> = {}): DaystromRollbackStatus {
  return {
    phase: 'unavailable',
    version: null,
    error: null,
    can_restore: false,
    mod_restore_pending: false,
    ...overrides,
  };
}

/** Build complete status-bar props. */
function props(overrides: Record<string, unknown> = {}) {
  return {
    status: gameStatus(),
    loading: false,
    error: null,
    actionError: null,
    actionPending: false,
    updateCheckBusy: false,
    update: updateStatus(),
    rollback: rollbackStatus(),
    ...overrides,
  };
}

describe('statusBar', () => {
  it('renders loading, failure, missing, and installed game states', async () => {
    const wrapper = mount(StatusBar, {props: props({loading: true})});
    expect(wrapper.text()).toContain('Detecting STFC');

    await wrapper.setProps({loading: false, error: 'failed'});
    expect(wrapper.text()).toContain('Game status unavailable');
    await wrapper.setProps({error: null, status: gameStatus({installed: false})});
    expect(wrapper.text()).toContain('STFC not installed');
    await wrapper.setProps({status: gameStatus({game_version: null})});
    expect(wrapper.text()).toContain('STFC');
    expect(wrapper.text()).not.toContain('v188');
  });

  it('opens the game updater and reports update-check failures', async () => {
    const wrapper = mount(StatusBar, {
      props: props({status: gameStatus({update_available: true, remote_version: 189})}),
    });
    const updateButton = wrapper.findAll('button').find(button => button.text().includes('STFC v189'))!;
    await updateButton.trigger('click');
    expect(wrapper.emitted('openGameUpdater')).toHaveLength(1);

    await wrapper.setProps({
      actionPending: true,
      status: gameStatus({update_available: true, remote_version: 189, can_launch_updater: false}),
    });
    expect(wrapper.findAll('button').find(button => button.text().includes('STFC v189'))!
      .attributes('disabled')).toBeDefined();
    await wrapper.setProps({status: gameStatus({update_available: true, remote_version: null})});
    expect(wrapper.text()).toContain('STFC v available');
    await wrapper.setProps({
      actionPending: false,
      status: gameStatus({update_check_failed: true}),
    });
    expect(wrapper.text()).toContain('STFC update check failed');
  });

  it('renders and dispatches all mod actions', async () => {
    const wrapper = mount(StatusBar, {props: props()});
    expect(wrapper.text()).toContain('Mod ready');
    const reinstallButton = wrapper.get('.mod-reinstall');
    expect(reinstallButton.attributes('aria-label')).toBe('Reinstall mod');
    expect(reinstallButton.attributes('data-tooltip')).toBe('Reinstall mod');
    await reinstallButton.trigger('click');
    await wrapper.findAll('button').find(button => button.text() === 'Remove mod')!.trigger('click');
    expect(wrapper.emitted('installMod')).toHaveLength(1);
    expect(wrapper.emitted('removeMod')).toHaveLength(1);

    await wrapper.setProps({status: gameStatus({mod_available: false})});
    expect(wrapper.text()).toContain('Mod ready');
    expect(wrapper.find('.mod-reinstall').exists()).toBe(false);
    await wrapper.setProps({
      status: gameStatus({mod_deployed: false, mod_outdated: false, mod_removable: false}),
    });
    expect(wrapper.text()).toContain('Install mod');
    await wrapper.findAll('button').find(button => button.text() === 'Install mod')!.trigger('click');
    expect(wrapper.emitted('installMod')).toHaveLength(2);
    await wrapper.setProps({
      status: gameStatus({mod_deployed: false, mod_outdated: true, mod_removable: false}),
    });
    expect(wrapper.text()).toContain('Update mod');
    await wrapper.setProps({
      actionPending: true,
      status: gameStatus({
        mod_deployed: false,
        mod_available: true,
        mod_outdated: true,
        mod_removable: true,
        can_install_mod: false,
        can_remove_mod: false,
      }),
    });
    expect(wrapper.findAll('button').filter(button => ['Update mod', 'Remove mod'].includes(button.text()))
      .every(button => button.attributes('disabled') !== undefined)).toBe(true);
    await wrapper.setProps({status: gameStatus({mod_deployed: false, mod_available: false})});
    expect(wrapper.text()).toContain('Mod unavailable');
  });

  it('renders game and launcher state with the relevant guidance', async () => {
    const wrapper = mount(StatusBar, {props: props()});
    expect(wrapper.text()).toContain('Game not running');

    await wrapper.setProps({status: gameStatus({game_running: true})});
    expect(wrapper.text()).toContain('Game running');
    await wrapper.setProps({status: gameStatus({launcher_running: true})});
    expect(wrapper.text()).toContain('Close the Scopely Launcher');
    await wrapper.setProps({status: gameStatus({launcher_running: true, launcher_started_by_us: true})});
    expect(wrapper.text()).toContain('has been started');
  });

  it('opens available Daystrom updates and hides dismissed ones', async () => {
    const wrapper = mount(StatusBar, {props: props()});
    expect(wrapper.text()).toContain('Daystrom up to date');
    const checkButton = wrapper.get('[aria-label="Check for Daystrom updates"]');
    await checkButton.trigger('click');
    expect(wrapper.emitted('checkUpdate')).toHaveLength(1);

    await wrapper.setProps({update: updateStatus({phase: 'available', version: '0.10.1'})});
    const updateButton = wrapper.findAll('button').find(button => button.text().includes('Daystrom 0.10.1'))!;
    await updateButton.trigger('click');
    expect(wrapper.emitted('openUpdate')).toHaveLength(1);

    await wrapper.setProps({update: updateStatus({phase: 'available', version: '0.10.1', dismissed: true})});
    expect(wrapper.text()).not.toContain('Daystrom 0.10.1 available');
    expect(wrapper.text()).toContain('Daystrom update deferred');
    await wrapper.setProps({updateCheckBusy: true});
    expect(wrapper.get('[aria-label="Check for Daystrom updates"]').attributes('disabled')).toBeDefined();
    await wrapper.setProps({updateCheckBusy: false});
    await wrapper.setProps({update: updateStatus({phase: 'checking'})});
    expect(wrapper.text()).toContain('Checking Daystrom updates');
    await wrapper.setProps({update: updateStatus({phase: 'failed', error: 'update_check_failed'})});
    expect(wrapper.text()).toContain('Daystrom update check failed');
    await wrapper.get('[aria-label="Check for Daystrom updates"]').trigger('click');
    expect(wrapper.emitted('checkUpdate')).toHaveLength(2);
  });

  it('opens a pending mod restore and shows action errors', async () => {
    const wrapper = mount(StatusBar, {
      props: props({
        actionError: 'game_launch_failed',
        rollback: rollbackStatus({mod_restore_pending: true}),
      }),
    });

    expect(wrapper.text()).toContain('STFC could not be started');
    const rollbackButton = wrapper.findAll('button').find(button => button.text().includes('Previous mod'))!;
    await rollbackButton.trigger('click');
    expect(wrapper.emitted('openRollback')).toHaveLength(1);
  });
});
