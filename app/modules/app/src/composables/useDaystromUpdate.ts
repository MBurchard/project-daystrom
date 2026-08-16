import type {DaystromUpdateStatus} from '@generated/DaystromUpdateStatus';
import type {Ref} from 'vue';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {ref} from 'vue';

const log = getLogger('DaystromUpdate');

const DEFAULT_STATUS: DaystromUpdateStatus = {
  phase: 'idle',
  version: null,
  notes: null,
  error: null,
  dismissed: false,
};

/** Reactive application-update state and the narrow commands exposed by the backend. */
export interface DaystromUpdateState {
  /** Display-safe update status computed by the Rust backend. */
  status: Readonly<Ref<DaystromUpdateStatus>>;
  /** Request an immediate application-update check. */
  check: () => void;
  /** Hide the current available-version banner until Daystrom restarts or the user checks manually. */
  dismiss: () => void;
  /** Register the status listener and fetch the current backend snapshot. */
  init: () => void;
  /** Unregister the backend event listener. */
  destroy: () => void;
}

/**
 * Connect the display-only frontend state to backend-owned Daystrom update discovery.
 *
 * @returns Reactive update state and narrowly scoped user actions.
 */
export function useDaystromUpdate(): DaystromUpdateState {
  const status = ref<DaystromUpdateStatus>({...DEFAULT_STATUS});
  let unlisten: (() => void) | null = null;

  /** Request a fresh check without exposing updater configuration to the webview. */
  function check(): void {
    invoke('check_for_daystrom_update')
      .catch(reason => log.error('Failed to request Daystrom update check:', reason));
  }

  /** Ask the backend to dismiss the currently available version for this process. */
  function dismiss(): void {
    invoke('dismiss_daystrom_update')
      .catch(reason => log.error('Failed to dismiss Daystrom update:', reason));
  }

  /** Apply one authoritative update-status snapshot from the backend. */
  function applyStatus(value: DaystromUpdateStatus): void {
    status.value = value;
  }

  /** Register the event listener before fetching the cached state to avoid missing startup checks. */
  function init(): void {
    initialize().catch(reason => log.error('Failed to initialize Daystrom update state:', reason));
  }

  /** Subscribe first, then fetch the cached snapshot unless a newer event already arrived. */
  async function initialize(): Promise<void> {
    let eventReceived = false;

    try {
      unlisten = await listen<DaystromUpdateStatus>('daystrom-update-status', (event) => {
        eventReceived = true;
        applyStatus(event.payload);
      });
    } catch (error) {
      log.error('Failed to listen for daystrom-update-status:', error);
    }

    try {
      const cached = await invoke<DaystromUpdateStatus>('get_cached_daystrom_update_status');
      if (!eventReceived) {
        applyStatus(cached);
      }
    } catch (error) {
      log.error('Failed to get cached Daystrom update status:', error);
    }
  }

  /** Unregister the backend event listener. */
  function destroy(): void {
    unlisten?.();
    unlisten = null;
  }

  return {
    status,
    check,
    dismiss,
    init,
    destroy,
  };
}
