import {describe, expect, it} from 'vitest';
import {isInternalFrame, parseCallSiteLine} from './index';

describe('isInternalFrame', () => {
  it('matches log directory (Unix path)', () => {
    expect(isInternalFrame('http://localhost:1420/modules/app/src/log/index.ts')).toBe(true);
  });

  it('matches log file (Unix path)', () => {
    expect(isInternalFrame('http://localhost:1420/modules/app/src/log.ts')).toBe(true);
  });

  it('matches log directory (Windows path)', () => {
    expect(isInternalFrame('C:\\app\\src\\log\\index.ts')).toBe(true);
  });

  it('matches log file (Windows path)', () => {
    expect(isInternalFrame('C:\\app\\src\\log.ts')).toBe(true);
  });

  it('matches plugin-log frames', () => {
    expect(isInternalFrame('http://localhost:1420/node_modules/@tauri-apps/plugin-log/guest-js/index.ts')).toBe(true);
  });

  it('does not match App.vue', () => {
    expect(isInternalFrame('http://localhost:1420/modules/app/src/App.vue')).toBe(false);
  });

  it('does not match main.ts', () => {
    expect(isInternalFrame('http://localhost:1420/modules/app/src/main.ts')).toBe(false);
  });

  it('does not match unrelated paths containing "log" as substring', () => {
    expect(isInternalFrame('http://localhost:1420/modules/app/src/dialog.ts')).toBe(false);
    expect(isInternalFrame('http://localhost:1420/modules/app/src/login.ts')).toBe(false);
  });
});

describe('parseCallSiteLine', () => {
  it('parses V8/Chrome format with function name', () => {
    const result = parseCallSiteLine('    at fire (http://localhost:1420/modules/app/src/log/index.ts:179:17)');
    expect(result).toEqual({
      url: 'http://localhost:1420/modules/app/src/log/index.ts',
      file: 'modules/app/src/log/index.ts',
      line: 179,
      column: 17,
    });
  });

  it('parses V8/Chrome format without function name', () => {
    const result = parseCallSiteLine('    at http://localhost:1420/modules/app/src/App.vue:20:7');
    expect(result).toEqual({
      url: 'http://localhost:1420/modules/app/src/App.vue',
      file: 'modules/app/src/App.vue',
      line: 20,
      column: 7,
    });
  });

  it('parses Safari/WebKit format', () => {
    const result = parseCallSiteLine('fire@http://localhost:1420/modules/app/src/log/index.ts:179:17');
    expect(result).toEqual({
      url: 'http://localhost:1420/modules/app/src/log/index.ts',
      file: 'modules/app/src/log/index.ts',
      line: 179,
      column: 17,
    });
  });

  it('returns undefined for unparseable lines', () => {
    expect(parseCallSiteLine('Error: something went wrong')).toBeUndefined();
    expect(parseCallSiteLine('')).toBeUndefined();
  });
});
