import {getLogger} from '@app/log';
import {getVersion} from '@tauri-apps/api/app';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';

const log = getLogger('Main');

/**
 * Create the Vue application, register plugins, and mount it to the DOM.
 */
async function initApp() {
  const version = await getVersion();
  log.info(`Project Daystrom ${version} frontend started`);
  const app = createApp(App);
  app.use(createPinia());
  app.mount('#app');
}

initApp().catch(err => log.error('Failed to initialise app:', err));
