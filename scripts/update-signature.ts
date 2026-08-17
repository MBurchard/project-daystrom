import {Buffer} from 'node:buffer';
import {readFileSync} from 'node:fs';
import {PublicKey, Signature} from '@threema/wasm-minisign-verify';

/** Relevant updater configuration embedded in the Tauri configuration. */
interface TauriConfig {
  plugins: {
    updater: {
      pubkey: string;
    };
  };
}

const BASE64_PATTERN = /^(?:[a-z\d+/]{4})*(?:[a-z\d+/]{2}==|[a-z\d+/]{3}=)?$/i;

/**
 * Decode a base64-encoded UTF-8 value used by Tauri's updater configuration.
 * @param encoded - Tauri's outer Base64 wrapper.
 * @param description - Human-readable value name for diagnostics.
 * @returns Decoded Minisign text.
 */
function decodeTauriText(encoded: string, description: string): string {
  const normalized = encoded.trim();
  if (!BASE64_PATTERN.test(normalized)) {
    throw new Error(`Failed to decode ${description}: invalid Base64`);
  }

  try {
    return new TextDecoder('utf-8', {fatal: true}).decode(Buffer.from(normalized, 'base64'));
  } catch (error) {
    throw new Error(`Failed to decode ${description}: ${String(error)}`);
  }
}

/**
 * Verify updater bytes using the double-encoded Minisign values consumed by Tauri.
 * @param data - Exact bytes covered by the signature.
 * @param encodedSignature - Base64-wrapped Minisign signature text.
 * @param encodedPublicKey - Base64-wrapped Minisign public-key text.
 */
export function verifyEncodedUpdateSignature(
  data: Uint8Array,
  encodedSignature: string,
  encodedPublicKey: string,
): void {
  let publicKey: PublicKey;
  let signature: Signature;

  try {
    publicKey = PublicKey.decode(decodeTauriText(encodedPublicKey, 'updater public key'));
  } catch (error) {
    throw new Error(`Updater signature verification failed: ${String(error)}`);
  }

  try {
    signature = Signature.decode(decodeTauriText(encodedSignature, 'updater signature'));
  } catch (error) {
    publicKey.free();
    throw new Error(`Updater signature verification failed: ${String(error)}`);
  }

  try {
    if (!publicKey.verify(data, signature)) {
      signature.free();
      publicKey.free();
      // noinspection ExceptionCaughtLocallyJS
      throw new Error('Minisign verifier rejected the updater signature');
    }
  } catch (error) {
    // wasm-bindgen keeps both values borrowed when verification throws. This command is short-lived,
    // so leave them to process teardown rather than masking the actual verification error with free().
    throw new Error(`Updater signature verification failed: ${String(error)}`);
  }

  signature.free();
  publicKey.free();
}

/**
 * Read an updater artefact, its detached signature, and the embedded Tauri public key, then verify them.
 * @param artifactPath - Path to the signed updater artefact.
 * @param signaturePath - Path to its Tauri-encoded Minisign signature.
 * @param configPath - Path to the Tauri configuration containing the public key.
 */
export function verifyUpdateArtifact(artifactPath: string, signaturePath: string, configPath: string): void {
  const artifact = readFileSync(artifactPath);
  const encodedSignature = readFileSync(signaturePath, 'utf8');
  const config = JSON.parse(readFileSync(configPath, 'utf8')) as TauriConfig;
  const encodedPublicKey = config.plugins?.updater?.pubkey;

  if (typeof encodedPublicKey !== 'string' || encodedPublicKey.length === 0) {
    throw new Error('Tauri configuration does not contain an updater public key');
  }

  verifyEncodedUpdateSignature(artifact, encodedSignature, encodedPublicKey);
}
