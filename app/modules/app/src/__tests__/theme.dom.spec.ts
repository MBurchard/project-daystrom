import {beforeEach, describe, expect, it, vi} from 'vitest';

const mocks = vi.hoisted(() => ({
  getTheme: vi.fn(),
  log: {error: vi.fn(), warn: vi.fn()},
  setTheme: vi.fn(),
}));

vi.mock('@app/commands/theme', () => ({
  getAppTheme: mocks.getTheme,
  setAppTheme: mocks.setTheme,
}));
vi.mock('@app/log', () => ({getLogger: () => mocks.log}));

/** Import a fresh theme singleton for one test. */
async function importTheme() {
  vi.resetModules();
  return import('../theme');
}

describe('application theme', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete document.documentElement.dataset.theme;
    mocks.getTheme.mockResolvedValue('classic');
    mocks.setTheme.mockResolvedValue(undefined);
  });

  it('initializes the backend-selected theme once', async () => {
    mocks.getTheme.mockResolvedValue('omega');
    const {initTheme, useTheme} = await importTheme();

    await initTheme();
    await initTheme();

    expect(mocks.getTheme).toHaveBeenCalledOnce();
    expect(useTheme().theme.value).toBe('omega');
    expect(document.documentElement.dataset.theme).toBe('omega');
  });

  it('falls back to Classic when the backend theme cannot be resolved', async () => {
    const reason = new Error('offline');
    mocks.getTheme.mockRejectedValue(reason);
    const {initTheme, useTheme} = await importTheme();

    await initTheme();

    expect(useTheme().theme.value).toBe('classic');
    expect(document.documentElement.dataset.theme).toBe('classic');
    expect(mocks.log.warn).toHaveBeenCalledWith(
      'Failed to resolve application theme; using Classic:',
      reason,
    );
  });

  it('changes and persists the theme immediately', async () => {
    const {changeTheme, initTheme, useTheme} = await importTheme();
    await initTheme();

    await changeTheme('omega');

    expect(useTheme().theme.value).toBe('omega');
    expect(document.documentElement.dataset.theme).toBe('omega');
    expect(mocks.setTheme).toHaveBeenCalledWith('omega');
  });

  it('keeps the selected theme when persistence fails and logs the failure', async () => {
    const reason = new Error('disk full');
    mocks.setTheme.mockRejectedValue(reason);
    const {changeTheme, initTheme} = await importTheme();
    await initTheme();

    await changeTheme('omega');

    expect(document.documentElement.dataset.theme).toBe('omega');
    expect(mocks.log.error).toHaveBeenCalledWith('Failed to persist application theme:', reason);
  });
});
