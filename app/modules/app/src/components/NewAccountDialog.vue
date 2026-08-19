<script setup lang="ts">
const emit = defineEmits<{
  confirm: [];
  cancel: [];
}>();
</script>

<template>
  <div class="new-account-dialog">
    <p>
      Start STFC without an existing Daystrom profile to add a new account?
    </p>
    <p>
      Sign in to the intended Scopely account. Daystrom will detect and add its player profile automatically.
    </p>
    <div class="actions">
      <button @click="emit('confirm')">
        Start new account
      </button>
      <button @click="emit('cancel')">
        Cancel
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
