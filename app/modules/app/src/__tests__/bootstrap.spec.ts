import {beforeEach, describe, expect, it, vi} from 'vitest';
import {initApp} from '../bootstrap';

const mocks = vi.hoisted(() => ({
  app: {
    mount: vi.fn(),
    use: vi.fn(),
  },
  createApp: vi.fn(),
  createPinia: vi.fn(),
  initI18n: vi.fn(),
  initTheme: vi.fn(),
  log: {
    debug: vi.fn(),
    error: vi.fn(),
    warn: vi.fn(),
  },
  registerShutdown: vi.fn(),
}));

vi.mock('../App.vue', () => ({default: {}}));
vi.mock('@app/log', () => ({getLogger: () => mocks.log}));
vi.mock('@app/log/shutdown', () => ({registerLoggingShutdownHandler: mocks.registerShutdown}));
vi.mock('@app/i18n', () => ({initI18n: mocks.initI18n}));
vi.mock('@app/theme', () => ({initTheme: mocks.initTheme}));
vi.mock('pinia', () => ({createPinia: mocks.createPinia}));
vi.mock('vue', () => ({createApp: mocks.createApp}));

describe('initApp', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.app.use.mockReturnValue(mocks.app);
    mocks.createApp.mockReturnValue(mocks.app);
    mocks.createPinia.mockReturnValue({});
    mocks.initI18n.mockResolvedValue(undefined);
    mocks.initTheme.mockResolvedValue(undefined);
    mocks.registerShutdown.mockResolvedValue(undefined);
  });

  it('registers shutdown handling before mounting the Vue application', async () => {
    await initApp();

    expect(mocks.registerShutdown).toHaveBeenCalledOnce();
    expect(mocks.initTheme).toHaveBeenCalledOnce();
    expect(mocks.initI18n).toHaveBeenCalledOnce();
    expect(mocks.createApp).toHaveBeenCalledOnce();
    expect(mocks.app.use).toHaveBeenCalledWith(expect.anything());
    expect(mocks.app.mount).toHaveBeenCalledWith('#app');
    expect(mocks.log.debug).toHaveBeenCalledWith('Project Daystrom frontend started');
  });

  it('continues mounting when coordinated shutdown registration fails', async () => {
    const error = new Error('listener unavailable');
    mocks.registerShutdown.mockRejectedValue(error);

    await initApp();

    expect(mocks.log.warn).toHaveBeenCalledWith(
      'Failed to register coordinated shutdown; relying on backend timeout:',
      error,
    );
    expect(mocks.app.mount).toHaveBeenCalledWith('#app');
  });

  it('logs application initialisation failures', async () => {
    const error = new Error('mount failed');
    mocks.app.mount.mockImplementation(() => {
      throw error;
    });

    await initApp();

    expect(mocks.log.error).toHaveBeenCalledWith('Failed to initialize app:', error);
  });
});
