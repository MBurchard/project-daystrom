import {execFileSync, execSync} from 'node:child_process';
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
const RUST_MOD_DIR = join(ROOT, 'rust-mod');
const MOD_OUTPUT_DIR = join(APP_DIR, 'resources', 'mod');

const MANIFEST_PATH = join(APP_DIR, 'modules', 'backend', 'Cargo.toml');
const TS_RS_EXPORT_DIR = join(APP_DIR, 'modules', 'app', 'src', 'generated');
const TAURI_APP_PATH = join(APP_DIR, 'modules', 'backend');

/** Tauri resource directories that receive copies of app/resources/mod/. */
const TAURI_MOD_DIRS = [
  join(TAURI_APP_PATH, 'target', 'debug', 'mod'),
  join(TAURI_APP_PATH, 'target', 'release', 'mod'),
];

interface PlatformConfig {
  /** File name of the mod library after copying to app/resources/mod/. */
  outputLibrary: string;
  /** Path to the Rust-built library inside rust-mod/target/release/. */
  rustLibrary: string;
}

const PLATFORM_CONFIG: Record<string, PlatformConfig> = {
  darwin: {
    outputLibrary: 'libstfc-mod.dylib',
    rustLibrary: 'target/release/libstfc_mod.dylib',
  },
  win32: {
    outputLibrary: 'stfc-mod.dll',
    rustLibrary: 'target/release/stfc_mod.dll',
  },
};

// -- commands ---------------------------------------------------------------

const COMMANDS: Record<string, () => void> = {
  lint,
  'lint:ci': lintCi,
  'lint:fix': lintFix,
  'lint:app': lintApp,
  'lint:app:fix': lintAppFix,
  'lint:mod': lintMod,
  'lint:mod:fix': lintModFix,
  typecheck,
  'typecheck:frontend': typecheckFrontend,
  'typecheck:backend': typecheckBackend,
  'typecheck:mod': typecheckMod,
  test: testAll,
  'test:tooling': testTooling,
  'test:frontend': testFrontend,
  'test:frontend:watch': testFrontendWatch,
  'test:frontend:coverage': testFrontendCoverage,
  'test:backend': testBackend,
  'test:backend:coverage': testBackendCoverage,
  'test:mod': testMod,
  'test:mod:coverage': testModCoverage,
  'check:mod:dump': checkModDump,
  'release:verify': verifyRelease,
  build: buildApp,
  'build:mod': buildMod,
  'build:mod:mac-universal': buildModMacUniversal,
  'build:app': buildApp,
  'build:tauri:mac-universal': buildTauriMacUniversal,
  'build:tauri:windows': buildTauriWindows,
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
 * @param rustcArgs - optional flags forwarded to rustc or clippy after `--`
 */
function cargo(args: string, rustcArgs = ''): void {
  const forwardedArgs = rustcArgs ? ` -- ${rustcArgs}` : '';
  execSync(`cargo ${args} --manifest-path ${MANIFEST_PATH}${forwardedArgs}`, {
    cwd: APP_DIR,
    stdio: 'inherit',
    env: {...process.env, TS_RS_EXPORT_DIR},
  });
}

/**
 * Run a cargo command in the Rust mod crate.
 * @param args - cargo sub-command and flags, e.g. `["clippy"]`
 * @param rustcArgs - optional flags forwarded to rustc or clippy after `--`
 */
function cargoMod(args: readonly string[], rustcArgs: readonly string[] = []): void {
  const forwardedArgs = rustcArgs.length > 0 ? ['--', ...rustcArgs] : [];
  execFileSync('cargo', [...args, ...forwardedArgs], {cwd: RUST_MOD_DIR, stdio: 'inherit'});
}

/**
 * Run eslint from the repository root.
 * @param args - eslint flags, e.g. "--fix"
 */
function eslint(args = ''): void {
  execSync(`pnpm exec eslint . ${args}`.trim(), {cwd: ROOT, stdio: 'inherit'});
}

/**
 * Run stylelint for CSS and Vue style blocks.
 * @param args - stylelint flags, e.g. "--fix"
 */
function stylelint(args = ''): void {
  execSync(`pnpm exec stylelint "app/modules/app/src/**/*.{css,vue}" ${args}`.trim(), {
    cwd: ROOT,
    stdio: 'inherit',
  });
}

// -- lint -------------------------------------------------------------------

/**
 * Run eslint for app/tooling TypeScript and Vue files.
 */
function lintAppEslint(): void {
  log.info('Linting app TypeScript and Vue files...');
  eslint();
}

/**
 * Run eslint --fix for app/tooling TypeScript and Vue files.
 */
function lintAppEslintFix(): void {
  log.info('Fixing app TypeScript and Vue lint issues...');
  eslint('--fix');
}

/**
 * Run eslint without allowing warnings.
 */
function lintAppEslintCi(): void {
  log.info('Linting app TypeScript and Vue files in strict mode...');
  eslint('--max-warnings 0');
}

/**
 * Run stylelint for app CSS and Vue style blocks.
 */
function lintAppStyle(): void {
  log.info('Linting app CSS and Vue style blocks...');
  stylelint();
}

/**
 * Run stylelint --fix for app CSS and Vue style blocks.
 */
function lintAppStyleFix(): void {
  log.info('Fixing app CSS and Vue style issues...');
  stylelint('--fix');
}

/**
 * Run stylelint without allowing warnings.
 */
function lintAppStyleCi(): void {
  log.info('Linting app CSS and Vue style blocks in strict mode...');
  stylelint('--max-warnings 0');
}

/**
 * Check Rust formatting and run clippy for the backend crate.
 */
function lintAppBackend(): void {
  log.info('Checking backend Rust formatting...');
  cargo('fmt --check');
  log.info('Linting backend Rust code...');
  cargo('clippy');
}

/**
 * Check backend Rust formatting and deny all clippy warnings.
 */
function lintAppBackendCi(): void {
  log.info('Checking backend Rust formatting...');
  cargo('fmt --check');
  log.info('Linting backend Rust code in strict mode...');
  cargo('clippy --all-targets', '-D warnings');
}

/**
 * Format backend Rust code and apply clippy fixes where possible.
 */
function lintAppBackendFix(): void {
  log.info('Formatting backend Rust code...');
  cargo('fmt');
  log.info('Fixing backend Rust clippy issues...');
  cargo('clippy --fix --allow-dirty --allow-staged');
  log.info('Formatting backend Rust code after clippy fixes...');
  cargo('fmt');
}

/**
 * Check Rust formatting and run clippy for the mod crate.
 */
function lintMod(): void {
  log.info('Checking mod Rust formatting...');
  cargoMod(['fmt', '--check']);
  log.info('Linting mod Rust code...');
  cargoMod(['clippy']);
}

/**
 * Check mod Rust formatting and deny all clippy warnings.
 */
function lintModCi(): void {
  log.info('Checking mod Rust formatting...');
  cargoMod(['fmt', '--check']);
  log.info('Linting mod Rust code in strict mode...');
  cargoMod(['clippy', '--all-targets'], ['-D', 'warnings']);
}

/**
 * Format mod Rust code and apply clippy fixes where possible.
 */
function lintModFix(): void {
  log.info('Formatting mod Rust code...');
  cargoMod(['fmt']);
  log.info('Fixing mod Rust clippy issues...');
  cargoMod(['clippy', '--fix', '--allow-dirty', '--allow-staged']);
  log.info('Formatting mod Rust code after clippy fixes...');
  cargoMod(['fmt']);
}

/**
 * Run all app lint checks (TypeScript/Vue plus backend Rust).
 */
function lintApp(): void {
  lintAppEslint();
  lintAppStyle();
  lintAppBackend();
}

/**
 * Run all app lint fixes (TypeScript/Vue plus backend Rust).
 */
function lintAppFix(): void {
  lintAppEslintFix();
  lintAppStyleFix();
  lintAppBackendFix();
}

/**
 * Run all app lint checks without allowing warnings.
 */
function lintAppCi(): void {
  lintAppEslintCi();
  lintAppStyleCi();
  lintAppBackendCi();
}

/**
 * Run all lint checks (mod and app).
 */
function lint(): void {
  lintMod();
  lintApp();
}

/**
 * Run all lint checks without allowing warnings.
 */
function lintCi(): void {
  lintModCi();
  lintAppCi();
}

/**
 * Run all lint fixes (mod and app).
 */
function lintFix(): void {
  lintModFix();
  lintAppFix();
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
  cargoMod(['clippy']);
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
 * Run tests for repository tooling such as custom lint rules.
 */
function testTooling(): void {
  log.info('Running tooling tests...');
  execSync('pnpm exec vitest run eslint-rules', {cwd: ROOT, stdio: 'inherit'});
}

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
  cargoMod(['test']);
}

/**
 * Run mod tests with llvm-cov coverage.
 */
function testModCoverage(): void {
  log.info('Running mod tests with coverage...');
  cargoMod(['llvm-cov']);
}

/**
 * Check one or more IL2CPP dumps against the mod compatibility manifest.
 */
function checkModDump(): void {
  const dumpPaths = forwardedCommandArgs();
  log.info('Checking IL2CPP dump compatibility...');
  checkModDumps(dumpPaths);
}

/**
 * Verify the macOS and Windows IL2CPP dumps before creating a release.
 */
function verifyRelease(): void {
  const dumpPaths = forwardedCommandArgs();
  if (dumpPaths.length !== 2) {
    log.error('Usage: pnpm release:verify -- <macOS-dump> <Windows-dump>');
    process.exit(2);
  }

  log.info('Running release compatibility gate for macOS and Windows...');
  checkModDumps(dumpPaths);
}

/**
 * Return command arguments after removing pnpm's optional `--` separator.
 */
function forwardedCommandArgs(): string[] {
  const commandArgs = process.argv.slice(3);
  return commandArgs[0] === '--' ? commandArgs.slice(1) : commandArgs;
}

/**
 * Run the dump compatibility checker for the supplied paths.
 */
function checkModDumps(dumpPaths: readonly string[]): void {
  cargoMod(['run', '--bin', 'check-il2cpp-dump', '--', ...dumpPaths]);
}

/**
 * Run all tests (mod, frontend, backend).
 */
function testAll(): void {
  testTooling();
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
 * Build the Rust mod via cargo and copy the library to app/resources/mod/.
 */
function buildRustMod(config: PlatformConfig): void {
  log.info('Building Rust mod...');
  cargoMod(['build', '--release', '--lib']);

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
 */
function buildMod(): void {
  const config = PLATFORM_CONFIG[process.platform];
  if (!config) {
    log.error(`Unsupported platform: ${process.platform}`);
    process.exit(1);
  }

  cleanModDirs();
  mkdirSync(MOD_OUTPUT_DIR, {recursive: true});
  buildRustMod(config);
}

/**
 * Build a universal (ARM64 + x86_64) macOS mod library.
 *
 * Compiles the Rust mod for both architectures via cargo cross-compilation,
 * then merges them with lipo. Only works on macOS (CI and local).
 */
function buildModMacUniversal(): void {
  if (process.platform !== 'darwin') {
    log.error('build:mod:mac-universal is only supported on macOS');
    process.exit(1);
  }

  cleanModDirs();
  mkdirSync(MOD_OUTPUT_DIR, {recursive: true});

  const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];

  for (const target of targets) {
    log.info(`Building Rust mod for ${target}...`);
    cargoMod(['build', '--release', '--lib', '--target', target]);
  }

  const arm64Lib = join(RUST_MOD_DIR, 'target', 'aarch64-apple-darwin', 'release', 'libstfc_mod.dylib');
  const x86Lib = join(RUST_MOD_DIR, 'target', 'x86_64-apple-darwin', 'release', 'libstfc_mod.dylib');
  const outputLib = 'libstfc-mod.dylib';

  for (const dir of [MOD_OUTPUT_DIR, ...TAURI_MOD_DIRS]) {
    mkdirSync(dir, {recursive: true});
    const dest = join(dir, outputLib);
    log.info(`Creating universal binary ${dest}...`);
    execSync(`lipo -create "${arm64Lib}" "${x86Lib}" -output "${dest}"`, {
      stdio: 'inherit',
    });
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
 * Build only the Tauri universal macOS bundle (no mod build).
 *
 * Used in CI where the mod has already been built in a prior step.
 */
function buildTauriMacUniversal(): void {
  if (process.platform !== 'darwin') {
    log.error('build:tauri:mac-universal is only supported on macOS');
    process.exit(1);
  }
  log.info('Building Project Daystrom app (universal macOS)...');
  tauri('build --target universal-apple-darwin');
}

/**
 * Build only the Tauri Windows NSIS bundle (no mod build).
 *
 * Used in CI where the mod has already been built in a prior step.
 */
function buildTauriWindows(): void {
  if (process.platform !== 'win32') {
    log.error('build:tauri:windows is only supported on Windows');
    process.exit(1);
  }
  log.info('Building Project Daystrom app (Windows NSIS)...');
  tauri('build --bundles nsis');
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
