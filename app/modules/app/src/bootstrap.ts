import {initI18n} from '@app/i18n';
import {getLogger} from '@app/log';
import {registerLoggingShutdownHandler} from '@app/log/shutdown';
import {initTheme} from '@app/theme';
import {createPinia} from 'pinia';
import {createApp} from 'vue';
import App from './App.vue';
import './styles/themes.css';

const log = getLogger('Main');

/**
 * Register application infrastructure, create the Vue application, and mount it to the DOM.
 *
 * @returns A promise that resolves after the application has been mounted or its failure was logged.
 */
export async function initApp(): Promise<void> {
  try {
    try {
      await registerLoggingShutdownHandler();
    } catch (error) {
      log.warn('Failed to register coordinated shutdown; relying on backend timeout:', error);
    }
    await initTheme();
    await initI18n();
    log.debug('Project Daystrom frontend started');
    const app = createApp(App);
    app.use(createPinia());
    app.mount('#app');
  } catch (reason) {
    log.error('Failed to initialize app:', reason);
  }
}
