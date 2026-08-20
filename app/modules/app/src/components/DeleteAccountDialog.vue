<script setup lang="ts">
import type {ProfileInfo} from '@generated/ProfileInfo';
import type {UiErrorCode} from '@generated/UiErrorCode';
import {useUiError} from '@app/composables/useUiError';
import {useI18n} from '@app/i18n';
import accountsDefaults from '@app/locales/en/accounts.json';
import globalDefaults from '@app/locales/en/global.json';
import {computed, ref} from 'vue';

const props = defineProps<{
  /** Account whose local profile will be deleted. */
  profile: ProfileInfo;
  /** Whether the backend is currently deleting the local profile. */
  pending: boolean;
  /** Backend-owned deletion error shown after a failed attempt. */
  error: UiErrorCode | null;
}>();

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();

const {t} = useI18n('accounts', accountsDefaults);
const {t: globalText} = useI18n('global', globalDefaults);
const {errorText} = useUiError();
const confirmation = ref('');
const canDelete = computed(() => confirmation.value === props.profile.name && !props.pending);
</script>

<template>
  <div class="delete-account-dialog">
    <p>
      {{ t('deleteLocalOnly', { name: props.profile.name }) }}
    </p>
    <p>
      {{ t('deleteScopely') }}
    </p>
    <div class="access-warning">
      <strong>{{ t('deleteWarningHeading') }}</strong>
      <p>{{ t('deleteUnlinkedWarning') }}</p>
    </div>
    <label for="delete-account-confirmation">
      {{ t('deleteConfirmation', { name: props.profile.name }) }}
    </label>
    <input id="delete-account-confirmation"
        v-model="confirmation"
        :aria-label="t('deleteConfirmationLabel')"
        autocomplete="off"
        spellcheck="false">
    <p v-if="props.error" class="delete-error" role="alert">
      {{ errorText(props.error) }}
    </p>
    <div class="dialog-actions">
      <button autofocus :disabled="props.pending" @click="emit('cancel')">
        {{ globalText('cancel') }}
      </button>
      <button class="delete-button" :disabled="!canDelete" @click="emit('confirm')">
        {{ t('deletePermanent') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.access-warning p {
  margin: 0.4rem 0 0;
}

.delete-account-dialog > p:first-child {
  margin-top: 0;
}

.access-warning {
  margin: 1.25rem 0;
  padding: 1rem;
  border: 1px solid var(--danger-border);
  border-radius: 0.4rem;
  background: var(--danger-surface-dialog);
}

label,
input {
  display: block;
}

label {
  margin-bottom: 0.4rem;
  font-weight: 600;
}

input {
  box-sizing: border-box;
  width: 100%;
  padding: 0.45rem 0.55rem;
}

.delete-error {
  color: var(--danger-border);
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.6rem;
  margin-top: 1.25rem;
}

.delete-button {
  border: 1px solid var(--danger-text);
  border-radius: 0.3rem;
  background: var(--danger-surface);
  color: var(--text-on-emphasis);
  font-weight: 600;
}

.delete-button:disabled {
  opacity: 0.45;
}

.delete-button:hover:not(:disabled) {
  background: var(--danger-surface-hover);
}
</style>
