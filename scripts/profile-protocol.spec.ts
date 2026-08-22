import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {describe, expect, it} from 'vitest';

const PROFILE_STEM_SOURCES = [
  'app/modules/app/src/profileProtocol.ts',
  'app/modules/backend/src/profile_protocol.rs',
  'rust-mod/src/profile_protocol.rs',
];

const PROFILE_ENV_SOURCES = [
  'app/modules/backend/src/profile_protocol.rs',
  'rust-mod/src/profile_protocol.rs',
];

/**
 * Read one string constant from a TypeScript or Rust source file.
 *
 * @param sourcePath - Repository-relative source path.
 * @param constantName - Shared protocol constant to extract.
 * @returns The string assigned to the constant.
 */
function readProtocolConstant(sourcePath: string, constantName: string): string {
  const source = readFileSync(resolve(sourcePath), 'utf8');
  const pattern = new RegExp(`\\bconst\\s+${constantName}(?:\\s*:\\s*&str)?\\s*=\\s*['"]([^'"]+)['"];`);
  const match = pattern.exec(source);
  if (!match?.[1]) {
    throw new Error(`Missing ${constantName} in ${sourcePath}`);
  }
  return match[1];
}

describe('profile launch protocol', () => {
  it.each(['INITIAL_PROFILE_STEM', 'NEW_ACCOUNT_PROFILE_STEM'])('%s agrees across all binaries', (constantName) => {
    const values = PROFILE_STEM_SOURCES.map(sourcePath => readProtocolConstant(sourcePath, constantName));

    expect(new Set(values).size).toBe(1);
  });

  it('profile environment variable agrees between the app and mod', () => {
    const values = PROFILE_ENV_SOURCES.map(sourcePath => readProtocolConstant(sourcePath, 'PROFILE_ENV_VAR'));

    expect(new Set(values).size).toBe(1);
  });
});
