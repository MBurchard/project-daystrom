import {describe, expect, it} from 'vitest';
import {DIALOG_PRIORITY, useDialogQueue} from '../useDialogQueue';

type TestDialog = 'first' | 'second' | 'third' | 'fourth' | 'critical' | 'stale';

describe('useDialogQueue', () => {
  it('opens one dialogue and deduplicates active and queued identities', () => {
    const queue = useDialogQueue<TestDialog>();

    expect(queue.request({id: 'first', priority: DIALOG_PRIORITY.normal})).toBe(true);
    expect(queue.request({id: 'first', priority: DIALOG_PRIORITY.critical})).toBe(false);
    expect(queue.request({id: 'second', priority: DIALOG_PRIORITY.normal})).toBe(true);
    expect(queue.request({id: 'second', priority: DIALOG_PRIORITY.critical})).toBe(false);
    expect(queue.activeDialog.value).toBe('first');
  });

  it('promotes queued dialogues by priority and then request order', () => {
    const queue = useDialogQueue<TestDialog>();

    queue.request({id: 'first', priority: DIALOG_PRIORITY.normal});
    queue.request({id: 'second', priority: DIALOG_PRIORITY.normal});
    queue.request({id: 'third', priority: DIALOG_PRIORITY.normal});
    queue.request({id: 'fourth', priority: DIALOG_PRIORITY.high});

    expect(queue.close('second')).toBe(false);
    expect(queue.close('first')).toBe(true);
    expect(queue.activeDialog.value).toBe('fourth');
    expect(queue.close('fourth')).toBe(true);
    expect(queue.activeDialog.value).toBe('second');
    expect(queue.close('second')).toBe(true);
    expect(queue.activeDialog.value).toBe('third');
    expect(queue.close('third')).toBe(true);
    expect(queue.activeDialog.value).toBeNull();
  });

  it('lets a critical request interrupt and later resume an interruptible dialogue', () => {
    const queue = useDialogQueue<TestDialog>();

    queue.request({id: 'first', priority: DIALOG_PRIORITY.normal});
    expect(queue.request({
      id: 'critical',
      priority: DIALOG_PRIORITY.critical,
      canInterrupt: true,
    })).toBe(true);

    expect(queue.activeDialog.value).toBe('critical');
    queue.close('critical');
    expect(queue.activeDialog.value).toBe('first');
  });

  it('queues interruptions behind a protected dialogue or an equal priority', () => {
    const queue = useDialogQueue<TestDialog>();
    queue.request({id: 'first', priority: DIALOG_PRIORITY.normal, isInterruptible: () => false});
    queue.request({id: 'critical', priority: DIALOG_PRIORITY.critical, canInterrupt: true});
    queue.request({id: 'second', priority: DIALOG_PRIORITY.normal, canInterrupt: true});

    expect(queue.activeDialog.value).toBe('first');
    queue.close('first');
    expect(queue.activeDialog.value).toBe('critical');
  });

  it('drops invalid requests before opening or promotion', () => {
    const queue = useDialogQueue<TestDialog>();
    let stale = false;

    expect(queue.request({
      id: 'stale',
      priority: DIALOG_PRIORITY.high,
      isValid: () => false,
    })).toBe(false);

    queue.request({id: 'first', priority: DIALOG_PRIORITY.normal});
    queue.request({
      id: 'stale',
      priority: DIALOG_PRIORITY.high,
      isValid: () => !stale,
    });
    queue.request({id: 'second', priority: DIALOG_PRIORITY.normal});
    stale = true;
    queue.close('first');

    expect(queue.activeDialog.value).toBe('second');
  });

  it('cancels active, queued, and unknown dialogues safely', () => {
    const queue = useDialogQueue<TestDialog>();

    queue.request({id: 'first', priority: DIALOG_PRIORITY.normal});
    queue.request({id: 'second', priority: DIALOG_PRIORITY.normal});
    queue.request({id: 'third', priority: DIALOG_PRIORITY.normal});

    expect(queue.cancel('second')).toBe(true);
    expect(queue.cancel('second')).toBe(false);
    expect(queue.cancel('first')).toBe(true);
    expect(queue.activeDialog.value).toBe('third');
  });
});
