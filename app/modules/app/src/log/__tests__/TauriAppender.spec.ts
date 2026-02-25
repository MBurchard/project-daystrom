import type {ILogEvent} from '@mburchard/bit-log/definitions';
import {beforeEach, describe, expect, it, vi} from 'vitest';
import {TauriAppender} from '../TauriAppender';

// Mock @tauri-apps/plugin-log — all five IPC functions
const {mockTrace, mockDebug, mockInfo, mockWarn, mockError} = vi.hoisted(() => ({
  mockTrace: vi.fn().mockResolvedValue(undefined),
  mockDebug: vi.fn().mockResolvedValue(undefined),
  mockInfo: vi.fn().mockResolvedValue(undefined),
  mockWarn: vi.fn().mockResolvedValue(undefined),
  mockError: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('@tauri-apps/plugin-log', () => ({
  trace: mockTrace,
  debug: mockDebug,
  info: mockInfo,
  warn: mockWarn,
  error: mockError,
}));

/**
 * Build a minimal ILogEvent for testing.
 * @param overrides - fields to override on the default event
 * @returns a complete ILogEvent
 */
function makeEvent(overrides: Partial<ILogEvent> = {}): ILogEvent {
  return {
    level: 'INFO',
    loggerName: 'Test',
    payload: ['hello world'],
    timestamp: new Date(),
    ...overrides,
  };
}

describe('tauriAppender', () => {
  let appender: TauriAppender;

  beforeEach(() => {
    vi.clearAllMocks();
    appender = new TauriAppender();
  });

  describe('level routing', () => {
    it('routes TRACE to trace()', async () => {
      await appender.handle(makeEvent({level: 'TRACE'}));
      expect(mockTrace).toHaveBeenCalledOnce();
    });

    it('routes DEBUG to debug()', async () => {
      await appender.handle(makeEvent({level: 'DEBUG'}));
      expect(mockDebug).toHaveBeenCalledOnce();
    });

    it('routes INFO to info()', async () => {
      await appender.handle(makeEvent({level: 'INFO'}));
      expect(mockInfo).toHaveBeenCalledOnce();
    });

    it('routes WARN to warn()', async () => {
      await appender.handle(makeEvent({level: 'WARN'}));
      expect(mockWarn).toHaveBeenCalledOnce();
    });

    it('routes ERROR to error()', async () => {
      await appender.handle(makeEvent({level: 'ERROR'}));
      expect(mockError).toHaveBeenCalledOnce();
    });

    it('routes FATAL to error()', async () => {
      await appender.handle(makeEvent({level: 'FATAL'}));
      expect(mockError).toHaveBeenCalledOnce();
    });
  });

  describe('message protocol', () => {
    it('builds \\x1F-delimited message from loggerName and payload', async () => {
      await appender.handle(makeEvent({
        loggerName: 'Auth',
        payload: ['User logged in'],
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        'Auth\x1FUser logged in',
        undefined,
      );
    });

    it('joins multiple payload items with spaces', async () => {
      await appender.handle(makeEvent({
        payload: ['count:', 42],
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        'Test\x1Fcount: 42',
        undefined,
      );
    });

    it('evaluates lazy payload functions', async () => {
      await appender.handle(makeEvent({
        payload: [() => 'lazy value'],
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        'Test\x1Flazy value',
        undefined,
      );
    });

    it('handles top-level lazy payload function', async () => {
      await appender.handle(makeEvent({
        payload: () => 'top-level lazy',
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        'Test\x1Ftop-level lazy',
        undefined,
      );
    });
  });

  describe('call-site handling', () => {
    it('strips browser origin from call-site file path', async () => {
      await appender.handle(makeEvent({
        callSite: {
          file: 'http://localhost:1420/modules/app/src/App.vue',
          line: 12,
          column: 5,
        },
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        expect.any(String),
        {file: 'modules/app/src/App.vue', line: 12},
      );
    });

    it('passes non-URL file paths through unchanged', async () => {
      await appender.handle(makeEvent({
        callSite: {file: 'src/App.vue', line: 7, column: 1},
      }));
      expect(mockInfo).toHaveBeenCalledWith(
        expect.any(String),
        {file: 'src/App.vue', line: 7},
      );
    });

    it('omits LogOptions when no call-site is present', async () => {
      await appender.handle(makeEvent({callSite: undefined}));
      expect(mockInfo).toHaveBeenCalledWith(
        expect.any(String),
        undefined,
      );
    });
  });

  describe('graceful degradation', () => {
    it('disables itself after first IPC failure', async () => {
      mockInfo.mockRejectedValueOnce(new Error('Tauri not available'));

      await appender.handle(makeEvent());
      expect(mockInfo).toHaveBeenCalledOnce();

      // Second call should be silently skipped
      await appender.handle(makeEvent());
      expect(mockInfo).toHaveBeenCalledOnce();
    });
  });
});
