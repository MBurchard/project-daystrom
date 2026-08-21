import type {AppTheme} from '@generated/AppTheme';
import {getAppTheme, setAppTheme} from '@app/commands/theme';
import {getLogger} from '@app/log';
import {readonly, ref} from 'vue';

const DEFAULT_THEME: AppTheme = 'classic';
const log = getLogger('Theme');
const theme = ref<AppTheme>(DEFAULT_THEME);
let initialized = false;

/** Apply one theme to reactive state and the document root. */
function applyTheme(nextTheme: AppTheme): void {
  theme.value = nextTheme;
  document.documentElement.dataset.theme = nextTheme;
}

/** Resolve and apply the persisted theme before Vue mounts. */
export async function initTheme(): Promise<void> {
  if (initialized) {
    return;
  }

  let initialTheme = DEFAULT_THEME;
  try {
    initialTheme = await getAppTheme();
  } catch (reason) {
    log.warn('Failed to resolve application theme; using Classic:', reason);
  }
  applyTheme(initialTheme);
  initialized = true;
}

/** Change the active theme immediately and persist the explicit selection. */
export async function changeTheme(nextTheme: AppTheme): Promise<void> {
  applyTheme(nextTheme);
  try {
    await setAppTheme(nextTheme);
  } catch (reason) {
    log.error('Failed to persist application theme:', reason);
  }
}

/** Expose the reactive application theme and its persistence action. */
export function useTheme() {
  return {
    theme: readonly(theme),
    setTheme: changeTheme,
  };
}
