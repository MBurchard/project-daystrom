import {getLogger} from '@app/log';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';

const log = getLogger('Main');

/**
 * Create the Vue application, register plugins, and mount it to the DOM.
 */
async function initApp() {
  const app = createApp(App);
  app.use(createPinia());
  app.mount('#app');
}

initApp().catch(err => log.error('Failed to initialise app:', err));
