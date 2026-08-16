import {createHash} from 'node:crypto';
import {existsSync, readdirSync, readFileSync, renameSync, writeFileSync} from 'node:fs';
import {basename, join} from 'node:path';
import process from 'node:process';
import {pathToFileURL} from 'node:url';

/** A signed updater artifact exposed through the Tauri update manifest. */
interface UpdatePlatform {
  signature: string;
  url: string;
}

/** Static Tauri update manifest generated for a GitHub release. */
export interface UpdateManifest {
  version: string;
  platforms: {
    'darwin-aarch64': UpdatePlatform;
    'darwin-x86_64': UpdatePlatform;
    'windows-x86_64': UpdatePlatform;
  };
}

/** Inputs required to generate a static Tauri update manifest. */
export interface GenerateUpdateManifestOptions {
  assetsDirectory: string;
  version: string;
  repository: string;
  tag: string;
}

/**
 * Convert a local bundle file name to the canonical GitHub release asset name.
 * @param file - Local release asset file name.
 * @returns Canonical release asset file name.
 */
function canonicalAssetName(file: string): string {
  return file.replaceAll(' ', '.');
}

/**
 * Rename release assets before manifest generation so GitHub cannot change their referenced names.
 * @param assetsDirectory - Directory containing release assets.
 */
function normalizeAssetNames(assetsDirectory: string): void {
  const files = readdirSync(assetsDirectory, {withFileTypes: true})
    .filter(entry => entry.isFile())
    .map(entry => entry.name);
  const sourcesByTarget = new Map<string, string>();

  for (const file of files) {
    const target = canonicalAssetName(file);
    const existingSource = sourcesByTarget.get(target);
    if (existingSource) {
      throw new Error(`Release assets ${existingSource} and ${file} both normalize to ${target}`);
    }
    sourcesByTarget.set(target, file);
  }

  for (const [target, source] of sourcesByTarget) {
    if (source !== target) {
      renameSync(join(assetsDirectory, source), join(assetsDirectory, target));
    }
  }
}

/**
 * Find exactly one release asset matching a predicate.
 * @param files - Available asset file names.
 * @param predicate - Asset selection predicate.
 * @param description - Human-readable asset description for errors.
 * @returns The matching asset file name.
 */
function findSingleAsset(
  files: string[],
  predicate: (file: string) => boolean,
  description: string,
): string {
  const matches = files.filter(predicate);
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one ${description}, found ${matches.length}`);
  }
  return matches[0]!;
}

/**
 * Read and validate a detached Tauri signature next to an updater asset.
 * @param assetsDirectory - Directory containing release assets.
 * @param asset - Signed updater asset file name.
 * @returns Detached signature content.
 */
function readSignature(assetsDirectory: string, asset: string): string {
  const signaturePath = join(assetsDirectory, `${asset}.sig`);
  if (!existsSync(signaturePath)) {
    throw new Error(`Missing updater signature ${basename(signaturePath)}`);
  }
  const signature = readFileSync(signaturePath, 'utf8').trim();
  if (!signature) {
    throw new Error(`Updater signature ${basename(signaturePath)} is empty`);
  }
  return signature;
}

/**
 * Build the immutable GitHub release URL for an asset.
 * @param repository - GitHub repository in owner/name form.
 * @param tag - Release tag.
 * @param asset - Release asset file name.
 * @returns Public download URL.
 */
function releaseAssetUrl(repository: string, tag: string, asset: string): string {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(asset)}`;
}

/**
 * Validate manifest inputs received from release automation.
 * @param version - SemVer application version.
 * @param repository - GitHub repository in owner/name form.
 * @param tag - Release tag.
 */
function validateInputs(version: string, repository: string, tag: string): void {
  const semverPattern = /^\d+\.\d+\.\d+(?:-[\da-z.-]+)?(?:\+[\da-z.-]+)?$/i;
  if (!semverPattern.test(version)) {
    throw new Error(`Invalid release version ${version}`);
  }
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) {
    throw new Error(`Invalid GitHub repository ${repository}`);
  }
  if (tag !== version) {
    throw new Error(`Release tag ${tag} does not match version ${version}`);
  }
}

/**
 * Write SHA-256 checksums for every release asset except the checksum file itself.
 * @param assetsDirectory - Directory containing final release assets.
 */
function writeChecksums(assetsDirectory: string): void {
  const files = readdirSync(assetsDirectory, {withFileTypes: true})
    .filter(entry => entry.isFile() && entry.name !== 'SHA256SUMS')
    .map(entry => entry.name)
    .sort();
  const lines = files.map((file) => {
    const digest = createHash('sha256').update(readFileSync(join(assetsDirectory, file))).digest('hex');
    return `${digest}  ${file}`;
  });
  writeFileSync(join(assetsDirectory, 'SHA256SUMS'), `${lines.join('\n')}\n`);
}

/**
 * Generate the static Tauri update manifest and release checksums.
 * @param options - Manifest inputs.
 * @param options.assetsDirectory - Directory containing final release assets.
 * @param options.version - SemVer application version.
 * @param options.repository - GitHub repository in owner/name form.
 * @param options.tag - Release tag.
 * @returns Generated update manifest.
 */
export function generateUpdateManifest({
  assetsDirectory,
  version,
  repository,
  tag,
}: GenerateUpdateManifestOptions): UpdateManifest {
  validateInputs(version, repository, tag);
  normalizeAssetNames(assetsDirectory);
  const files = readdirSync(assetsDirectory, {withFileTypes: true})
    .filter(entry => entry.isFile())
    .map(entry => entry.name);
  const macAsset = findSingleAsset(files, file => file.endsWith('.app.tar.gz'), 'macOS updater archive');
  const windowsAsset = findSingleAsset(files, file => file.endsWith('.exe'), 'Windows NSIS installer');
  const macPlatform = {
    signature: readSignature(assetsDirectory, macAsset),
    url: releaseAssetUrl(repository, tag, macAsset),
  };
  const manifest = {
    version,
    platforms: {
      'darwin-aarch64': macPlatform,
      'darwin-x86_64': macPlatform,
      'windows-x86_64': {
        signature: readSignature(assetsDirectory, windowsAsset),
        url: releaseAssetUrl(repository, tag, windowsAsset),
      },
    },
  };
  writeFileSync(join(assetsDirectory, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  writeChecksums(assetsDirectory);
  return manifest;
}

/**
 * Run manifest generation from command-line arguments.
 */
function main(): void {
  const [assetsDirectory, version, repository, tag] = process.argv.slice(2);
  if (!assetsDirectory || !version || !repository || !tag) {
    throw new Error('Usage: node scripts/update-manifest.ts <assets-dir> <version> <owner/repo> <tag>');
  }
  generateUpdateManifest({assetsDirectory, version, repository, tag});
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
