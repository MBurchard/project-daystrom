import type {GenerateUpdateManifestOptions, ReleaseNotes, UpdateManifest} from './update-manifest.ts';
import {createHash} from 'node:crypto';
import {existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {afterEach, describe, expect, it} from 'vitest';
import {
  finalizeUpdateManifest,
  generateUpdateManifest as generateUpdateManifestBase,
  parseReleaseNotes,
  publishUpdateManifest,
  readReleaseNotes,
  renderReleaseBody,
  renderUpdateNotes,
} from './update-manifest.ts';

const temporaryDirectories: string[] = [];

/** Bilingual notes supplied by manifest-generation tests. */
const RELEASE_NOTES: ReleaseNotes = {
  version: '0.9.0',
  locales: {
    de: {title: 'Das ist neu', changes: ['Erste Änderung', 'Zweite Änderung']},
    en: {title: 'What is new', changes: ['First change', 'Second change']},
  },
};

/**
 * Generate a test manifest with the mandatory release notes included.
 * @param options - Manifest options other than the shared test notes.
 * @returns Generated update manifest.
 */
function generateUpdateManifest(
  options: Omit<GenerateUpdateManifestOptions, 'releaseNotes'>,
): UpdateManifest {
  return generateUpdateManifestBase({
    ...options,
    releaseNotes: {...RELEASE_NOTES, version: options.version},
  });
}

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
      notes: '• First change\n• Second change',
      localized_notes: {
        schema: 1,
        de: '• Erste Änderung\n• Zweite Änderung',
        en: '• First change\n• Second change',
      },
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

  it('keeps standard notes English and embeds localized notes for new clients', () => {
    const assetsDirectory = createAssetsDirectory();

    const manifest = generateUpdateManifestBase({
      assetsDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
      releaseNotes: {...RELEASE_NOTES, version: '0.10.0'},
    });

    expect(manifest.notes).toBe('• First change\n• Second change');
    expect(manifest.localized_notes?.de).toBe('• Erste Änderung\n• Zweite Änderung');
  });

  it('rejects release notes for a different version', () => {
    const assetsDirectory = createAssetsDirectory();

    expect(() => generateUpdateManifestBase({
      assetsDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
      releaseNotes: RELEASE_NOTES,
    })).toThrow('Release notes version 0.9.0 does not match 0.10.0');
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

describe('releaseNotes', () => {
  it('provides valid checked-in notes for the current package version', () => {
    const packageMetadata = JSON.parse(readFileSync('package.json', 'utf8')) as {version: string};

    expect(readReleaseNotes(packageMetadata.version).version).toBe(packageMetadata.version);
  });

  it('validates and renders one shared source for the client and GitHub', () => {
    const releaseNotes = parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: {title: 'Das ist neu', changes: ['Erste Änderung', 'Zweite Änderung']},
        en: {title: 'What is new', changes: ['First change', 'Second change']},
      },
    }), '0.10.0');

    expect(renderUpdateNotes(releaseNotes.locales.en)).toBe('• First change\n• Second change');
    expect(renderUpdateNotes(releaseNotes.locales.de)).toBe('• Erste Änderung\n• Zweite Änderung');
    expect(renderReleaseBody(releaseNotes)).toBe(
      '## What is new\n\n- First change\n- Second change\n\n' +
      '## Das ist neu\n\n- Erste Änderung\n- Zweite Änderung\n',
    );
  });

  it('rejects mismatched versions and incomplete content', () => {
    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.1',
      locales: RELEASE_NOTES.locales,
    }), '0.10.0')).toThrow('Release notes version 0.10.1 does not match 0.10.0');

    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: {title: 'Das ist neu', changes: []},
        en: {title: 'What is new', changes: []},
      },
    }), '0.10.0')).toThrow('Release notes locale de must contain 1-20 changes');
  });

  it('requires exactly German and English with matching change counts', () => {
    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {en: RELEASE_NOTES.locales.en},
    }), '0.10.0')).toThrow('Release notes locales must contain exactly de, en');

    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: {title: 'Das ist neu', changes: ['Änderung']},
        en: {title: 'What is new', changes: ['First change', 'Second change']},
      },
    }), '0.10.0')).toThrow('German and English release notes must contain the same number of changes');
  });

  it('rejects unknown fields, duplicate changes, and multiline entries', () => {
    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: RELEASE_NOTES.locales,
      extra: true,
    }), '0.10.0')).toThrow('Release notes must contain exactly locales, version');

    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: {title: 'Das ist neu', changes: ['Änderung', 'Änderung']},
        en: RELEASE_NOTES.locales.en,
      },
    }), '0.10.0')).toThrow('Release notes locale de must not contain duplicate changes');

    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: RELEASE_NOTES.locales.de,
        en: {title: 'What is new', changes: ['First line\nSecond line', 'Second change']},
      },
    }), '0.10.0')).toThrow('Release notes change en.1 must contain one line of display text');
  });

  it('rejects bidirectional formatting characters removed by published clients', () => {
    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: RELEASE_NOTES.locales.de,
        en: {title: 'What is new', changes: ['First\u202E change', 'Second change']},
      },
    }), '0.10.0')).toThrow('Release notes change en.1 must contain one line of display text');
  });

  it('keeps each language within the limit understood by published clients', () => {
    const longChanges = Array.from(
      {length: 12},
      (_, index) => `${index} ${'x'.repeat(176)}`,
    );

    expect(() => parseReleaseNotes(JSON.stringify({
      version: '0.10.0',
      locales: {
        de: {title: 'Das ist neu', changes: longChanges},
        en: {title: 'What is new', changes: longChanges},
      },
    }), '0.10.0')).toThrow('Release notes locale de exceeds 2000 characters');
  });
});

describe('publishUpdateManifest', () => {
  it('adds the publication time without changing signed content', () => {
    const assetsDirectory = createAssetsDirectory();
    const previousManifest = generateUpdateManifest({
      assetsDirectory: createAssetsDirectory(),
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    });
    generateUpdateManifest({
      assetsDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
      previousManifest,
    });
    writeFileSync(join(assetsDirectory, 'rollback-metadata.json.sig'), 'rollback signature\n');
    const finalized = finalizeUpdateManifest(assetsDirectory);
    const protectedFiles = [
      'Project.Daystrom.app.tar.gz',
      'Project.Daystrom.app.tar.gz.sig',
      'Project.Daystrom_0.9.0_x64-setup.exe',
      'Project.Daystrom_0.9.0_x64-setup.exe.sig',
      'rollback-metadata.json',
      'rollback-metadata.json.sig',
    ];
    const protectedContents = new Map(
      protectedFiles.map(file => [file, readFileSync(join(assetsDirectory, file))]),
    );

    const published = publishUpdateManifest(assetsDirectory, '0.10.0', '2026-08-17T12:34:56Z');

    expect(published.pub_date).toBe('2026-08-17T12:34:56Z');
    expect(published.platforms).toEqual(finalized.platforms);
    expect(published.rollback).toEqual(finalized.rollback);
    for (const [file, contents] of protectedContents) {
      expect(readFileSync(join(assetsDirectory, file))).toEqual(contents);
    }
    const manifestBytes = readFileSync(join(assetsDirectory, 'latest.json'));
    const manifestDigest = createHash('sha256').update(manifestBytes).digest('hex');
    expect(readFileSync(join(assetsDirectory, 'SHA256SUMS'), 'utf8'))
      .toContain(`${manifestDigest}  latest.json`);
  });

  it('rejects publication events for a different release', () => {
    const assetsDirectory = createAssetsDirectory();
    generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    });

    expect(() => publishUpdateManifest(assetsDirectory, '0.10.0', '2026-08-17T12:34:56Z'))
      .toThrow('Published release 0.10.0 contains manifest for 0.9.0');
  });

  it('rejects an invalid publication time', () => {
    const assetsDirectory = createAssetsDirectory();
    generateUpdateManifest({
      assetsDirectory,
      version: '0.9.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.9.0',
    });

    expect(() => publishUpdateManifest(assetsDirectory, '0.9.0', 'not-a-date'))
      .toThrow('Invalid release publication time not-a-date');
  });

  it('keeps the first publication time across patches of one release line', () => {
    const predecessorDirectory = createAssetsDirectory();
    generateUpdateManifest({
      assetsDirectory: predecessorDirectory,
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
    });
    const predecessor = publishUpdateManifest(
      predecessorDirectory,
      '0.10.0',
      '2026-08-17T12:00:00Z',
    );
    const assetsDirectory = createAssetsDirectory();

    const generated = generateUpdateManifest({
      assetsDirectory,
      version: '0.10.1',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.1',
      previousManifest: predecessor,
    });
    const published = publishUpdateManifest(assetsDirectory, '0.10.1', '2026-08-17T14:00:00Z');

    expect(generated.pub_date).toBe('2026-08-17T12:00:00Z');
    expect(published.pub_date).toBe('2026-08-17T12:00:00Z');
  });

  it('rejects a patch whose predecessor has no rollout date', () => {
    const assetsDirectory = createAssetsDirectory();
    const predecessor = generateUpdateManifest({
      assetsDirectory: createAssetsDirectory(),
      version: '0.10.0',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.0',
    });

    expect(() => generateUpdateManifest({
      assetsDirectory,
      version: '0.10.1',
      repository: 'MBurchard/project-daystrom',
      tag: '0.10.1',
      previousManifest: predecessor,
    })).toThrow('Previous update manifest has no valid rollout date for 0.10.0');
  });
});
