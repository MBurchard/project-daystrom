import {createHash} from 'node:crypto';
import {existsSync, readdirSync, readFileSync, renameSync, writeFileSync} from 'node:fs';
import {basename, join} from 'node:path';
import process from 'node:process';
import {pathToFileURL} from 'node:url';

/** A signed updater artefact exposed through the Tauri update manifest. */
interface UpdatePlatform {
  signature: string;
  url: string;
}

/** Signed relationship between a release and its sole permitted rollback predecessor. */
export interface RollbackMetadata {
  schema: 1;
  successorVersion: string;
  predecessorVersion: string;
  platforms: UpdateManifest['platforms'];
}

/** Rollback metadata and its Tauri-compatible detached signature. */
interface RollbackEnvelope {
  metadata: string;
  signature: string;
}

/** Static Tauri update manifest generated for a GitHub release. */
export interface UpdateManifest {
  version: string;
  platforms: {
    'darwin-aarch64': UpdatePlatform;
    'darwin-x86_64': UpdatePlatform;
    'windows-x86_64': UpdatePlatform;
  };
  rollback?: RollbackEnvelope;
}

/** Inputs required to generate a static Tauri update manifest. */
export interface GenerateUpdateManifestOptions {
  assetsDirectory: string;
  version: string;
  repository: string;
  tag: string;
  previousManifest?: UpdateManifest;
}

/** File containing canonical rollback metadata before its signature is embedded. */
const ROLLBACK_METADATA_FILE = 'rollback-metadata.json';

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
 * Create canonical metadata for the sole permitted predecessor.
 * @param version - Version being released.
 * @param previousManifest - Published manifest of the installed predecessor.
 * @returns Signed rollback payload content.
 */
function createRollbackMetadata(version: string, previousManifest: UpdateManifest): RollbackMetadata {
  if (previousManifest.version === version) {
    throw new Error(`Previous update manifest already describes version ${version}`);
  }
  if (!/^\d+\.\d+\.\d+(?:-[\da-z.-]+)?(?:\+[\da-z.-]+)?$/i.test(previousManifest.version)) {
    throw new Error(`Previous update manifest has invalid version ${previousManifest.version}`);
  }
  for (const target of ['darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'] as const) {
    const platform = previousManifest.platforms?.[target];
    if (!platform || !platform.signature.trim() || !platform.url.trim()) {
      throw new Error(`Previous update manifest has no complete ${target} platform`);
    }
  }
  return {
    schema: 1,
    successorVersion: version,
    predecessorVersion: previousManifest.version,
    platforms: previousManifest.platforms,
  };
}

/**
 * Generate the static Tauri update manifest and release checksums.
 * @param options - Manifest inputs.
 * @param options.assetsDirectory - Directory containing final release assets.
 * @param options.version - SemVer application version.
 * @param options.repository - GitHub repository in owner/name form.
 * @param options.tag - Release tag.
 * @param options.previousManifest - Optional manifest of the latest published predecessor.
 * @returns Generated update manifest.
 */
export function generateUpdateManifest({
  assetsDirectory,
  version,
  repository,
  tag,
  previousManifest,
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
  if (previousManifest) {
    const metadata = createRollbackMetadata(version, previousManifest);
    writeFileSync(join(assetsDirectory, ROLLBACK_METADATA_FILE), JSON.stringify(metadata));
  }
  writeFileSync(join(assetsDirectory, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  writeChecksums(assetsDirectory);
  return manifest;
}

/**
 * Embed the detached rollback-metadata signature and refresh final release checksums.
 * @param assetsDirectory - Directory containing generated release assets.
 * @returns Final update manifest.
 */
export function finalizeUpdateManifest(assetsDirectory: string): UpdateManifest {
  const manifestPath = join(assetsDirectory, 'latest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as UpdateManifest;
  const metadataPath = join(assetsDirectory, ROLLBACK_METADATA_FILE);
  if (existsSync(metadataPath)) {
    manifest.rollback = {
      metadata: readFileSync(metadataPath, 'utf8'),
      signature: readSignature(assetsDirectory, ROLLBACK_METADATA_FILE),
    };
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  }
  writeChecksums(assetsDirectory);
  return manifest;
}

/**
 * Read a previous published manifest when one was supplied to the command line.
 * @param path - Optional path to the predecessor's published update manifest.
 * @returns Parsed predecessor manifest, or undefined for the first updater release.
 */
function readPreviousManifest(path: string | undefined): UpdateManifest | undefined {
  return path ? JSON.parse(readFileSync(path, 'utf8')) as UpdateManifest : undefined;
}

/**
 * Run manifest generation from command-line arguments.
 */
function main(): void {
  const [commandOrAssets, ...arguments_] = process.argv.slice(2);
  if (commandOrAssets === 'finalize') {
    const [assetsDirectory, ...unexpected] = arguments_;
    if (!assetsDirectory || unexpected.length > 0) {
      throw new Error('Usage: node scripts/update-manifest.ts finalize <assets-dir>');
    }
    finalizeUpdateManifest(assetsDirectory);
    return;
  }

  const assetsDirectory = commandOrAssets;
  const [version, repository, tag, previousManifestPath, ...unexpected] = arguments_;
  if (!assetsDirectory || !version || !repository || !tag || unexpected.length > 0) {
    throw new Error(
      'Usage: node scripts/update-manifest.ts <assets-dir> <version> <owner/repo> <tag> [previous-manifest]',
    );
  }
  generateUpdateManifest({
    assetsDirectory,
    version,
    repository,
    tag,
    previousManifest: readPreviousManifest(previousManifestPath),
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
