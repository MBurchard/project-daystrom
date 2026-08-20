<script setup lang="ts">
import type {ProfileInfo} from '@generated/ProfileInfo';
import {resolveSelectedProfileStem} from '@app/components/accountTabs';
import {useI18n} from '@app/i18n';
import accountsDefaults from '@app/locales/en/accounts.json';
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
              :class="{ running: props.isProfileRunning(profile.stem) }"
              :disabled="!props.modDeployed
                || props.actionPending
                || props.isProfileRunning(profile.stem)
                || launchBlocked"
              @click="launchProfile(profile.stem)">
            <span v-if="props.isProfileRunning(profile.stem)" class="running-indicator" aria-hidden="true" />
            {{ t(props.isProfileRunning(profile.stem) ? 'running' : 'start') }}
          </button>
        </div>
      </div>
      <button class="account-tab add-account"
          :title="t('add')"
          :aria-label="t('add')"
          :disabled="!props.modDeployed || props.actionPending || launchBlocked"
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
          :disabled="!props.canLaunchInitial || props.actionPending || launchBlocked"
          @click="emit('launch', 'initial')">
        {{ t('startNew') }}
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
  border: 1px solid rgb(127 127 127 / 40%);
  border-bottom: 0;
  border-radius: 0;
  background: rgb(127 127 127 / 10%);
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
  background: rgb(127 127 127 / 22%);
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
  border: 1px solid #5cddff;
  border-radius: 999px;
  background: linear-gradient(180deg, #159fe8 0%, #0769c8 100%);
  box-shadow:
    0 0 0.35rem rgb(0 183 255 / 70%),
    inset 0 0 0.3rem rgb(116 231 255 / 55%);
  color: #fff;
  font-size: 0.8rem;
  font-weight: 400;
  text-shadow: 0 1px 1px rgb(0 29 75 / 70%);
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

.account-start:hover:not(:disabled) {
  border-color: #b4f4ff;
  background: linear-gradient(180deg, #25c8f5 0%, #0877dc 100%);
  box-shadow:
    0 0 0.55rem rgb(0 203 255 / 90%),
    inset 0 0 0.35rem rgb(184 246 255 / 70%);
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
  background: #57e36e;
  box-shadow: 0 0 0.35rem rgb(87 227 110 / 90%);
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
  border: 1px solid rgb(127 127 127 / 40%);
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
  border: 1px solid #d1242f;
  border-radius: 0.4rem;
  background: rgb(209 36 47 / 6%);
}

.account-danger h4 {
  margin: 0;
  color: #b4232c;
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
  border: 1px solid #b4232c;
  border-radius: 0.3rem;
  background: Canvas;
  color: #b4232c;
  font-weight: 600;
}

.account-delete:disabled {
  opacity: 0.45;
}

.account-delete:hover:not(:disabled) {
  background: #cf222e;
  color: #fff;
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
  border: 1px solid #2196f3;
  color: #2196f3;
}
</style>
