import {closeLogging} from '@mburchard/bit-log';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';

const SHUTDOWN_REQUESTED_EVENT = 'shutdown-requested';
const COMPLETE_SHUTDOWN_COMMAND = 'complete_shutdown';

let shutdownStarted = false;

/**
 * Flush frontend logging and tell the backend that the coordinated shutdown may continue.
 */
function handleShutdownRequested(): void {
  if (shutdownStarted) {
    return;
  }
  shutdownStarted = true;

  closeLogging()
    .catch(reason => console.error('Failed to close frontend logging', reason))
    .then(() => invoke(COMPLETE_SHUTDOWN_COMMAND))
    .then(() => {
      shutdownStarted = false;
    })
    .catch((reason) => {
      shutdownStarted = false;
      console.error('Failed to complete application shutdown', reason);
    });
}

/**
 * Register the frontend side of the coordinated application shutdown.
 * @returns a promise resolving after the Tauri event listener has been registered
 */
export async function registerLoggingShutdownHandler(): Promise<void> {
  await listen(SHUTDOWN_REQUESTED_EVENT, handleShutdownRequested);
}
