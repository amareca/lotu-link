#!/usr/bin/env node
/**
 * Download and verify the pinned third-party artifacts declared in
 * `vendor.lock.json`.
 *
 * Usage:
 *   node scripts/fetch-vendor.mjs           Download missing or corrupted targets.
 *   node scripts/fetch-vendor.mjs --check   Verify existing targets only (no network).
 *
 * Exit codes:
 *   0  every artifact is present and matches the lock
 *   1  verification failure: missing target in `--check`, wrong size or SHA-256,
 *      or an unreadable/invalid `vendor.lock.json`
 *   2  network/download failure, or an invalid invocation
 *
 * The script has no npm dependencies (Node >= 22) and never writes outside the
 * `target` paths declared in the lock: downloads land in `<target>.tmp`, are
 * verified and then renamed atomically over the target.
 */

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const EXIT_OK = 0;
const EXIT_VERIFICATION = 1;
const EXIT_NETWORK = 2;

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');
const LOCK_FILE = path.join(REPO_ROOT, 'vendor.lock.json');
const LOCK_NAME = path.relative(REPO_ROOT, LOCK_FILE);
const DOWNLOAD_TIMEOUT_MS = 120_000;
const SHA256_HEX = /^[0-9a-f]{64}$/;

const USAGE = 'usage: node scripts/fetch-vendor.mjs [--check]';

class VendoringError extends Error {
  /**
   * @param {string} message
   * @param {number} exitCode
   */
  constructor(message, exitCode) {
    super(message);
    this.name = 'VendoringError';
    this.exitCode = exitCode;
  }
}

/**
 * @typedef {object} Artifact
 * @property {string} name
 * @property {string} version
 * @property {string} url
 * @property {string} sha256
 * @property {number} size
 * @property {string} target  Path relative to the repository root, as declared.
 * @property {string} path    Absolute path of the target.
 */

/**
 * @param {string[]} argv
 * @returns {{ check: boolean, help: boolean }}
 */
function parseArguments(argv) {
  let check = false;
  let help = false;

  for (const argument of argv) {
    if (argument === '--check') {
      check = true;
    } else if (argument === '--help' || argument === '-h') {
      help = true;
    } else {
      throw new VendoringError(`unknown argument: ${argument}\n${USAGE}`, EXIT_NETWORK);
    }
  }

  return { check, help };
}

/**
 * Resolves a `target` from the lock, refusing anything outside the repository.
 *
 * @param {string} artifactName
 * @param {unknown} target
 * @returns {string} Absolute path.
 */
function resolveTarget(artifactName, target) {
  if (typeof target !== 'string' || target.trim() === '') {
    throw new VendoringError(
      `${artifactName}: target must be a non-empty string`,
      EXIT_VERIFICATION,
    );
  }
  if (path.isAbsolute(target)) {
    throw new VendoringError(
      `${artifactName}: target must be relative to the repository root: ${target}`,
      EXIT_VERIFICATION,
    );
  }

  const resolved = path.resolve(REPO_ROOT, target);
  const relative = path.relative(REPO_ROOT, resolved);
  if (relative === '' || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new VendoringError(
      `${artifactName}: target must stay inside the repository: ${target}`,
      EXIT_VERIFICATION,
    );
  }

  return resolved;
}

/**
 * @param {string} name
 * @param {unknown} raw
 * @returns {Artifact}
 */
function normalizeArtifact(name, raw) {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
    throw new VendoringError(
      `${name}: artifact entry must be an object`,
      EXIT_VERIFICATION,
    );
  }

  const { version, url, sha256, size, target } = /** @type {Record<string, unknown>} */ (raw);

  if (typeof version !== 'string' || version.trim() === '') {
    throw new VendoringError(`${name}: version must be a non-empty string`, EXIT_VERIFICATION);
  }
  if (typeof url !== 'string' || !/^https?:\/\/\S+$/.test(url)) {
    throw new VendoringError(`${name}: url must be an http(s) URL`, EXIT_VERIFICATION);
  }
  if (typeof sha256 !== 'string' || !SHA256_HEX.test(sha256)) {
    throw new VendoringError(
      `${name}: sha256 must be 64 lowercase hexadecimal characters`,
      EXIT_VERIFICATION,
    );
  }
  if (!Number.isInteger(size) || /** @type {number} */ (size) <= 0) {
    throw new VendoringError(`${name}: size must be a positive integer`, EXIT_VERIFICATION);
  }

  return {
    name,
    version,
    url,
    sha256,
    size: /** @type {number} */ (size),
    target,
    path: resolveTarget(name, target),
  };
}

/**
 * @returns {Promise<Artifact[]>}
 */
async function loadArtifacts() {
  let contents;
  try {
    contents = await readFile(LOCK_FILE, 'utf8');
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new VendoringError(`cannot read ${LOCK_NAME}: ${message}`, EXIT_VERIFICATION);
  }

  let lock;
  try {
    lock = JSON.parse(contents);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new VendoringError(`${LOCK_NAME} is not valid JSON: ${message}`, EXIT_VERIFICATION);
  }

  if (lock === null || typeof lock !== 'object' || Array.isArray(lock)) {
    throw new VendoringError(`${LOCK_NAME} must contain an object`, EXIT_VERIFICATION);
  }
  if (lock.v !== 1) {
    throw new VendoringError(
      `${LOCK_NAME}: unsupported format version ${JSON.stringify(lock.v)} (expected 1)`,
      EXIT_VERIFICATION,
    );
  }

  const artifacts = lock.artifacts;
  if (artifacts === null || typeof artifacts !== 'object' || Array.isArray(artifacts)) {
    throw new VendoringError(`${LOCK_NAME}: "artifacts" must be an object`, EXIT_VERIFICATION);
  }

  const entries = Object.entries(artifacts);
  if (entries.length === 0) {
    throw new VendoringError(`${LOCK_NAME} declares no artifacts`, EXIT_VERIFICATION);
  }

  return entries.map(([name, raw]) => normalizeArtifact(name, raw));
}

/**
 * @param {string} filePath
 * @returns {Promise<number | null>} Size in bytes, or `null` when it is missing.
 */
async function fileSize(filePath) {
  try {
    return (await stat(filePath)).size;
  } catch (error) {
    if (/** @type {NodeJS.ErrnoException} */ (error).code === 'ENOENT') {
      return null;
    }
    throw error;
  }
}

/**
 * @param {string} filePath
 * @returns {Promise<string>} Lowercase hex SHA-256.
 */
async function hashFile(filePath) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(filePath)) {
    hash.update(/** @type {Buffer} */ (chunk));
  }
  return hash.digest('hex');
}

/**
 * @typedef {object} Inspection
 * @property {boolean} valid
 * @property {string} reason
 */

/**
 * @param {Artifact} artifact
 * @returns {Promise<Inspection>}
 */
async function inspectArtifact(artifact) {
  const size = await fileSize(artifact.path);
  if (size === null) {
    return { valid: false, reason: 'target is missing' };
  }
  if (size !== artifact.size) {
    return {
      valid: false,
      reason: `size is ${size} bytes, expected ${artifact.size}`,
    };
  }

  const digest = await hashFile(artifact.path);
  if (digest !== artifact.sha256) {
    return { valid: false, reason: `sha256 is ${digest}, expected ${artifact.sha256}` };
  }

  return { valid: true, reason: 'ok' };
}

/**
 * Downloads `artifact` to `<target>.tmp`, verifies it and renames it over the
 * target. The declared target is never modified unless the download matches.
 *
 * @param {Artifact} artifact
 */
async function downloadArtifact(artifact) {
  const temporary = `${artifact.path}.tmp`;
  await mkdir(path.dirname(artifact.path), { recursive: true });

  try {
    const response = await fetch(artifact.url, {
      redirect: 'follow',
      signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS),
    });
    if (!response.ok) {
      throw new VendoringError(
        `download failed with HTTP ${response.status} ${response.statusText}`,
        EXIT_NETWORK,
      );
    }
    await writeFile(temporary, Buffer.from(await response.arrayBuffer()));
  } catch (error) {
    await rm(temporary, { force: true });
    if (error instanceof VendoringError) {
      throw error;
    }
    const message = error instanceof Error ? error.message : String(error);
    throw new VendoringError(`download failed: ${message}`, EXIT_NETWORK);
  }

  const size = await fileSize(temporary);
  if (size !== artifact.size) {
    await rm(temporary, { force: true });
    throw new VendoringError(
      `downloaded ${size ?? 0} bytes, expected ${artifact.size}`,
      EXIT_VERIFICATION,
    );
  }

  const digest = await hashFile(temporary);
  if (digest !== artifact.sha256) {
    await rm(temporary, { force: true });
    throw new VendoringError(
      `downloaded sha256 is ${digest}, expected ${artifact.sha256}`,
      EXIT_VERIFICATION,
    );
  }

  try {
    await rename(temporary, artifact.path);
  } catch (error) {
    await rm(temporary, { force: true });
    const message = error instanceof Error ? error.message : String(error);
    throw new VendoringError(`cannot move the verified file into place: ${message}`, EXIT_NETWORK);
  }
}

/**
 * @param {string[]} argv
 * @returns {Promise<number>} Process exit code.
 */
async function main(argv) {
  const { check, help } = parseArguments(argv);
  if (help) {
    console.log(USAGE);
    return EXIT_OK;
  }

  const artifacts = await loadArtifacts();
  let exitCode = EXIT_OK;

  for (const artifact of artifacts) {
    const label = `${artifact.name}@${artifact.version}`;

    try {
      const inspection = await inspectArtifact(artifact);

      if (inspection.valid) {
        console.log(`ok      ${label} -> ${artifact.target}`);
        continue;
      }

      if (check) {
        console.error(`failed  ${label} -> ${artifact.target}: ${inspection.reason}`);
        exitCode = Math.max(exitCode, EXIT_VERIFICATION);
        continue;
      }

      console.log(`fetch   ${label} -> ${artifact.target} (${inspection.reason})`);
      await downloadArtifact(artifact);
      console.log(`ok      ${label} -> ${artifact.target} (downloaded and verified)`);
    } catch (error) {
      if (!(error instanceof VendoringError)) {
        throw error;
      }
      console.error(`failed  ${label} -> ${artifact.target}: ${error.message}`);
      exitCode = Math.max(exitCode, error.exitCode);
    }
  }

  if (exitCode === EXIT_OK) {
    console.log(`vendor: ${artifacts.length} artifact(s) verified`);
  } else if (check) {
    console.error('vendor: verification failed, nothing was downloaded');
  } else {
    console.error('vendor: some artifacts are not available');
  }

  return exitCode;
}

try {
  process.exitCode = await main(process.argv.slice(2));
} catch (error) {
  if (error instanceof VendoringError) {
    console.error(`error: ${error.message}`);
    process.exitCode = error.exitCode;
  } else {
    const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
    console.error(`error: ${message}`);
    process.exitCode = EXIT_NETWORK;
  }
}
