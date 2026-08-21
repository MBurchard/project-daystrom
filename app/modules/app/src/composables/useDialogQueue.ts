import type {ComputedRef} from 'vue';
import {computed, shallowRef} from 'vue';

/** Shared dialogue priority levels, leaving room for domain-specific values between them. */
export const DIALOG_PRIORITY = {
  low: 0,
  normal: 100,
  high: 200,
  critical: 300,
} as const;

/** One dialogue waiting for or occupying the application dialogue layer. */
export interface DialogRequest<Dialog extends string> {
  /** Stable identity used for deduplication and exact closure. */
  id: Dialog;
  /** Higher values are promoted before lower values. */
  priority: number;
  /** Whether this dialogue may interrupt a lower-priority dialogue. */
  canInterrupt?: boolean;
  /** Return whether a higher-priority dialogue may interrupt this dialogue now. */
  isInterruptible?: () => boolean;
  /** Whether the condition represented by this dialogue still applies. */
  isValid?: () => boolean;
}

/** Application-level dialogue coordinator. */
export interface DialogQueue<Dialog extends string> {
  /** Dialogue currently occupying the single visible dialogue layer. */
  activeDialog: ComputedRef<Dialog | null>;
  /** Request a dialogue unless the same identity is already active or queued. */
  request: (request: DialogRequest<Dialog>) => boolean;
  /** Close only the named active dialogue and promote the next valid request. */
  close: (id: Dialog) => boolean;
  /** Remove a named dialogue whether it is active or queued. */
  cancel: (id: Dialog) => boolean;
}

interface QueuedDialog<Dialog extends string> extends DialogRequest<Dialog> {
  sequence: number;
}

/** Coordinate one visible dialogue with priority-aware queuing and safe interruption. */
export function useDialogQueue<Dialog extends string>(): DialogQueue<Dialog> {
  const active = shallowRef<QueuedDialog<Dialog> | null>(null);
  const pending: QueuedDialog<Dialog>[] = [];
  let sequence = 0;

  const activeDialog = computed(() => active.value?.id ?? null);

  /** Return whether a queued request still represents a current condition. */
  function isValid(request: QueuedDialog<Dialog>): boolean {
    return request.isValid?.() ?? true;
  }

  /** Promote the highest-priority valid request, preserving FIFO order within a priority. */
  function promote(): void {
    pending.sort((left, right) => right.priority - left.priority || left.sequence - right.sequence);
    let next = pending.shift();
    while (next && !isValid(next)) {
      next = pending.shift();
    }
    active.value = next ?? null;
  }

  /** Request a dialogue unless the same identity is already active or queued. */
  function request(dialog: DialogRequest<Dialog>): boolean {
    if (dialog.isValid?.() === false ||
      active.value?.id === dialog.id ||
      pending.some(candidate => candidate.id === dialog.id)) {
      return false;
    }

    const queued = {...dialog, sequence: sequence++};
    const current = active.value;
    if (!current) {
      active.value = queued;
      return true;
    }

    const mayInterrupt = dialog.canInterrupt === true &&
      dialog.priority > current.priority &&
      (current.isInterruptible?.() ?? true);
    if (mayInterrupt) {
      pending.push(current);
      active.value = queued;
      return true;
    }

    pending.push(queued);
    return true;
  }

  /** Close only the named active dialogue and promote the next valid request. */
  function close(id: Dialog): boolean {
    if (active.value?.id !== id) {
      return false;
    }
    active.value = null;
    promote();
    return true;
  }

  /** Remove a named dialogue whether it is active or queued. */
  function cancel(id: Dialog): boolean {
    if (active.value?.id === id) {
      active.value = null;
      promote();
      return true;
    }

    const index = pending.findIndex(candidate => candidate.id === id);
    if (index < 0) {
      return false;
    }
    pending.splice(index, 1);
    return true;
  }

  return {activeDialog, request, close, cancel};
}
