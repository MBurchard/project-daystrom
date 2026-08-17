import type {UpdateManifest} from './update-manifest.ts';
import {existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {afterEach, describe, expect, it} from 'vitest';
import {finalizeUpdateManifest, generateUpdateManifest} from './update-manifest.ts';

const temporaryDirectories: string[] = [];

/**
 * Create a temporary release-assets directory populated with valid updater files.
 * @returns Temporary assets directory.
 */
function createAssetsDirectory(): string {
  const directory = mkdtempSync(join(tmpdir(), 'daystrom-update-manifest-'));
  temporaryDirectories.push(directory);
  writeFileSync(join(directory, 'Project Daystrom.app.tar.gz'), 'mac archive');
  writeFileSync(join(directory, 'Project Daystrom.app.tar.gz.sig'), 'mac signature\n');
  writeFileSync(join(directory, 'Project Daystrom_0.9.0_x64-setup.exe'), 'windows installer');
  writeFileSync(join(directory, 'Project Daystrom_0.9.0_x64-setup.exe.sig'), 'windows signature\n');
  writeFileSync(join(directory, 'Project Daystrom_0.9.0_universal.dmg'), 'mac installer');
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, {recursive: true, force: true});
  }
});

describe('generateUpdateManifest', () => {
  it('maps the universal macOS archive and Windows installer to Tauri platforms', () => {
    const assetsDirectory = createAssetsDirectory();

    const manifest = generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    });
    const releaseBaseUrl = 'https://github.com/MBurchard/project-daystrom/releases/download/0.9.0';

    expect(manifest).toEqual({
      version: '0.9.0',
      platforms: {
        'darwin-aarch64': {
          signature: 'mac signature',
          url: `${releaseBaseUrl}/Project.Daystrom.app.tar.gz`,
        },
        'darwin-x86_64': {
          signature: 'mac signature',
          url: `${releaseBaseUrl}/Project.Daystrom.app.tar.gz`,
        },
        'windows-x86_64': {
          signature: 'windows signature',
          url: `${releaseBaseUrl}/Project.Daystrom_0.9.0_x64-setup.exe`,
        },
      },
    });
    expect(existsSync(join(assetsDirectory, 'Project Daystrom.app.tar.gz'))).toBe(false);
    expect(existsSync(join(assetsDirectory, 'Project.Daystrom.app.tar.gz'))).toBe(true);
    expect(existsSync(join(assetsDirectory, 'Project.Daystrom.app.tar.gz.sig'))).toBe(true);
    expect(JSON.parse(readFileSync(join(assetsDirectory, 'latest.json'), 'utf8'))).toEqual(manifest);
    expect(readFileSync(join(assetsDirectory, 'SHA256SUMS'), 'utf8')).toContain('latest.json');
  });

  it('rejects a release tag that does not match the application version', () => {
    const assetsDirectory = createAssetsDirectory();

    expect(() => generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
    })).toThrow('Release tag 0.10.0 does not match version 0.9.0');
  });

  it('requires a detached signature for every updater asset', () => {
    const assetsDirectory = createAssetsDirectory();
    rmSync(join(assetsDirectory, 'Project Daystrom.app.tar.gz.sig'));

    expect(() => generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    })).toThrow('Missing updater signature Project.Daystrom.app.tar.gz.sig');
  });

  it('rejects release assets that collide after normalization', () => {
    const assetsDirectory = createAssetsDirectory();
    writeFileSync(join(assetsDirectory, 'Project.Daystrom.app.tar.gz'), 'conflicting mac archive');

    expect(() => generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    })).toThrow(/both normalize to Project\.Daystrom\.app\.tar\.gz/);
  });

  it('embeds signed metadata for exactly the previous published release', () => {
    const assetsDirectory = createAssetsDirectory();
    const previousManifest = {
      version: '0.9.0',
      platforms: {
        'darwin-aarch64': {signature: 'old mac', url: 'https://example.test/old-mac'},
        'darwin-x86_64': {signature: 'old mac', url: 'https://example.test/old-mac'},
        'windows-x86_64': {signature: 'old windows', url: 'https://example.test/old-windows'},
      },
    };

    generateUpdateManifest({
      assetsDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
      previousManifest,
    });
    writeFileSync(join(assetsDirectory, 'rollback-metadata.json.sig'), 'rollback signature\n');
    const manifest = finalizeUpdateManifest(assetsDirectory);

    expect(manifest.rollback).toEqual({
      metadata: JSON.stringify({
        schema: 1,
        successorVersion: '0.10.0',
        predecessorVersion: '0.9.0',
        platforms: previousManifest.platforms,
      }),
      signature: 'rollback signature',
    });
    expect(readFileSync(join(assetsDirectory, 'rollback-metadata.json'), 'utf8'))
      .toBe(manifest.rollback?.metadata);
    expect(JSON.parse(readFileSync(join(assetsDirectory, 'latest.json'), 'utf8')))
      .toEqual(manifest);
    expect(readFileSync(join(assetsDirectory, 'SHA256SUMS'), 'utf8'))
      .toContain('rollback-metadata.json.sig');
  });

  it('rejects incomplete predecessor metadata before signing', () => {
    const assetsDirectory = createAssetsDirectory();

    expect(() => generateUpdateManifest({
      assetsDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
      previousManifest: {
        version: '0.9.0',
        platforms: {} as UpdateManifest['platforms'],
      },
    })).toThrow('Previous update manifest has no complete darwin-aarch64 platform');
  });
});
