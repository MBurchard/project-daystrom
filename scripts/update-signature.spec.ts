import {Buffer} from 'node:buffer';
import {mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {afterEach, describe, expect, it} from 'vitest';
import {verifyEncodedUpdateSignature, verifyUpdateArtifact} from './update-signature.ts';

const PUBLIC_KEY = `untrusted comment: minisign public key E7620F1842B4E81F
RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3`;
const SIGNATURE = `untrusted comment: signature from minisign secret key
RWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=
trusted comment: timestamp:1555779966\tfile:test
QtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==`;

const temporaryDirectories: string[] = [];

/**
 * Encode Minisign text as it appears in Tauri configuration and manifests.
 * @param value - Complete Minisign text.
 * @returns Tauri's outer Base64 representation.
 */
function encodeTauriValue(value: string): string {
  return Buffer.from(value).toString('base64');
}

/**
 * Create a valid Minisign public key with a different key identifier.
 * @returns Complete Minisign public-key text.
 */
function differentPublicKey(): string {
  const key = Buffer.concat([Buffer.from('Ed'), Buffer.alloc(8), Buffer.alloc(32)]);
  return `untrusted comment: different test key\n${key.toString('base64')}`;
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, {recursive: true, force: true});
  }
});

describe('verifyEncodedUpdateSignature', () => {
  it('accepts a valid Tauri-encoded signature', () => {
    expect(() => verifyEncodedUpdateSignature(
      Buffer.from('test'),
      encodeTauriValue(SIGNATURE),
      encodeTauriValue(PUBLIC_KEY),
    )).not.toThrow();
  });

  it('rejects a signature checked with a different public key', () => {
    expect(() => verifyEncodedUpdateSignature(
      Buffer.from('test'),
      encodeTauriValue(SIGNATURE),
      encodeTauriValue(differentPublicKey()),
    )).toThrow('Updater signature verification failed');
  });

  it('rejects a modified artefact', () => {
    expect(() => verifyEncodedUpdateSignature(
      Buffer.from('modified'),
      encodeTauriValue(SIGNATURE),
      encodeTauriValue(PUBLIC_KEY),
    )).toThrow('Updater signature verification failed');
  });

  it('rejects malformed outer Base64', () => {
    expect(() => verifyEncodedUpdateSignature(
      Buffer.from('test'),
      'not-base64',
      encodeTauriValue(PUBLIC_KEY),
    )).toThrow('invalid Base64');
  });
});

describe('verifyUpdateArtifact', () => {
  it('reads the artefact, signature, and embedded Tauri public key', () => {
    const directory = mkdtempSync(join(tmpdir(), 'daystrom-update-signature-'));
    temporaryDirectories.push(directory);
    const artifactPath = join(directory, 'artefact.bin');
    const signaturePath = join(directory, 'artefact.bin.sig');
    const configPath = join(directory, 'tauri.conf.json');
    writeFileSync(artifactPath, 'test');
    writeFileSync(signaturePath, encodeTauriValue(SIGNATURE));
    writeFileSync(configPath, JSON.stringify({
      plugins: {updater: {pubkey: encodeTauriValue(PUBLIC_KEY)}},
    }));

    expect(() => verifyUpdateArtifact(artifactPath, signaturePath, configPath)).not.toThrow();
  });
});
