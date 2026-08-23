<script setup lang="ts">
import type {ProfileInfo} from '@generated/ProfileInfo';
import {resolveSelectedProfileStem} from '@app/components/accountTabs';
import {useI18n} from '@app/i18n';
import accountsDefaults from '@app/locales/en/accounts.json';
import {INITIAL_PROFILE_STEM, NEW_ACCOUNT_PROFILE_STEM} from '@app/profileProtocol';
import {computed, ref, watch} from 'vue';

const props = defineProps<{
  /** Whether STFC is installed on this system. */
  installed: boolean;
  /** Whether the Daystrom mod is ready for profile launches. */
  modDeployed: boolean;
  /** Whether the backend permits an initial launch without a known profile. */
  canLaunchInitial: boolean;
  /** Whether another game action is currently running. */
  actionPending: boolean;
  /** Whether a game launched outside Daystrom blocks new launches. */
  externalGameRunning: boolean;
  /** Whether Daystrom is still restoring the origin of a running game. */
  gameOriginPending: boolean;
  /** Known player profiles. */
  profiles: ProfileInfo[];
  /** Determine whether one profile is currently running. */
  isProfileRunning: (stem: string) => boolean;
  /** Determine whether one running profile is waiting for its first ready UI frame. */
  isProfileStarting: (stem: string) => boolean;
  /** Determine whether one profile failed to report a ready game UI in time. */
  isProfileStartFailed: (stem: string) => boolean;
  /** Determine whether UI readiness cannot be observed for one profile. */
  isProfileStatusUnclear: (stem: string) => boolean;
}>();
const emit = defineEmits<{
  launch: [profile: string];
  addAccount: [];
  deleteAccount: [profile: ProfileInfo];
}>();

const {t} = useI18n('accounts', accountsDefaults);

const selectedStem = ref<string | null>(null);

const selectedProfile = computed(() => props.profiles.find(profile => profile.stem === selectedStem.value) ?? null);

const launchBlocked = computed(() => props.externalGameRunning || props.gameOriginPending);

watch(() => props.profiles, (profiles) => {
  selectedStem.value = resolveSelectedProfileStem(profiles, selectedStem.value);
}, {immediate: true});

/** Select the account represented by one profile tab. */
function selectProfile(stem: string): void {
  selectedStem.value = stem;
}

/** Select and launch the account represented by one profile tab. */
function launchProfile(stem: string): void {
  selectProfile(stem);
  emit('launch', stem);
}

/**
 * Return the translated launch-state label for one profile.
 *
 * @param stem - profile stem to inspect
 * @returns the launch-state label
 */
function profileLaunchLabel(stem: string): string {
  if (props.isProfileStartFailed(stem)) {
    return t('startFailed');
  }
  if (props.isProfileStatusUnclear(stem)) {
    return t('statusUnclear');
  }
  if (props.isProfileStarting(stem)) {
    return t('starting');
  }
  return t(props.isProfileRunning(stem) ? 'running' : 'start');
}

/**
 * Return the translated launch-state label for the initial account button.
 *
 * @returns the initial launch-state label
 */
function initialLaunchLabel(): string {
  if (props.isProfileStartFailed(INITIAL_PROFILE_STEM)) {
    return t('startFailed');
  }
  if (props.isProfileStatusUnclear(INITIAL_PROFILE_STEM)) {
    return t('statusUnclear');
  }
  if (props.isProfileStarting(INITIAL_PROFILE_STEM)) {
    return t('starting');
  }
  return t(props.isProfileRunning(INITIAL_PROFILE_STEM) ? 'running' : 'startNew');
}
</script>

<template>
  <section class="accounts" aria-labelledby="accounts-heading">
    <h2 id="accounts-heading">
      {{ t('heading') }}
    </h2>

    <div v-if="props.profiles.length > 0"
        class="account-tabs"
        role="group"
        :aria-label="t('heading')">
      <div class="account-tab-list">
        <div v-for="profile in props.profiles"
            :key="profile.stem"
            class="account-tab"
            :class="{ active: selectedStem === profile.stem }"
            role="presentation">
          <button :id="`account-tab-${profile.stem}`"
              class="account-tab-select"
              :aria-pressed="selectedStem === profile.stem"
              :aria-controls="`account-panel-${profile.stem}`"
              @click="selectProfile(profile.stem)">
            {{ profile.name }} · {{ t('server', { server: profile.server }) }}
          </button>
          <button class="account-start"
              :class="{
                running: props.isProfileRunning(profile.stem)
                  && !props.isProfileStarting(profile.stem)
                  && !props.isProfileStartFailed(profile.stem)
                  && !props.isProfileStatusUnclear(profile.stem),
                starting: props.isProfileStarting(profile.stem),
                failed: props.isProfileStartFailed(profile.stem),
                unclear: props.isProfileStatusUnclear(profile.stem),
              }"
              :disabled="!props.modDeployed
                || props.actionPending
                || props.isProfileRunning(profile.stem)
                || launchBlocked"
              @click="launchProfile(profile.stem)">
            <span v-if="props.isProfileRunning(profile.stem)"
                class="running-indicator"
                :class="{
                  starting: props.isProfileStarting(profile.stem),
                  failed: props.isProfileStartFailed(profile.stem),
                  unclear: props.isProfileStatusUnclear(profile.stem),
                }"
                aria-hidden="true" />
            {{ profileLaunchLabel(profile.stem) }}
          </button>
        </div>
      </div>
      <button class="account-tab add-account"
          :title="t('add')"
          :aria-label="t('add')"
          :disabled="!props.modDeployed
            || props.actionPending
            || props.isProfileRunning(NEW_ACCOUNT_PROFILE_STEM)
            || launchBlocked"
          @click="emit('addAccount')">
        +
      </button>
    </div>

    <div v-if="props.gameOriginPending" class="account-message">
      {{ t('reconnecting') }}
    </div>
    <div v-else-if="props.externalGameRunning" class="account-message">
      {{ t('external') }}
    </div>

    <div v-if="selectedProfile"
        :id="`account-panel-${selectedProfile.stem}`"
        class="account-panel"
        role="region"
        :aria-labelledby="`account-tab-${selectedProfile.stem}`">
      <div class="account-summary">
        <h3>{{ selectedProfile.name }}</h3>
        <p>{{ t('server', { server: selectedProfile.server }) }}</p>
      </div>
      <section class="account-danger" :aria-labelledby="`account-danger-${selectedProfile.stem}`">
        <h4 :id="`account-danger-${selectedProfile.stem}`">
          {{ t('removeHeading') }}
        </h4>
        <p>{{ t('removeDescription') }}</p>
        <button class="account-delete"
            :disabled="props.actionPending
              || props.isProfileRunning(selectedProfile.stem)
              || launchBlocked"
            @click="emit('deleteAccount', selectedProfile)">
          {{ t('removeLocal') }}
        </button>
        <small v-if="props.isProfileRunning(selectedProfile.stem) || launchBlocked">
          {{ t('closeGameToRemove') }}
        </small>
      </section>
    </div>

    <div v-else class="account-panel empty-account">
      <div>
        <h3>{{ props.installed ? t('none') : t('notInstalled') }}</h3>
        <p v-if="props.installed">
          {{ t('first') }}
        </p>
      </div>
      <button v-if="props.installed"
          :disabled="!props.canLaunchInitial
            || props.actionPending
            || props.isProfileRunning(INITIAL_PROFILE_STEM)
            || launchBlocked"
          @click="emit('launch', INITIAL_PROFILE_STEM)">
        {{ initialLaunchLabel() }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.accounts {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  margin-top: 0.75rem;
}

.accounts h2 {
  margin: 0 0 0.75rem;
}

.account-tabs {
  display: flex;
  align-items: stretch;
  overflow-x: auto;
}

.account-tab-list {
  display: flex;
  align-items: stretch;
}

.account-tab {
  display: flex;
  align-items: stretch;
  padding: 0;
  border: 1px solid var(--border-control);
  border-bottom: 0;
  border-radius: 0;
  background: var(--surface-muted);
  color: inherit;
  white-space: nowrap;
}

.account-tab-list .account-tab + .account-tab {
  margin-left: -1px;
}

.account-tab-list .account-tab:first-child {
  border-radius: 0.35rem 0 0;
}

.account-tab.active {
  background: var(--surface-hover);
  font-weight: 600;
}

.account-tab-select,
.account-start {
  border: 0;
  background: none;
  color: inherit;
  cursor: pointer;
}

.account-tab-select {
  padding: 0.55rem 0.8rem;
}

.account-start {
  display: flex;
  align-self: center;
  align-items: center;
  gap: 0.35rem;
  margin: 0.2rem 0.4rem 0.2rem 0;
  padding: 0.2rem 0.7rem;
  border: 1px solid var(--action-primary-border);
  border-radius: 999px;
  background: var(--action-primary-surface);
  box-shadow: var(--action-primary-shadow-compact);
  color: var(--text-on-emphasis);
  font-size: 0.8rem;
  font-weight: 400;
  text-shadow: var(--action-primary-text-shadow);
}

.account-start:disabled {
  cursor: default;
  filter: saturate(0.45);
  opacity: 0.4;
}

.account-start.running {
  filter: none;
  opacity: 1;
}

.account-start.starting {
  filter: none;
  opacity: 0.8;
}

.account-start.unclear {
  color: var(--status-warning);
  filter: none;
  opacity: 1;
}

.account-start.failed {
  color: var(--status-danger);
  filter: none;
  opacity: 1;
}

.account-start:hover:not(:disabled) {
  border-color: var(--action-primary-border-hover);
  background: var(--action-primary-surface-hover);
  box-shadow: var(--action-primary-shadow-hover);
}

.add-account {
  display: grid;
  min-width: 2.5rem;
  padding: 0 0.75rem;
  place-items: center;
  font-size: 1rem;
  font-weight: 700;
  line-height: 1;
  cursor: pointer;
  margin-left: -1px;
  border-radius: 0 0.35rem 0 0;
}

.add-account:disabled {
  cursor: default;
  opacity: 0.45;
}

.running-indicator {
  width: 0.45rem;
  height: 0.45rem;
  border-radius: 50%;
  background: var(--activity-indicator);
  box-shadow: var(--activity-shadow);
}

.running-indicator.starting {
  background: var(--status-warning);
  box-shadow: none;
}

.running-indicator.unclear {
  background: var(--status-warning);
  box-shadow: none;
}

.running-indicator.failed {
  background: var(--status-danger);
  box-shadow: none;
}

.account-panel {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: stretch;
  gap: 1rem;
  min-height: 5rem;
  padding: 1rem;
  overflow-y: auto;
  border: 1px solid var(--border-control);
}

.account-summary {
  min-height: 3rem;
}

.account-panel h3,
.account-panel p {
  margin: 0;
}

.account-danger {
  margin-top: auto;
  padding: 1rem;
  border: 1px solid var(--danger-border);
  border-radius: 0.4rem;
  background: var(--danger-surface-subtle);
}

.account-danger h4 {
  margin: 0;
  color: var(--danger-text);
  text-transform: uppercase;
}

.account-danger p {
  margin: 0.45rem 0 0.8rem;
  opacity: 1;
}

.account-danger small {
  display: block;
  margin-top: 0.5rem;
}

.account-delete {
  padding: 0.4rem 0.75rem;
  border: 1px solid var(--danger-text);
  border-radius: 0.3rem;
  background: var(--surface-canvas);
  color: var(--danger-text);
  font-weight: 600;
}

.account-delete:disabled {
  opacity: 0.45;
}

.account-delete:hover:not(:disabled) {
  background: var(--danger-surface);
  color: var(--text-on-emphasis);
}

.account-panel p {
  margin-top: 0.25rem;
  opacity: 0.65;
}

.empty-account button {
  margin-left: auto;
  padding: 0.45rem 1.25rem;
}

.account-message {
  padding: 0.65rem 0.8rem;
  border: 1px solid var(--status-info);
  color: var(--status-info);
}
</style>
