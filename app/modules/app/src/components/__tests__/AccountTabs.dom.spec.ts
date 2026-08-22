import type {ProfileInfo} from '@generated/ProfileInfo';
import {INITIAL_PROFILE_STEM} from '@app/profileProtocol';
import {mount} from '@vue/test-utils';
import {describe, expect, it, vi} from 'vitest';
import AccountTabs from '../AccountTabs.vue';

const PROFILES: ProfileInfo[] = [
  {name: 'Test Alpha', server: 1, stem: '1_TestAlpha', primary: true},
  {name: 'Test Beta', server: 2, stem: '2_TestBeta', primary: false},
];

/** Build the complete default prop set for account-tab rendering tests. */
function props(overrides: Partial<InstanceType<typeof AccountTabs>['$props']> = {}) {
  return {
    installed: true,
    modDeployed: true,
    canLaunchInitial: true,
    actionPending: false,
    externalGameRunning: false,
    gameOriginPending: false,
    profiles: PROFILES,
    isProfileRunning: () => false,
    isProfileStarting: () => false,
    ...overrides,
  };
}

describe('accountTabs', () => {
  it('selects accounts and launches from their tabs', async () => {
    const wrapper = mount(AccountTabs, {props: props()});
    const tabs = wrapper.findAll('.account-tab-select');

    expect(tabs[0]!.attributes('aria-pressed')).toBe('true');
    expect(wrapper.text()).toContain('Test Alpha');
    await tabs[1]!.trigger('click');
    expect(tabs[1]!.attributes('aria-pressed')).toBe('true');
    expect(wrapper.get('[role="region"]').text()).toContain('Server 2');

    await wrapper.findAll('.account-start')[0]!.trigger('click');
    expect(wrapper.emitted('launch')).toEqual([['1_TestAlpha']]);
    expect(wrapper.findAll('.account-tab-select')[0]!.attributes('aria-pressed')).toBe('true');
  });

  it('requests confirmation before adding an account', async () => {
    const wrapper = mount(AccountTabs, {props: props()});

    await wrapper.get('.add-account').trigger('click');

    expect(wrapper.emitted('addAccount')).toHaveLength(1);
  });

  it('offers local deletion for the selected account', async () => {
    const wrapper = mount(AccountTabs, {props: props()});

    expect(wrapper.text()).toContain('Delete this account\'s local profile');
    await wrapper.get('.account-delete').trigger('click');

    expect(wrapper.emitted('deleteAccount')).toEqual([[PROFILES[0]]]);
  });

  it('shows running accounts and disables their launch action', () => {
    const isRunning = vi.fn((stem: string) => stem === '2_TestBeta');
    const wrapper = mount(AccountTabs, {props: props({isProfileRunning: isRunning})});

    expect(wrapper.findAll('.running-indicator')).toHaveLength(1);
    expect(wrapper.findAll('.account-start')[0]!.attributes('disabled')).toBeUndefined();
    expect(wrapper.findAll('.account-start')[1]!.attributes('disabled')).toBeDefined();
    expect(wrapper.findAll('.account-start')[0]!.text()).toBe('Start');
    expect(wrapper.findAll('.account-start')[1]!.text()).toBe('Running');
    expect(wrapper.findAll('.account-start')[1]!.classes()).toContain('running');
  });

  it('shows the intermediate state until the mod handshake is confirmed', () => {
    const wrapper = mount(AccountTabs, {
      props: props({
        isProfileRunning: stem => stem === '1_TestAlpha',
        isProfileStarting: stem => stem === '1_TestAlpha',
      }),
    });

    expect(wrapper.findAll('.account-start')[0]!.text()).toBe('Starting…');
    expect(wrapper.findAll('.account-start')[0]!.classes()).toContain('starting');
    expect(wrapper.find('.running-indicator.starting').exists()).toBe(true);
    expect(wrapper.findAll('.account-start')[0]!.attributes('disabled')).toBeDefined();
  });

  it.each([
    {actionPending: true},
    {externalGameRunning: true},
    {gameOriginPending: true},
  ])('blocks local deletion during active game work', (blocked) => {
    const wrapper = mount(AccountTabs, {props: props(blocked)});

    expect(wrapper.get('.account-delete').attributes('disabled')).toBeDefined();
  });

  it('blocks local deletion while the selected profile is running', () => {
    const wrapper = mount(AccountTabs, {
      props: props({isProfileRunning: stem => stem === '1_TestAlpha'}),
    });

    expect(wrapper.get('.account-delete').attributes('disabled')).toBeDefined();
    expect(wrapper.text()).toContain('Close STFC before deleting');
  });

  it.each([
    {modDeployed: false},
    {actionPending: true},
    {externalGameRunning: true},
    {gameOriginPending: true},
  ])('blocks account launches for $modDeployed$actionPending$externalGameRunning$gameOriginPending', (blocked) => {
    const wrapper = mount(AccountTabs, {props: props(blocked)});

    expect(wrapper.findAll('.account-start').every(button => button.attributes('disabled') !== undefined)).toBe(true);
    expect(wrapper.get('.add-account').attributes('disabled')).toBeDefined();
  });

  it('keeps the selection valid when profiles change', async () => {
    const wrapper = mount(AccountTabs, {props: props()});
    await wrapper.findAll('.account-tab-select')[1]!.trigger('click');

    await wrapper.setProps({profiles: [PROFILES[0]!]});

    expect(wrapper.get('.account-tab-select').attributes('aria-pressed')).toBe('true');
    expect(wrapper.get('[role="region"]').text()).toContain('Test Alpha');
  });

  it('exposes account actions as a labelled button group', () => {
    const wrapper = mount(AccountTabs, {props: props()});

    expect(wrapper.get('[role="group"]').attributes('aria-label')).toBe('Accounts');
    expect(wrapper.find('[role="tablist"]').exists()).toBe(false);
    expect(wrapper.find('[role="tab"]').exists()).toBe(false);
  });

  it('starts the first account when no profile is known', async () => {
    const wrapper = mount(AccountTabs, {props: props({profiles: []})});

    expect(wrapper.text()).toContain('No account detected');
    await wrapper.get('.empty-account button').trigger('click');
    expect(wrapper.emitted('launch')).toEqual([[INITIAL_PROFILE_STEM]]);
  });

  it('labels the initial account launch as starting during its grace period', () => {
    const wrapper = mount(AccountTabs, {
      props: props({
        profiles: [],
        isProfileRunning: stem => stem === INITIAL_PROFILE_STEM,
        isProfileStarting: stem => stem === INITIAL_PROFILE_STEM,
      }),
    });

    expect(wrapper.get('.empty-account button').text()).toBe('Starting…');
  });

  it('stops labelling an unconfirmed initial launch as starting after its grace period', () => {
    const wrapper = mount(AccountTabs, {
      props: props({
        profiles: [],
        isProfileRunning: stem => stem === INITIAL_PROFILE_STEM,
      }),
    });

    expect(wrapper.get('.empty-account button').text()).toBe('Running');
    expect(wrapper.get('.empty-account button').attributes('disabled')).toBeDefined();
  });

  it.each([
    {canLaunchInitial: false},
    {actionPending: true},
    {externalGameRunning: true},
    {gameOriginPending: true},
  ])('blocks the initial launch when unavailable', (blocked) => {
    const wrapper = mount(AccountTabs, {props: props({profiles: [], ...blocked})});

    expect(wrapper.get('.empty-account button').attributes('disabled')).toBeDefined();
  });

  it('reports a missing installation without offering a launch', () => {
    const wrapper = mount(AccountTabs, {props: props({installed: false, profiles: []})});

    expect(wrapper.text()).toContain('STFC is not installed');
    expect(wrapper.find('.empty-account button').exists()).toBe(false);
  });

  it('prioritises reconnection over the external-game warning', async () => {
    const wrapper = mount(AccountTabs, {
      props: props({externalGameRunning: true, gameOriginPending: true}),
    });

    expect(wrapper.text()).toContain('Reconnecting to the running game');
    expect(wrapper.text()).not.toContain('started externally');
    await wrapper.setProps({gameOriginPending: false});
    expect(wrapper.text()).toContain('started externally');
  });
});
