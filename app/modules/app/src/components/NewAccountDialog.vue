<script setup lang="ts">
import {useI18n} from '@app/i18n';
import accountsDefaults from '@app/locales/en/accounts.json';
import globalDefaults from '@app/locales/en/global.json';

const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();
const {t} = useI18n('accounts', {...accountsDefaults, cancel: globalDefaults.cancel});
</script>

<template>
  <div class="new-account-dialog">
    <p>
      {{ t('newQuestion') }}
    </p>
    <p>
      {{ t('newInstructions') }}
    </p>
    <div class="actions">
      <button @click="emit('confirm')">
        {{ t('startNew') }}
      </button>
      <button @click="emit('cancel')">
        {{ t('cancel') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.new-account-dialog > :first-child {
  margin-top: 0;
}

.actions {
  display: flex;
  gap: 0.5rem;
}
</style>
