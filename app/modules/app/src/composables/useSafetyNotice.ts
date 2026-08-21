import type {SafetyNoticeContext} from '@generated/SafetyNoticeContext';
import type {Ref} from 'vue';
import {
  acknowledgeSafetyNotice,
  getSafetyNoticeContext,
  isSafetyNoticeRequired,
} from '@app/commands/safetyNotice';
import {getLogger} from '@app/log';
import {readonly, ref} from 'vue';

const log = getLogger('SafetyNotice');

/** Reactive state and actions for the backend-owned safety-notice policy. */
export interface SafetyNoticeState {
  /** Whether the notice currently blocks normal application interaction. */
  required: Readonly<Ref<boolean>>;
  /** Whether acknowledgement is currently being persisted. */
  pending: Readonly<Ref<boolean>>;
  /** Whether the latest acknowledgement attempt failed. */
  failed: Readonly<Ref<boolean>>;
  /** Platform-specific paths shown in the notice. */
  context: Readonly<Ref<SafetyNoticeContext | null>>;
  /** Load the authoritative requirement from the backend. */
  init: () => void;
  /** Acknowledge the current notice and clear it only after backend success. */
  acknowledge: () => void;
}

/**
 * Connect the safety-notice dialogue to backend-owned acknowledgement state.
 *
 * @returns Reactive display state and the narrow acknowledgement action.
 */
export function useSafetyNotice(): SafetyNoticeState {
  const required = ref(false);
  const pending = ref(false);
  const failed = ref(false);
  const context = ref<SafetyNoticeContext | null>(null);

  /** Load whether the current notice still requires acknowledgement. */
  function init(): void {
    isSafetyNoticeRequired()
      .then((value) => {
        required.value = value;
      })
      .catch((reason) => {
        required.value = true;
        log.error('Failed to determine whether the safety notice is required:', reason);
      });

    getSafetyNoticeContext()
      .then((loadedContext) => {
        context.value = loadedContext;
      })
      .catch(reason => log.error('Failed to load the safety notice context:', reason));
  }

  /** Persist acknowledgement and keep the notice visible when the command fails. */
  function acknowledge(): void {
    if (pending.value) {
      return;
    }
    pending.value = true;
    failed.value = false;
    acknowledgeSafetyNotice()
      .then(() => {
        required.value = false;
        pending.value = false;
      })
      .catch((reason) => {
        pending.value = false;
        failed.value = true;
        log.error('Failed to acknowledge the safety notice:', reason);
      });
  }

  return {
    required: readonly(required),
    pending: readonly(pending),
    failed: readonly(failed),
    context,
    init,
    acknowledge,
  };
}
