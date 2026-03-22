import type {ProfileState} from '@generated/ProfileState';
import type {Ref} from 'vue';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {computed, ref} from 'vue';

// ---- Public Interface -----------------------------------------------------------

export interface ProfileStateComposable {
  /** All detected profiles. */
  profiles: Readonly<Ref<ProfileState>>;
  /** Label for the launch button (e.g. "Nabor (Server 106)" or "Launch Game"). */
  launchLabel: Readonly<Ref<string>>;
  /** Register event listeners and load the initial state. Call from onMounted. */
  init: () => void;
  /** Unregister event listeners. Call from onUnmounted. */
  destroy: () => void;
}

const DEFAULT_PROFILE_STATE: ProfileState = {
  profiles: [],
};

/**
 * Composable that tracks player profiles detected by the backend.
 *
 * The backend scans the config directory every 60 seconds for profile TOML files created by the mod and emits
 * `profile-status` events on change.
 *
 * @returns reactive profile state and a computed launch button label
 */
export function useProfileState(): ProfileStateComposable {
  const profiles = ref<ProfileState>({...DEFAULT_PROFILE_STATE});
  let unlisten: (() => void) | null = null;

  /**
   * Compute a human-readable label for the launch button.
   *
   * - No profiles: "Launch Game"
   * - One profile: "{name} (Server {server})"
   */
  const launchLabel = computed(() => {
    const list = profiles.value.profiles;
    if (list.length === 0) {
      return 'Launch Game';
    }
    const p = list[0]!;
    return `${p.name} (Server ${p.server})`;
  });

  /**
   * Apply a profile state update from the backend.
   */
  function applyProfileState(state: ProfileState): void {
    profiles.value = state;
  }

  /**
   * Register the event listener and fetch the initial state.
   */
  function init(): void {
    listen<ProfileState>('profile-status', (event) => {
      applyProfileState(event.payload);
    }).then((fn) => {
      unlisten = fn;
    }).catch((err: unknown) => {
      console.error('Failed to listen for profile-status:', err);
    });

    invoke<ProfileState>('get_cached_profile_state').then((state) => {
      applyProfileState(state);
    }).catch((err: unknown) => {
      console.error('Failed to get cached profile state:', err);
    });
  }

  /**
   * Unregister the event listener.
   */
  function destroy(): void {
    unlisten?.();
    unlisten = null;
  }

  return {
    profiles,
    launchLabel,
    init,
    destroy,
  };
}
