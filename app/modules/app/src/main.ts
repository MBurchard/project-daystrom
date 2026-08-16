import {getLogger} from '@app/log';
import {registerLoggingShutdownHandler} from '@app/log/shutdown';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';

const log = getLogger('Main');

/**
 * Register application infrastructure, create the Vue application, and mount it to the DOM.
 * @returns a promise that resolves after the application has been mounted
 */
async function initApp(): Promise<void> {
  try {
    try {
      await registerLoggingShutdownHandler();
    } catch (error) {
      log.warn('Failed to register coordinated shutdown; relying on backend timeout:', error);
    }
    log.debug('Project Daystrom frontend started');
    const app = createApp(App);
    app.use(createPinia());
    app.mount('#app');
  } catch (reason) {
    log.error('Failed to initialise app:', reason);
  }
}

initApp().catch(reason => console.error('Unexpected frontend initialisation failure', reason));
