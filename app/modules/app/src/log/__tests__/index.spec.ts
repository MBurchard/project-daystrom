import {beforeEach, describe, expect, it, vi} from 'vitest';

const mocks = vi.hoisted(() => ({
  configureLogging: vi.fn(),
  useLog: vi.fn(),
}));

vi.mock('@mburchard/bit-log', () => ({
  configureLogging: mocks.configureLogging,
  useLog: mocks.useLog,
}));

vi.mock('@mburchard/bit-log/appender/ConsoleAppender', () => ({
  ConsoleAppender: class ConsoleAppender {},
}));

vi.mock('../TauriAppender', () => ({
  TauriAppender: class TauriAppender {},
}));

describe('logging configuration', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it('configures console and Tauri appenders and exports the logger factory', async () => {
    const logging = await import('../index');

    expect(mocks.configureLogging).toHaveBeenCalledWith({
      appender: {
        CONSOLE: expect.objectContaining({colored: false, useSpecificMethods: true}),
        TAURI: expect.objectContaining({Class: expect.any(Function)}),
      },
      root: {
        level: 'DEBUG',
        includeCallSite: true,
        appender: ['CONSOLE', 'TAURI'],
      },
    });
    expect(logging.getLogger).toBe(mocks.useLog);
  });
});
