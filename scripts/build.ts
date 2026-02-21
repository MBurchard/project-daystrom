import {execSync} from 'node:child_process';
import {cpSync, existsSync, mkdirSync} from 'node:fs';
import {basename, join, resolve} from 'node:path';
import process from 'node:process';
import {configureLogging, useLog} from '@mburchard/bit-log';
import {ConsoleAppender} from '@mburchard/bit-log/appender/ConsoleAppender';

configureLogging({
  appender: {
    CONSOLE: {
      Class: ConsoleAppender,
    },
  },
  root: {
    level: 'DEBUG',
    appender: ['CONSOLE'],
  },
});

const log = useLog('Build');

// -- paths ------------------------------------------------------------------

const ROOT = resolve(import.meta.dirname, '..');
const APP_DIR = join(ROOT, 'app');
const MOD_DIR = join(ROOT, 'stfc-mod');
const MOD_OUTPUT_DIR = join(APP_DIR, 'resources', 'mod');
const MANIFEST_PATH = join(APP_DIR, 'modules', 'backend', 'Cargo.toml');
const TS_RS_EXPORT_DIR = join(APP_DIR, 'modules', 'app', 'src', 'generated');
const TAURI_APP_PATH = join(APP_DIR, 'modules', 'backend');

const PLATFORM_CONFIG: Record<string, {target: string; dylib: string}> = {
  darwin: {
    target: 'stfc-community-patch',
    dylib: 'build/macosx/arm64/release/libstfc-community-patch.dylib',
  },
  // win32: {
  //   target: 'stfc-community-patch',
  //   dylib: 'build/windows/x64/release/stfc-community-patch.dll',
  // },
};

// -- commands ---------------------------------------------------------------

const COMMANDS: Record<string, () => void> = {
  typecheck,
  'typecheck:frontend': typecheckFrontend,
  'typecheck:backend': typecheckBackend,
  test: testAll,
  'test:frontend': testFrontend,
  'test:frontend:watch': testFrontendWatch,
  'test:frontend:coverage': testFrontendCoverage,
  'test:backend': testBackend,
  'test:backend:coverage': testBackendCoverage,
  build: buildApp,
  'build:mod': buildMod,
  'build:app': buildApp,
  icons,
  dev,
};

// -- helpers ----------------------------------------------------------------

/**
 * Run a pnpm script defined in app/package.json.
 * @param script - the script name, e.g. "test:frontend"
 */
function appRun(script: string): void {
  execSync(`pnpm run ${script}`, {cwd: APP_DIR, stdio: 'inherit'});
}

/**
 * Run a Tauri CLI command with the correct TAURI_APP_PATH.
 * @param args - tauri sub-command and flags, e.g. "dev" or "icon resources/daystrom.png"
 */
function tauri(args: string): void {
  execSync(`pnpm exec tauri ${args}`, {
    cwd: APP_DIR,
    stdio: 'inherit',
    env: {...process.env, TAURI_APP_PATH},
  });
}

/**
 * Run a cargo command with the backend manifest path and ts-rs export dir.
 * @param args - cargo sub-command and flags, e.g. "test"
 */
function cargo(args: string): void {
  execSync(`cargo ${args} --manifest-path ${MANIFEST_PATH}`, {
    cwd: APP_DIR,
    stdio: 'inherit',
    env: {...process.env, TS_RS_EXPORT_DIR},
  });
}

// -- typecheck --------------------------------------------------------------

/**
 * Run the frontend TypeScript type check (vue-tsc).
 */
function typecheckFrontend(): void {
  log.info('Type-checking frontend...');
  appRun('typecheck:frontend');
}

/**
 * Run the backend Rust type check (cargo check).
 */
function typecheckBackend(): void {
  log.info('Type-checking backend...');
  cargo('check');
}

/**
 * Run both frontend and backend type checks.
 */
function typecheck(): void {
  typecheckFrontend();
  typecheckBackend();
}

// -- test -------------------------------------------------------------------

/**
 * Run frontend tests via vitest.
 */
function testFrontend(): void {
  log.info('Running frontend tests...');
  appRun('test:frontend');
}

/**
 * Run frontend tests in watch mode.
 */
function testFrontendWatch(): void {
  log.info('Running frontend tests in watch mode...');
  appRun('test:frontend:watch');
}

/**
 * Run frontend tests with v8 coverage.
 */
function testFrontendCoverage(): void {
  log.info('Running frontend tests with coverage...');
  appRun('test:frontend:coverage');
}

/**
 * Run backend tests and generate TypeScript bindings via ts-rs.
 */
function testBackend(): void {
  log.info('Running backend tests...');
  cargo('test');
}

/**
 * Run backend tests with llvm-cov coverage.
 */
function testBackendCoverage(): void {
  log.info('Running backend tests with coverage...');
  cargo('llvm-cov');
}

/**
 * Run all tests (frontend + backend).
 */
function testAll(): void {
  testFrontend();
  testBackend();
}

// -- build ------------------------------------------------------------------

/**
 * Build the mod dylib and copy it to app/resources/mod/.
 */
function buildMod(): void {
  const config = PLATFORM_CONFIG[process.platform];
  if (!config) {
    log.error(`Unsupported platform: ${process.platform}`);
    process.exit(1);
  }

  if (!existsSync(MOD_OUTPUT_DIR)) {
    mkdirSync(MOD_OUTPUT_DIR, {recursive: true});
    log.info(`Created ${MOD_OUTPUT_DIR}`);
  }

  log.info(`Building ${config.target}...`);
  execSync(`xmake build -y ${config.target}`, {cwd: MOD_DIR, stdio: 'inherit'});

  const src = join(MOD_DIR, config.dylib);
  const dest = join(MOD_OUTPUT_DIR, basename(src));
  cpSync(src, dest);
  log.info(`Copied ${dest}`);
}

/**
 * Build the Tauri app bundle (always rebuilds the mod dylib first).
 */
function buildApp(): void {
  buildMod();
  log.info('Building Project Daystrom app...');
  tauri('build');
}

/**
 * Generate Tauri icons from the app logo.
 */
function icons(): void {
  log.info('Generating icons...');
  tauri('icon resources/daystrom.png');
}

// -- dev --------------------------------------------------------------------

/**
 * Start the Tauri app with Vite hot reload.
 */
function dev(): void {
  log.info('Starting Project Daystrom in dev mode...');
  tauri('dev');
}

// -- dispatch ---------------------------------------------------------------

const command = process.argv[2];
const handler = COMMANDS[command];

if (handler) {
  handler();
} else {
  log.error(`Unknown command: ${command ?? '(none)'}`);
  log.error(`Available: ${Object.keys(COMMANDS).join(', ')}`);
  process.exit(1);
}
