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

/** Localized notes consumed only by Daystrom 0.10.0 and newer. */
interface LocalizedUpdateNotes {
  schema: 1;
  de: string;
  en: string;
}

/** Static Tauri update manifest generated for a GitHub release. */
export interface UpdateManifest {
  version: string;
  notes?: string;
  localized_notes?: LocalizedUpdateNotes;
  pub_date?: string;
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
  releaseNotes: ReleaseNotes;
  previousManifest?: UpdateManifest;
}

/** One localized release title and its corresponding user-visible changes. */
export interface ReleaseNotesLocale {
  title: string;
  changes: string[];
}

/** Versioned, human-reviewed release notes used by every release surface. */
export interface ReleaseNotes {
  version: string;
  locales: {
    de: ReleaseNotesLocale;
    en: ReleaseNotesLocale;
  };
}

/** File containing canonical rollback metadata before its signature is embedded. */
const ROLLBACK_METADATA_FILE = 'rollback-metadata.json';

/** Mirrors the published clients' Rust `MAX_RELEASE_NOTES_LINES` limit. */
const MAX_RELEASE_NOTE_CHANGES = 20;

/** Maximum number of Unicode characters accepted for one release-note entry. */
const MAX_RELEASE_NOTE_CHANGE_CHARACTERS = 180;

/** Mirrors the published clients' Rust `MAX_RELEASE_NOTES_CHARACTERS` limit. */
const MAX_RELEASE_NOTE_CHARACTERS = 2_000;

/** Maximum number of Unicode characters accepted for the release-note title. */
const MAX_RELEASE_NOTE_TITLE_CHARACTERS = 80;

/** Required keys in a versioned release-notes document. */
const RELEASE_NOTES_KEYS = ['locales', 'version'];

/** Required locale keys in a versioned release-notes document. */
const RELEASE_NOTES_LOCALE_KEYS = ['de', 'en'];

/** Required keys in each localized release-notes entry. */
const RELEASE_NOTES_LOCALE_VALUE_KEYS = ['changes', 'title'];

/** Semantic versions accepted by release automation. */
const RELEASE_VERSION_PATTERN = /^\d+\.\d+\.\d+(?:-[\da-z.-]+)?(?:\+[\da-z.-]+)?$/i;

/**
 * Require a semantic release version before using it in paths or release metadata.
 * @param version - Candidate release version.
 */
function validateVersion(version: string): void {
  if (!RELEASE_VERSION_PATTERN.test(version)) {
    throw new Error(`Invalid release version ${version}`);
  }
}

/**
 * Require a trimmed, single-line release-note string within its size limit.
 * @param value - Untrusted JSON field value.
 * @param description - Human-readable field description for errors.
 * @param maximumCharacters - Maximum permitted Unicode character count.
 * @returns Validated release-note string.
 */
function validateReleaseNoteText(
  value: unknown,
  description: string,
  maximumCharacters: number,
): string {
  if (typeof value !== 'string' || value !== value.trim() || !value) {
    throw new Error(`${description} must be a non-empty trimmed string`);
  }
  const containsDisallowedCharacter = [...value].some((character) => {
    const codePoint = character.codePointAt(0)!;
    return codePoint <= 31 ||
      (codePoint >= 127 && codePoint <= 159) ||
      codePoint === 0x061C ||
      (codePoint >= 0x200E && codePoint <= 0x200F) ||
      (codePoint >= 0x202A && codePoint <= 0x202E) ||
      (codePoint >= 0x2066 && codePoint <= 0x2069);
  });
  if (containsDisallowedCharacter) {
    throw new Error(`${description} must contain one line of display text`);
  }
  if ([...value].length > maximumCharacters) {
    throw new Error(`${description} exceeds ${maximumCharacters} characters`);
  }
  return value;
}

/**
 * Parse and strictly validate one versioned release-notes document.
 * @param source - JSON source text.
 * @param expectedVersion - Release version whose notes are required.
 * @returns Validated structured release notes.
 */
export function parseReleaseNotes(source: string, expectedVersion: string): ReleaseNotes {
  const value: unknown = JSON.parse(source);
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Release notes must be a JSON object');
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (keys.length !== RELEASE_NOTES_KEYS.length ||
    keys.some((key, index) => key !== RELEASE_NOTES_KEYS[index])) {
    throw new Error(`Release notes must contain exactly ${RELEASE_NOTES_KEYS.join(', ')}`);
  }
  if (record.version !== expectedVersion) {
    throw new Error(`Release notes version ${String(record.version)} does not match ${expectedVersion}`);
  }
  if (!record.locales || typeof record.locales !== 'object' || Array.isArray(record.locales)) {
    throw new Error('Release notes locales must be an object');
  }
  const locales = record.locales as Record<string, unknown>;
  const localeKeys = Object.keys(locales).sort();
  if (localeKeys.length !== RELEASE_NOTES_LOCALE_KEYS.length ||
    localeKeys.some((key, index) => key !== RELEASE_NOTES_LOCALE_KEYS[index])) {
    throw new Error(`Release notes locales must contain exactly ${RELEASE_NOTES_LOCALE_KEYS.join(', ')}`);
  }
  const de = parseReleaseNotesLocale(locales.de, 'de');
  const en = parseReleaseNotesLocale(locales.en, 'en');
  if (de.changes.length !== en.changes.length) {
    throw new Error('German and English release notes must contain the same number of changes');
  }
  return {version: expectedVersion, locales: {de, en}};
}

/**
 * Parse and strictly validate one localized release-notes entry.
 * @param value - Untrusted locale value.
 * @param locale - Locale identifier used in validation errors.
 * @returns Validated localized release notes.
 */
function parseReleaseNotesLocale(value: unknown, locale: string): ReleaseNotesLocale {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`Release notes locale ${locale} must be an object`);
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (keys.length !== RELEASE_NOTES_LOCALE_VALUE_KEYS.length ||
    keys.some((key, index) => key !== RELEASE_NOTES_LOCALE_VALUE_KEYS[index])) {
    throw new Error(
      `Release notes locale ${locale} must contain exactly ${RELEASE_NOTES_LOCALE_VALUE_KEYS.join(', ')}`,
    );
  }
  const title = validateReleaseNoteText(
    record.title,
    `Release notes title ${locale}`,
    MAX_RELEASE_NOTE_TITLE_CHARACTERS,
  );
  if (!Array.isArray(record.changes) ||
    record.changes.length === 0 ||
    record.changes.length > MAX_RELEASE_NOTE_CHANGES) {
    throw new Error(`Release notes locale ${locale} must contain 1-${MAX_RELEASE_NOTE_CHANGES} changes`);
  }
  const changes = record.changes.map((change, index) => validateReleaseNoteText(
    change,
    `Release notes change ${locale}.${index + 1}`,
    MAX_RELEASE_NOTE_CHANGE_CHARACTERS,
  ));
  if (new Set(changes).size !== changes.length) {
    throw new Error(`Release notes locale ${locale} must not contain duplicate changes`);
  }
  const releaseNotes = {title, changes};
  if ([...renderUpdateNotes(releaseNotes)].length > MAX_RELEASE_NOTE_CHARACTERS) {
    throw new Error(`Release notes locale ${locale} exceeds ${MAX_RELEASE_NOTE_CHARACTERS} characters`);
  }
  return releaseNotes;
}

/**
 * Load the required checked-in release-notes document for a version.
 * @param version - Release version whose notes are required.
 * @param directory - Directory containing versioned release-notes JSON files.
 * @returns Validated structured release notes.
 */
export function readReleaseNotes(
  version: string,
  directory = join(process.cwd(), 'release-notes'),
): ReleaseNotes {
  validateVersion(version);
  const path = join(directory, `${version}.json`);
  if (!existsSync(path)) {
    throw new Error(`Missing release notes ${path}`);
  }
  return parseReleaseNotes(readFileSync(path, 'utf8'), version);
}

/**
 * Render structured changes as safe plain text for Tauri's update manifest.
 * @param locale - Validated localized release notes.
 * @returns Plain-text bullet list displayed by the Daystrom client.
 */
export function renderUpdateNotes(locale: ReleaseNotesLocale): string {
  return locale.changes.map(change => `• ${change}`).join('\n');
}

/**
 * Render structured changes as the GitHub release body.
 * @param releaseNotes - Validated structured release notes.
 * @returns Markdown release body.
 */
export function renderReleaseBody(releaseNotes: ReleaseNotes): string {
  const sections = (['en', 'de'] as const)
    .map((locale) => {
      const notes = releaseNotes.locales[locale];
      return `## ${notes.title}\n\n${notes.changes.map(change => `- ${change}`).join('\n')}`;
    })
    .join('\n\n');
  return `${sections}\n`;
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
  validateVersion(version);
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) {
    throw new Error(`Invalid GitHub repository ${repository}`);
  }
  if (tag !== version) {
    throw new Error(`Release tag ${tag} does not match version ${version}`);
  }
}

/** Major and minor components that identify one feature-release line. */
interface ReleaseLine {
  major: number;
  minor: number;
}

/**
 * Extract the feature-release line from a validated semantic version.
 * @param version - Semantic version accepted by release validation.
 * @returns Major and minor version components.
 */
function releaseLine(version: string): ReleaseLine {
  const [major, minor] = version.split('.', 3).map(Number);
  if (major === undefined || minor === undefined || !Number.isSafeInteger(major) || !Number.isSafeInteger(minor)) {
    throw new TypeError(`Invalid release version ${version}`);
  }
  return {major, minor};
}

/**
 * Retain the original rollout anchor while publishing patches for the same release line.
 * @param version - Version being prepared.
 * @param previousManifest - Latest published update manifest.
 * @returns Inherited RFC 3339 rollout anchor, or undefined for a new release line.
 */
function inheritedRolloutDate(version: string, previousManifest: UpdateManifest): string | undefined {
  const current = releaseLine(version);
  const previous = releaseLine(previousManifest.version);
  if (current.major !== previous.major || current.minor !== previous.minor) {
    return undefined;
  }
  if (!previousManifest.pub_date || Number.isNaN(new Date(previousManifest.pub_date).valueOf())) {
    throw new TypeError(`Previous update manifest has no valid rollout date for ${previousManifest.version}`);
  }
  return previousManifest.pub_date;
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
 * Refresh one entry in an existing checksum file without requiring every release asset locally.
 * @param assetsDirectory - Directory containing the checksum file and changed asset.
 * @param file - Changed release asset whose checksum must be replaced.
 */
function refreshChecksum(assetsDirectory: string, file: string): void {
  const checksumPath = join(assetsDirectory, 'SHA256SUMS');
  const lines = readFileSync(checksumPath, 'utf8').trimEnd().split('\n');
  const suffix = `  ${file}`;
  const matches = lines.filter(line => line.endsWith(suffix));
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one ${file} entry in SHA256SUMS, found ${matches.length}`);
  }
  const digest = createHash('sha256').update(readFileSync(join(assetsDirectory, file))).digest('hex');
  writeFileSync(
    checksumPath,
    `${lines.map(line => line.endsWith(suffix) ? `${digest}${suffix}` : line).join('\n')}\n`,
  );
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
 * @param options.releaseNotes - Validated bilingual release notes.
 * @param options.previousManifest - Optional manifest of the latest published predecessor.
 * @returns Generated update manifest.
 */
export function generateUpdateManifest({
  assetsDirectory,
  version,
  repository,
  tag,
  releaseNotes,
  previousManifest,
}: GenerateUpdateManifestOptions): UpdateManifest {
  validateInputs(version, repository, tag);
  if (releaseNotes.version !== version) {
    throw new Error(`Release notes version ${releaseNotes.version} does not match ${version}`);
  }
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
  const rolloutDate = previousManifest ? inheritedRolloutDate(version, previousManifest) : undefined;
  const manifest: UpdateManifest = {
    version,
    notes: renderUpdateNotes(releaseNotes.locales.en),
    localized_notes: {
      schema: 1,
      de: renderUpdateNotes(releaseNotes.locales.de),
      en: renderUpdateNotes(releaseNotes.locales.en),
    },
    ...(rolloutDate ? {pub_date: rolloutDate} : {}),
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
 * Add the real GitHub publication time without touching signed release artefacts.
 * @param assetsDirectory - Directory containing only `latest.json` and `SHA256SUMS`.
 * @param version - Version of the release that emitted the publish event.
 * @param publishedAt - GitHub release publication time in RFC 3339 form.
 * @returns Publication-finalized update manifest.
 */
export function publishUpdateManifest(
  assetsDirectory: string,
  version: string,
  publishedAt: string,
): UpdateManifest {
  const manifestPath = join(assetsDirectory, 'latest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as UpdateManifest;
  if (manifest.version !== version) {
    throw new Error(`Published release ${version} contains manifest for ${manifest.version}`);
  }
  const parsedDate = new Date(publishedAt);
  if (Number.isNaN(parsedDate.valueOf())) {
    throw new TypeError(`Invalid release publication time ${publishedAt}`);
  }
  if (manifest.pub_date) {
    const rolloutDate = new Date(manifest.pub_date);
    if (Number.isNaN(rolloutDate.valueOf()) || rolloutDate > parsedDate) {
      throw new TypeError(`Invalid inherited rollout date ${manifest.pub_date}`);
    }
  } else {
    manifest.pub_date = publishedAt;
  }
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  refreshChecksum(assetsDirectory, 'latest.json');
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
  if (commandOrAssets === 'notes') {
    const [version, outputPath, ...unexpected] = arguments_;
    if (!version || !outputPath || unexpected.length > 0) {
      throw new Error('Usage: node scripts/update-manifest.ts notes <version> <output-file>');
    }
    writeFileSync(outputPath, renderReleaseBody(readReleaseNotes(version)));
    return;
  }
  if (commandOrAssets === 'finalize') {
    const [assetsDirectory, ...unexpected] = arguments_;
    if (!assetsDirectory || unexpected.length > 0) {
      throw new Error('Usage: node scripts/update-manifest.ts finalize <assets-dir>');
    }
    finalizeUpdateManifest(assetsDirectory);
    return;
  }
  if (commandOrAssets === 'publish') {
    const [assetsDirectory, version, publishedAt, ...unexpected] = arguments_;
    if (!assetsDirectory || !version || !publishedAt || unexpected.length > 0) {
      throw new Error('Usage: node scripts/update-manifest.ts publish <assets-dir> <version> <published-at>');
    }
    publishUpdateManifest(assetsDirectory, version, publishedAt);
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
    releaseNotes: readReleaseNotes(version),
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
