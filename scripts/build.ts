import {execSync} from 'node:child_process';
import {cpSync, existsSync, mkdirSync, readdirSync, rmSync} from 'node:fs';
import {join, resolve} from 'node:path';
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
const RUST_MOD_DIR = join(ROOT, 'rust-mod');
const MOD_OUTPUT_DIR = join(APP_DIR, 'resources', 'mod');

type ModVariant = 'cpp' | 'rust';
const ACTIVE_MOD: ModVariant = 'rust';
const MANIFEST_PATH = join(APP_DIR, 'modules', 'backend', 'Cargo.toml');
const TS_RS_EXPORT_DIR = join(APP_DIR, 'modules', 'app', 'src', 'generated');
const TAURI_APP_PATH = join(APP_DIR, 'modules', 'backend');

const MOD_LIBRARY_NAME = 'stfc-mod';

/** Tauri resource directories that receive copies of app/resources/mod/. */
const TAURI_MOD_DIRS = [
  join(TAURI_APP_PATH, 'target', 'debug', 'mod'),
  join(TAURI_APP_PATH, 'target', 'release', 'mod'),
];

interface PlatformConfig {
  target: string;
  library: string;
  xmakePlatform: string;
  xmakeArch: string;
  /** File name of the mod library after copying to app/resources/mod/. */
  outputLibrary: string;
  /** Path to the Rust-built library inside rust-mod/target/release/. */
  rustLibrary: string;
}

const PLATFORM_CONFIG: Record<string, PlatformConfig> = {
  darwin: {
    target: 'stfc-community-patch',
    library: 'build/macosx/arm64/release/libstfc-community-patch.dylib',
    xmakePlatform: 'macosx',
    xmakeArch: 'arm64',
    outputLibrary: `lib${MOD_LIBRARY_NAME}.dylib`,
    rustLibrary: 'target/release/libstfc_mod.dylib',
  },
  win32: {
    target: 'stfc-community-patch',
    library: 'build/windows/x64/release/stfc-community-patch.dll',
    xmakePlatform: 'windows',
    xmakeArch: 'x64',
    outputLibrary: `${MOD_LIBRARY_NAME}.dll`,
    rustLibrary: 'target/release/stfc_mod.dll',
  },
};

// -- commands ---------------------------------------------------------------

const COMMANDS: Record<string, () => void> = {
  typecheck,
  'typecheck:frontend': typecheckFrontend,
  'typecheck:backend': typecheckBackend,
  'typecheck:mod': typecheckMod,
  test: testAll,
  'test:frontend': testFrontend,
  'test:frontend:watch': testFrontendWatch,
  'test:frontend:coverage': testFrontendCoverage,
  'test:backend': testBackend,
  'test:backend:coverage': testBackendCoverage,
  'test:mod': testMod,
  'test:mod:coverage': testModCoverage,
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
 * Run the backend Rust type check (cargo clippy) and regenerate TypeScript bindings via ts-rs.
 */
function typecheckBackend(): void {
  log.info('Type-checking backend...');
  cargo('clippy');
  log.info('Generating TypeScript bindings...');
  cargo('test export_bindings');
}

/**
 * Run cargo clippy on the Rust mod crate.
 */
function typecheckMod(): void {
  log.info('Type-checking mod...');
  execSync('cargo clippy', {cwd: RUST_MOD_DIR, stdio: 'inherit'});
}

/**
 * Run all type checks (mod, backend, frontend).
 *
 * Backend runs before frontend so that ts-rs bindings are up to date before vue-tsc checks.
 */
function typecheck(): void {
  typecheckMod();
  typecheckBackend();
  typecheckFrontend();
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
 * Run mod tests via cargo test.
 */
function testMod(): void {
  log.info('Running mod tests...');
  execSync('cargo test', {cwd: RUST_MOD_DIR, stdio: 'inherit'});
}

/**
 * Run mod tests with llvm-cov coverage.
 */
function testModCoverage(): void {
  log.info('Running mod tests with coverage...');
  execSync('cargo llvm-cov', {cwd: RUST_MOD_DIR, stdio: 'inherit'});
}

/**
 * Run all tests (mod, frontend, backend).
 */
function testAll(): void {
  testMod();
  testFrontend();
  testBackend();
}

// -- build ------------------------------------------------------------------

/**
 * Remove all files from a directory (keeps the directory itself).
 * No-op if the directory does not exist.
 *
 * @param dir - absolute path to the directory to clean
 */
function cleanDir(dir: string): void {
  if (!existsSync(dir)) {
    return;
  }
  for (const entry of readdirSync(dir)) {
    rmSync(join(dir, entry), {force: true});
  }
}

/**
 * Remove stale mod libraries from both the source and Tauri target directories.
 * Ensures that after a build only the freshly copied library is present, preventing name mismatches between
 * build variants.
 */
function cleanModDirs(): void {
  for (const dir of [MOD_OUTPUT_DIR, ...TAURI_MOD_DIRS]) {
    cleanDir(dir);
  }
  log.info('Cleaned mod output directories');
}

/**
 * Build the C++ mod via xmake and copy the library to app/resources/mod/.
 */
function buildCppMod(config: PlatformConfig): void {
  log.info(`Configuring xmake for ${config.xmakePlatform} ${config.xmakeArch}...`);
  execSync(
    `xmake f -p ${config.xmakePlatform} -a ${config.xmakeArch} -m release -y`,
    {cwd: MOD_DIR, stdio: 'inherit'},
  );

  log.info(`Building ${config.target}...`);
  execSync(`xmake build -y ${config.target}`, {cwd: MOD_DIR, stdio: 'inherit'});

  const src = join(MOD_DIR, config.library);
  const dest = join(MOD_OUTPUT_DIR, config.outputLibrary);
  cpSync(src, dest);
  log.info(`Copied ${dest}`);
}

/**
 * Build the Rust mod via cargo and copy the library to app/resources/mod/.
 */
function buildRustMod(config: PlatformConfig): void {
  log.info('Building Rust mod...');
  execSync('cargo build --release', {cwd: RUST_MOD_DIR, stdio: 'inherit'});

  const src = join(RUST_MOD_DIR, config.rustLibrary);
  const libName = config.outputLibrary;
  for (const dir of [MOD_OUTPUT_DIR, ...TAURI_MOD_DIRS]) {
    mkdirSync(dir, {recursive: true});
    const dest = join(dir, libName);
    cpSync(src, dest);
    log.info(`Copied ${dest}`);
  }
}

/**
 * Build the mod library and copy it to app/resources/mod/.
 *
 * Dispatches to the C++ (xmake) or Rust (cargo) build based on the `ACTIVE_MOD` constant.
 */
function buildMod(): void {
  const config = PLATFORM_CONFIG[process.platform];
  if (!config) {
    log.error(`Unsupported platform: ${process.platform}`);
    process.exit(1);
  }

  cleanModDirs();
  mkdirSync(MOD_OUTPUT_DIR, {recursive: true});

  if (ACTIVE_MOD === 'rust') {
    buildRustMod(config);
  } else {
    buildCppMod(config);
  }
}

/**
 * Build the Tauri app bundle (always rebuilds the mod library first).
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
  buildMod();
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
