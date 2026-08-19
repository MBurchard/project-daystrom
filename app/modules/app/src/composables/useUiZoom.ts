import type {UiZoomAction} from '@generated/UiZoomAction';
import type {UiZoomState} from '@generated/UiZoomState';
import {changeUiZoom} from '@app/commands/zoom';
import {getLogger} from '@app/log';
import {computed, onBeforeUnmount, onMounted, ref} from 'vue';

const OVERLAY_TIMEOUT_MS = 2000;
const log = getLogger('UiZoom');

/** Connect browser-style zoom input to backend-owned webview zoom state. */
export function useUiZoom() {
  const factor = ref(1);
  const visible = ref(false);
  const percent = computed(() => `${Math.round(factor.value * 100)}%`);
  let hideTimeout: ReturnType<typeof setTimeout> | null = null;

  /** Display one backend-confirmed zoom value and restart the overlay timeout. */
  function showOverlay(state: UiZoomState): void {
    factor.value = state.factor;
    visible.value = true;
    if (hideTimeout !== null) {
      clearTimeout(hideTimeout);
    }
    hideTimeout = setTimeout(() => {
      visible.value = false;
      hideTimeout = null;
    }, OVERLAY_TIMEOUT_MS);
  }

  /** Send one zoom intent to the backend and display its authoritative result. */
  function requestZoom(action: UiZoomAction): void {
    changeUiZoom(action)
      .then(showOverlay)
      .catch(reason => log.error('Failed to change application zoom:', reason));
  }

  /** Convert a browser-style zoom keystroke into backend intent. */
  function handleKeydown(event: KeyboardEvent): void {
    if (!usesZoomModifier(event) || event.altKey) {
      return;
    }

    const action = keyboardZoomAction(event.key);
    if (action === null) {
      return;
    }
    event.preventDefault();
    requestZoom(action);
  }

  /** Convert a modified mouse-wheel movement into backend zoom intent. */
  function handleWheel(event: WheelEvent): void {
    if (!usesZoomModifier(event) || event.altKey || event.deltaY === 0) {
      return;
    }
    event.preventDefault();
    requestZoom(event.deltaY < 0 ? 'increase' : 'decrease');
  }

  onMounted(() => {
    window.addEventListener('keydown', handleKeydown);
    window.addEventListener('wheel', handleWheel, {passive: false});
  });

  onBeforeUnmount(() => {
    window.removeEventListener('keydown', handleKeydown);
    window.removeEventListener('wheel', handleWheel);
    if (hideTimeout !== null) {
      clearTimeout(hideTimeout);
      hideTimeout = null;
    }
  });

  return {percent, visible};
}

/** Determine whether an input event uses the platform-native zoom modifier. */
function usesZoomModifier(event: KeyboardEvent | WheelEvent): boolean {
  const isMac = navigator.platform.toLowerCase().includes('mac');
  return isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
}

/** Resolve one layout-aware keyboard value to a backend zoom action. */
function keyboardZoomAction(key: string): UiZoomAction | null {
  if (key === '+' || key === '=') {
    return 'increase';
  }
  if (key === '-') {
    return 'decrease';
  }
  return key === '0' ? 'reset' : null;
}
