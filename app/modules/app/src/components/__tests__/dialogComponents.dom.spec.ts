import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import {mount} from '@vue/test-utils';
import {afterAll, beforeAll, beforeEach, describe, expect, it, vi} from 'vitest';
import AppDialog from '../AppDialog.vue';
import AppHeader from '../AppHeader.vue';
import NewAccountDialog from '../NewAccountDialog.vue';
import RollbackDialog from '../RollbackDialog.vue';
import UpdateDialog from '../UpdateDialog.vue';

/** Build a complete update status for rendering tests. */
function updateStatus(overrides: Partial<DaystromUpdateStatus> = {}): DaystromUpdateStatus {
  return {
    phase: 'available',
    version: '0.10.1',
    notes: null,
    download_progress: null,
    error: null,
    dismissed: false,
    can_install: true,
    ...overrides,
  };
}

/** Build a complete rollback status for rendering tests. */
function rollbackStatus(overrides: Partial<DaystromRollbackStatus> = {}): DaystromRollbackStatus {
  return {
    phase: 'available',
    version: '0.9.1',
    error: null,
    can_restore: true,
    mod_restore_pending: false,
    ...overrides,
  };
}

describe('appHeader', () => {
  it('renders an optional version and opens settings', async () => {
    const wrapper = mount(AppHeader, {props: {version: '0.10.0'}});

    expect(wrapper.text()).toContain('0.10.0');
    await wrapper.get('button').trigger('click');
    expect(wrapper.emitted('openSettings')).toHaveLength(1);
    await wrapper.setProps({version: ''});
    expect(wrapper.find('small').exists()).toBe(false);
  });
});

describe('appDialog', () => {
  const originalShowModal = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal');
  const originalClose = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');

  /** Simulate jsdom's missing native modal-opening behaviour. */
  const showModal = vi.fn(function (this: HTMLDialogElement): void {
    this.setAttribute('open', '');
  });

  /** Simulate jsdom's missing native modal-closing behaviour. */
  const close = vi.fn(function (this: HTMLDialogElement): void {
    this.removeAttribute('open');
  });

  beforeAll(() => {
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {configurable: true, value: showModal});
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {configurable: true, value: close});
  });

  beforeEach(() => {
    showModal.mockClear();
    close.mockClear();
  });

  afterAll(() => {
    if (originalShowModal) {
      Object.defineProperty(HTMLDialogElement.prototype, 'showModal', originalShowModal);
    } else {
      delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;
    }
    if (originalClose) {
      Object.defineProperty(HTMLDialogElement.prototype, 'close', originalClose);
    } else {
      delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).close;
    }
  });

  it('renders its slot and supports all explicit close gestures', async () => {
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialog'},
      slots: {default: '<button class="inside">Inside</button>'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });

    expect(document.body.textContent).toContain('Inside');
    await wrapper.get('.inside').trigger('click');
    expect(wrapper.emitted('close')).toBeUndefined();
    await wrapper.get('.dialog-close').trigger('click');
    await wrapper.get('.dialog-shell').trigger('click');
    const cancel = new Event('cancel', {cancelable: true});
    wrapper.get('.dialog-shell').element.dispatchEvent(cancel);

    expect(wrapper.emitted('close')).toHaveLength(3);
    expect(cancel.defaultPrevented).toBe(true);
    wrapper.unmount();
  });

  it('leaves Escape to nested keyboard handling when consumed', async () => {
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialog'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });
    const consume = (event: KeyboardEvent) => event.preventDefault();
    window.addEventListener('keydown', consume, {once: true});

    document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true, cancelable: true}));

    expect(wrapper.emitted('close')).toBeUndefined();
    wrapper.unmount();
  });

  it('opens modally and restores the previous focus when removed', () => {
    const trigger = document.createElement('button');
    document.body.append(trigger);
    trigger.focus();
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialog'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });

    expect(showModal).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(wrapper.get('.dialog-shell').element);

    wrapper.unmount();

    expect(close).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it('removes cleanly when no previous focus or open modal remains', () => {
    const activeElement = vi.spyOn(document, 'activeElement', 'get').mockReturnValue(null);
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialog'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });
    activeElement.mockRestore();
    wrapper.get('.dialog-shell').element.removeAttribute('open');

    wrapper.unmount();

    expect(close).not.toHaveBeenCalled();
  });
});

describe('newAccountDialog', () => {
  it('requires an explicit confirmation or cancellation', async () => {
    const wrapper = mount(NewAccountDialog);
    const buttons = wrapper.findAll('button');

    await buttons[0]!.trigger('click');
    await buttons[1]!.trigger('click');

    expect(wrapper.emitted('confirm')).toHaveLength(1);
    expect(wrapper.emitted('cancel')).toHaveLength(1);
  });
});

describe('updateDialog', () => {
  it('renders update details, disabled installation, errors, and actions', async () => {
    const wrapper = mount(UpdateDialog, {
      props: {
        status: updateStatus({notes: 'Release notes', can_install: false, error: 'Download failed'}),
        rollbackBusy: false,
      },
    });

    expect(wrapper.text()).toContain('Release notes');
    expect(wrapper.text()).toContain('Installation is disabled');
    expect(wrapper.text()).toContain('Download failed');
    expect(wrapper.get('button').attributes('disabled')).toBeDefined();

    await wrapper.setProps({status: updateStatus(), rollbackBusy: true});
    expect(wrapper.get('button').attributes('disabled')).toBeDefined();
    await wrapper.setProps({rollbackBusy: false});
    const buttons = wrapper.findAll('button');
    await buttons[0]!.trigger('click');
    await buttons[1]!.trigger('click');
    expect(wrapper.emitted('install')).toHaveLength(1);
    expect(wrapper.emitted('later')).toHaveLength(1);
  });

  it.each([
    ['confirming', null, 'Confirming update'],
    ['retaining_rollback', null, 'Preparing rollback package'],
    ['retaining_rollback', 25, '25%'],
    ['downloading', null, 'Downloading and verifying update'],
    ['downloading', 75, '75%'],
    ['installing', null, 'Installing update and restarting Daystrom'],
  ] as const)('renders the %s phase', (phase, progress, expected) => {
    const wrapper = mount(UpdateDialog, {
      props: {
        status: updateStatus({phase, download_progress: progress}),
        rollbackBusy: false,
      },
    });

    expect(wrapper.text()).toContain(expected);
    expect(wrapper.find('progress').exists()).toBe(progress !== null);
  });
});

describe('rollbackDialog', () => {
  it('explains a pending mod restore for running and stopped games', async () => {
    const wrapper = mount(RollbackDialog, {
      props: {
        status: rollbackStatus({mod_restore_pending: true}),
        gameRunning: true,
        updateBusy: false,
      },
    });

    expect(wrapper.text()).toContain('Close STFC when convenient');
    await wrapper.setProps({gameRunning: false});
    expect(wrapper.text()).toContain('finishing the restored mod');
  });

  it('renders recovery actions and backend errors', async () => {
    const wrapper = mount(RollbackDialog, {
      props: {
        status: rollbackStatus({error: 'Restore failed'}),
        gameRunning: false,
        updateBusy: true,
      },
    });

    expect(wrapper.text()).toContain('0.9.1');
    expect(wrapper.text()).toContain('Restore failed');
    expect(wrapper.get('button').attributes('disabled')).toBeDefined();
    await wrapper.setProps({updateBusy: false});
    await wrapper.get('button').trigger('click');
    expect(wrapper.emitted('restore')).toHaveLength(1);

    await wrapper.setProps({status: rollbackStatus({phase: 'failed'})});
    expect(wrapper.find('button').exists()).toBe(true);
    await wrapper.setProps({status: rollbackStatus({phase: 'preparing'})});
    expect(wrapper.text()).toContain('Verifying rollback package');
    await wrapper.setProps({status: rollbackStatus({phase: 'installing'})});
    expect(wrapper.text()).toContain('Restoring the previous release');
  });

  it('reports when no verified recovery release exists', () => {
    const wrapper = mount(RollbackDialog, {
      props: {
        status: rollbackStatus({phase: 'unavailable', version: null, can_restore: false}),
        gameRunning: false,
        updateBusy: false,
      },
    });

    expect(wrapper.text()).toContain('No verified previous Daystrom release');
  });
});
