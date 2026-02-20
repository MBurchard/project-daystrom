import {createLogger} from '@app/log';
import {attachConsole} from '@tauri-apps/plugin-log';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';

const log = createLogger('Main');

function initApp() {
  const app = createApp(App);
  app.use(createPinia());
  app.mount('#app');
}

async function init() {
  try {
    await attachConsole();
    initApp();
  } catch (err) {
    log.error(`Failed to initialise Skynet: ${err}`);
  }
}

init();
