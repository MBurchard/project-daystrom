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
  /** Check whether a specific profile is currently running. */
  isProfileRunning: (stem: string) => boolean;
  /** Check whether a running profile is waiting for its first ready UI frame. */
  isProfileStarting: (stem: string) => boolean;
  /** Check whether a profile did not report a ready game UI before its startup deadline. */
  isProfileStartFailed: (stem: string) => boolean;
  /** Check whether UI readiness cannot be observed for a running profile. */
  isProfileStatusUnclear: (stem: string) => boolean;
  /** Register event listeners and load the initial state. Call from onMounted. */
  init: () => void;
  /** Unregister event listeners. Call from onUnmounted. */
  destroy: () => void;
}

const DEFAULT_PROFILE_STATE: ProfileState = {
  profiles: [],
  running_profiles: [],
  starting_profiles: [],
  ready_profiles: [],
  failed_profiles: [],
  unclear_profiles: [],
  external_game_running: false,
  game_origin_pending: false,
  mod_connection_missing: false,
  can_terminate_unconfirmed_start: false,
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
export function useProfileState(): ProfileStateComposable {
  const profiles = ref<ProfileState>({...DEFAULT_PROFILE_STATE});
  let unlisten: (() => void) | null = null;

  const hasProfiles = computed(() => profiles.value.profiles.length > 0);

  const externalGameRunning = computed(() => profiles.value.external_game_running);

  const gameOriginPending = computed(() => profiles.value.game_origin_pending);

  /**
   * Check whether a specific profile is currently running.
   *
   * @param stem - profile stem (e.g. "106_Nabor")
   */
  function isProfileRunning(stem: string): boolean {
    return profiles.value.running_profiles.includes(stem);
  }

  /**
   * Check whether a running profile is waiting for its first ready UI frame.
   *
   * @param stem - profile stem (e.g. "106_Nabor")
   * @returns whether the profile is still starting
   */
  function isProfileStarting(stem: string): boolean {
    return profiles.value.starting_profiles.includes(stem);
  }

  /**
   * Check whether a specific profile failed to report a ready game UI in time.
   *
   * @param stem - profile stem to inspect
   * @returns whether the profile's UI startup deadline expired
   */
  function isProfileStartFailed(stem: string): boolean {
    return profiles.value.failed_profiles.includes(stem);
  }

  /**
   * Check whether UI readiness cannot be observed for a running profile.
   *
   * @param stem - profile stem to inspect
   * @returns whether the profile's game status is unclear
   */
  function isProfileStatusUnclear(stem: string): boolean {
    return profiles.value.unclear_profiles.includes(stem);
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
    isProfileStarting,
    isProfileStartFailed,
    isProfileStatusUnclear,
    init,
    destroy,
  };
}
