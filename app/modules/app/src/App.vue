<script setup lang="ts">
import type {GameStatus} from '@generated/GameStatus';
import {getLogger} from '@app/log';
import {invoke} from '@tauri-apps/api/core';
import {onMounted, ref} from 'vue';

const log = getLogger('App');

const status = ref<GameStatus | null>(null);
const error = ref<string | null>(null);

onMounted(async () => {
  log.debug('App.vue mounted');
  try {
    status.value = await invoke<GameStatus>('get_game_status');
    log.debug('Game status received:', status.value.installed ? 'installed' : 'not found');
  } catch (err) {
    error.value = String(err);
    log.error(`Failed to get game status: ${err}`);
  }
});
</script>

<template>
  <main>
    <h1>Skynet</h1>

    <p v-if="error">
      Failed to load game status: {{ error }}
    </p>

    <section v-else-if="status">
      <h2>Game Detection</h2>
      <dl>
        <dt>STFC installed</dt>
        <dd>{{ status.installed ? 'Yes' : 'No' }}</dd>

        <template v-if="status.installed">
          <dt>Install directory</dt>
          <dd>{{ status.install_dir }}</dd>

          <dt>Executable</dt>
          <dd>{{ status.executable }}</dd>

          <dt>Entitlements</dt>
          <dd v-if="status.entitlements_ok">
            All granted
          </dd>
          <dd v-else>
            Missing: {{ status.missing_entitlements.join(', ') }}
          </dd>
        </template>
      </dl>
    </section>

    <p v-else>
      Loading...
    </p>
  </main>
</template>
