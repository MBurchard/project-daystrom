import {createLogger} from '@app/log';
import {attachConsole} from '@tauri-apps/plugin-log';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';

const log = createLogger('Main');

/**
 * Create the Vue application, register plugins, and mount it to the DOM.
 */
function initApp() {
  const app = createApp(App);
  app.use(createPinia());
  app.mount('#app');
}

/**
 * Attach the Tauri log console bridge, then bootstrap the app.
 */
async function init() {
  try {
    await attachConsole();
    initApp();
  } catch (err) {
    log.error(`Failed to initialise Skynet: ${err}`);
  }
}

init().catch(err => log.error('Unexpected init failure', err));
