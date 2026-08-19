import type {DaystromRollbackStatus} from '@generated/DaystromRollbackStatus';
import type {Ref} from 'vue';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {ref} from 'vue';

const log = getLogger('DaystromRollback');

const DEFAULT_STATUS: DaystromRollbackStatus = {
  phase: 'unavailable',
  version: null,
  error: null,
  can_restore: false,
  mod_restore_pending: false,
};

/** Reactive rollback state and the narrow restore command exposed by the backend. */
export interface DaystromRollbackState {
  /** Display-safe rollback status computed by the Rust backend. */
  status: Readonly<Ref<DaystromRollbackStatus>>;
  /** Restore the sole verified predecessor release. */
  restore: () => void;
  /** Register the status listener and fetch the current backend snapshot. */
  init: () => void;
  /** Unregister the backend event listener. */
  destroy: () => void;
}

/**
 * Connect display-only frontend state to backend-owned rollback recovery.
 *
 * @returns Reactive rollback state and narrowly scoped user action.
 */
export function useDaystromRollback(): DaystromRollbackState {
  const status = ref<DaystromRollbackStatus>({...DEFAULT_STATUS});
  let unlisten: (() => void) | null = null;

  /** Ask the backend to restore only its verified predecessor package. */
  function restore(): void {
    invoke('restore_previous_daystrom_version')
      .catch(reason => log.error('Failed to request Daystrom rollback:', reason));
  }

  /** Apply one authoritative rollback-status snapshot from the backend. */
  function applyStatus(value: DaystromRollbackStatus): void {
    status.value = value;
  }

  /** Register the event listener before fetching the cached state. */
  function init(): void {
    initialize().catch(
      /* v8 ignore next -- initialize handles every expected failure internally. */
      reason => log.error('Failed to initialize Daystrom rollback state:', reason),
    );
  }

  /** Subscribe first, then fetch the cached snapshot unless a newer event already arrived. */
  async function initialize(): Promise<void> {
    let eventReceived = false;

    try {
      unlisten = await listen<DaystromRollbackStatus>('daystrom-rollback-status', (event) => {
        eventReceived = true;
        applyStatus(event.payload);
      });
    } catch (error) {
      log.error('Failed to listen for daystrom-rollback-status:', error);
    }

    try {
      const cached = await invoke<DaystromRollbackStatus>('get_cached_daystrom_rollback_status');
      if (!eventReceived) {
        applyStatus(cached);
      }
    } catch (error) {
      log.error('Failed to get cached Daystrom rollback status:', error);
    }
  }

  /** Unregister the backend event listener. */
  function destroy(): void {
    unlisten?.();
    unlisten = null;
  }

  return {
    status,
    restore,
    init,
    destroy,
  };
}
