import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import {mount} from '@vue/test-utils';
import {afterAll, beforeAll, beforeEach, describe, expect, it, vi} from 'vitest';
import AppDialog from '../AppDialog.vue';
import AppHeader from '../AppHeader.vue';
import DeleteAccountDialog from '../DeleteAccountDialog.vue';
import NewAccountDialog from '../NewAccountDialog.vue';
import RollbackDialog from '../RollbackDialog.vue';
import SafetyNoticeDialog from '../SafetyNoticeDialog.vue';
import {releaseNotesForLanguage} from '../updateDialog';
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
    busy: false,
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
    busy: false,
    ...overrides,
  };
}

describe('appHeader', () => {
  it('renders an optional version and exposes draggable window controls', async () => {
    const wrapper = mount(AppHeader, {props: {version: '0.10.0'}});

    expect(wrapper.text()).toContain('0.10.0');
    expect(wrapper.get('.app-header').attributes('data-tauri-drag-region')).toBe('');
    expect(wrapper.get('.app-drag-region').attributes('data-tauri-drag-region')).toBe('');
    await wrapper.get('.settings-button').trigger('click');
    await wrapper.get('.close-button').trigger('click');
    expect(wrapper.emitted('openSettings')).toHaveLength(1);
    expect(wrapper.emitted('closeWindow')).toHaveLength(1);
    expect(wrapper.get('.close-button').classes()).toContain('hover-suppressed');
    await wrapper.get('.close-button').trigger('pointerleave');
    expect(wrapper.get('.close-button').classes()).not.toContain('hover-suppressed');
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
      props: {title: 'Test dialogue'},
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

  it('blocks every implicit close gesture when dismissal is disabled', async () => {
    const wrapper = mount(AppDialog, {
      props: {title: 'Required dialogue', dismissible: false},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });

    expect(wrapper.find('.dialog-close').exists()).toBe(false);
    await wrapper.get('.dialog-shell').trigger('click');
    const cancel = new Event('cancel', {cancelable: true});
    wrapper.get('.dialog-shell').element.dispatchEvent(cancel);

    expect(wrapper.emitted('close')).toBeUndefined();
    expect(cancel.defaultPrevented).toBe(true);
    wrapper.unmount();
  });

  it('leaves Escape to nested keyboard handling when consumed', async () => {
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialogue'},
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
      props: {title: 'Test dialogue'},
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

  it('prefers an explicitly requested initial focus target', () => {
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialogue'},
      slots: {default: '<button autofocus class="preferred">Preferred</button>'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });

    expect(document.activeElement).toBe(wrapper.get('.preferred').element);
    wrapper.unmount();
  });

  it('removes cleanly when no previous focus or open modal remains', () => {
    const activeElement = vi.spyOn(document, 'activeElement', 'get').mockReturnValue(null);
    const wrapper = mount(AppDialog, {
      props: {title: 'Test dialogue'},
      attachTo: document.body,
      global: {stubs: {Teleport: true}},
    });
    activeElement.mockRestore();
    wrapper.get('.dialog-shell').element.removeAttribute('open');

    wrapper.unmount();

    expect(close).not.toHaveBeenCalled();
  });
});

describe('safetyNoticeDialog', () => {
  it('requires explicit understanding before acknowledgement', async () => {
    const wrapper = mount(SafetyNoticeDialog, {
      props: {
        pending: false,
        failed: false,
        acknowledgementRequired: true,
        context: {
          platform: 'windows',
          cleanupPaths: [
            'C:\\Users\\Test\\AppData\\Roaming\\mbur.project-daystrom',
            'C:\\Users\\Test\\AppData\\Local\\mbur.project-daystrom',
          ],
          modLibraryPath: 'C:\\Games\\STFC\\version.dll',
        },
      },
    });
    const button = wrapper.get('.continue-button');

    expect(wrapper.text()).toContain('neither developed nor supported by Scopely');
    expect(wrapper.text()).toContain('C:\\Games\\STFC\\version.dll');
    expect(wrapper.text()).not.toContain('does not leave a mod library');
    expect(button.attributes('disabled')).toBeDefined();
    await wrapper.get('input[type="checkbox"]').setValue(true);
    expect(button.attributes('disabled')).toBeUndefined();
    await button.trigger('click');

    expect(wrapper.emitted('acknowledge')).toHaveLength(1);
  });

  it('renders pending and failed acknowledgement states', async () => {
    const wrapper = mount(SafetyNoticeDialog, {
      props: {
        pending: true,
        failed: true,
        acknowledgementRequired: true,
        context: {
          platform: 'macos',
          cleanupPaths: ['/Users/Test/Library/Logs/mbur.project-daystrom'],
          modLibraryPath: null,
        },
      },
    });

    expect(wrapper.text()).toContain('Saving…');
    expect(wrapper.get('[role="alert"]').text()).toContain('could not be saved');
    expect(wrapper.text()).toContain('/Users/Test/Library/Logs/mbur.project-daystrom');
    expect(wrapper.text()).not.toContain('Remove mod');
    expect(wrapper.get('input').attributes('disabled')).toBeDefined();
    await wrapper.get('.continue-button').trigger('click');
    expect(wrapper.emitted('acknowledge')).toBeUndefined();
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

describe('deleteAccountDialog', () => {
  const profile = {name: 'Test Account', server: 1, stem: '1_TestAccount', primary: false};

  it('requires the exact account name before deleting local data', async () => {
    const wrapper = mount(DeleteAccountDialog, {
      props: {profile, pending: false, error: null},
    });
    const input = wrapper.get('input');
    const deleteButton = wrapper.get('.delete-button');

    expect(wrapper.text()).toContain('does not delete the account from Scopely');
    expect(wrapper.text()).toContain('permanently lose access');
    expect(deleteButton.attributes('disabled')).toBeDefined();
    await input.setValue('Wrong Account');
    expect(deleteButton.attributes('disabled')).toBeDefined();
    await input.setValue('Test Account');
    expect(deleteButton.attributes('disabled')).toBeUndefined();
    await deleteButton.trigger('click');

    expect(wrapper.emitted('confirm')).toHaveLength(1);
  });

  it('blocks cancellation while pending and renders backend errors', async () => {
    const wrapper = mount(DeleteAccountDialog, {
      props: {profile, pending: true, error: 'profile_deletion_failed'},
    });
    const cancel = wrapper.get('button[autofocus]');

    expect(wrapper.text()).toContain('could not be deleted');
    expect(cancel.attributes('disabled')).toBeDefined();
    await cancel.trigger('click');
    expect(wrapper.emitted('cancel')).toBeUndefined();
    await wrapper.setProps({pending: false});
    await cancel.trigger('click');
    expect(wrapper.emitted('cancel')).toHaveLength(1);
  });
});

describe('updateDialog', () => {
  it('renders update details, disabled installation, errors, and actions', async () => {
    const wrapper = mount(UpdateDialog, {
      props: {
        status: updateStatus({
          notes: {de: 'Versionshinweise', en: 'Release notes'},
          can_install: false,
          error: 'update_download_failed',
        }),
        rollbackBusy: false,
      },
    });

    expect(wrapper.get('.release-notes h3').text()).toBe('What\'s new');
    expect(wrapper.text()).toContain('Release notes');
    expect(wrapper.text()).toContain('Installation is disabled');
    expect(wrapper.text()).toContain('Could not download and verify');
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

  it('selects German or English notes and uses English for the Easter-egg locale', () => {
    const notes = {de: 'Versionshinweise', en: 'Release notes'};

    expect(releaseNotesForLanguage(notes, 'de')).toBe('Versionshinweise');
    expect(releaseNotesForLanguage(notes, 'en')).toBe('Release notes');
    expect(releaseNotesForLanguage(notes, 'tlh')).toBe('Release notes');
    expect(releaseNotesForLanguage(null, 'de')).toBeNull();
  });

  it.each([
    ['confirming', null, 'Confirming update'],
    ['retaining_rollback', null, 'Preparing the previous version'],
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
    expect(wrapper.text()).toContain('preparing the previous mod version');
  });

  it('renders recovery actions and backend errors', async () => {
    const wrapper = mount(RollbackDialog, {
      props: {
        status: rollbackStatus({error: 'rollback_restore_failed'}),
        gameRunning: false,
        updateBusy: true,
      },
    });

    expect(wrapper.text()).toContain('0.9.1');
    expect(wrapper.text()).toContain('Could not return');
    expect(wrapper.get('button').attributes('disabled')).toBeDefined();
    await wrapper.setProps({updateBusy: false});
    await wrapper.get('button').trigger('click');
    expect(wrapper.emitted('restore')).toHaveLength(1);

    await wrapper.setProps({status: rollbackStatus({phase: 'failed'})});
    expect(wrapper.find('button').exists()).toBe(true);
    await wrapper.setProps({status: rollbackStatus({phase: 'preparing'})});
    expect(wrapper.text()).toContain('Verifying the previous version');
    await wrapper.setProps({status: rollbackStatus({phase: 'installing'})});
    expect(wrapper.text()).toContain('Returning to the previous version');
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
