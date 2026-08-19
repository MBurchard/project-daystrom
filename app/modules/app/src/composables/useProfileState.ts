import type {ProfileState} from '@generated/ProfileState';
import type {Ref} from 'vue';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import {computed, ref} from 'vue';

const log = getLogger('Profiles');

// ---- Public Interface -----------------------------------------------------------

export interface ProfileStateComposable {
  /** All detected profiles. */
  profiles: Readonly<Ref<ProfileState>>;
  /** Whether at least one profile exists. */
  hasProfiles: Readonly<Ref<boolean>>;
  /** Whether a game is running, that was not started by Daystrom. */
  externalGameRunning: Readonly<Ref<boolean>>;
  /** Whether Daystrom is waiting for a running game to restore its launch identity. */
  gameOriginPending: Readonly<Ref<boolean>>;
  /** Check whether a specific profile is currently running or recently launched. */
  isProfileRunning: (stem: string) => boolean;
  /** Mark a profile as recently launched (30s cooldown). */
  markLaunched: (stem: string) => void;
  /** Register event listeners and load the initial state. Call from onMounted. */
  init: () => void;
  /** Unregister event listeners. Call from onUnmounted. */
  destroy: () => void;
}

const DEFAULT_PROFILE_STATE: ProfileState = {
  profiles: [],
  running_profiles: [],
  external_game_running: false,
  game_origin_pending: false,
};

/**
 * Composable that tracks player profiles detected by the backend.
 *
 * The backend scans the config directory every 60 seconds for profile TOML
 * files created by the mod and tracks which profiles are currently running
 * by PID. Emits `profile-status` events on change.
 *
 * @returns reactive profile state, running checks, and lifecycle functions
 */
/// How long a profile button stays disabled after launch (ms).
const LAUNCH_COOLDOWN_MS = 30_000;

export function useProfileState(): ProfileStateComposable {
  const profiles = ref<ProfileState>({...DEFAULT_PROFILE_STATE});
  let unlisten: (() => void) | null = null;

  /** Profile stems that were recently launched (cooldown period). */
  const pendingLaunches = ref<Set<string>>(new Set());

  const hasProfiles = computed(() => profiles.value.profiles.length > 0);

  const externalGameRunning = computed(() => profiles.value.external_game_running);

  const gameOriginPending = computed(() => profiles.value.game_origin_pending);

  /**
   * Check whether a specific profile is currently running or recently launched.
   *
   * @param stem - profile stem (e.g. "106_Nabor")
   */
  function isProfileRunning(stem: string): boolean {
    return profiles.value.running_profiles.includes(stem) ||
      pendingLaunches.value.has(stem);
  }

  /**
   * Mark a profile as recently launched. The cooldown expires after 30 seconds
   * or when the backend reports the profile as running (whichever comes first).
   *
   * @param stem - profile stem (e.g. "106_Nabor")
   */
  function markLaunched(stem: string): void {
    pendingLaunches.value.add(stem);
    setTimeout(() => {
      pendingLaunches.value.delete(stem);
    }, LAUNCH_COOLDOWN_MS);
  }

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
    initializeProfileState().catch(
      /* v8 ignore next -- initializeProfileState handles every expected failure internally. */
      reason => log.error('Failed to initialize profile state:', reason),
    );
  }

  /**
   * Subscribe before reading the initial snapshot so the backend's first profile scan cannot be missed.
   * An event received during the snapshot request takes precedence over the potentially older snapshot.
   */
  async function initializeProfileState(): Promise<void> {
    let eventReceived = false;

    try {
      unlisten = await listen<ProfileState>('profile-status', (event) => {
        eventReceived = true;
        applyProfileState(event.payload);
      });
    } catch (error) {
      log.error('Failed to listen for profile-status:', error);
    }

    try {
      const state = await invoke<ProfileState>('get_cached_profile_state');
      if (!eventReceived) {
        applyProfileState(state);
      }
    } catch (error) {
      log.error('Failed to get cached profile state:', error);
    }
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
    hasProfiles,
    externalGameRunning,
    gameOriginPending,
    isProfileRunning,
    markLaunched,
    init,
    destroy,
  };
}
