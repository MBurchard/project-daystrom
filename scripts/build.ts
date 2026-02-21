import { cpSync, existsSync, mkdirSync } from 'node:fs';
import { execSync } from 'node:child_process';
import { basename, join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const APP_DIR = join(ROOT, 'app');
const MOD_DIR = join(ROOT, 'mod');
const MOD_OUTPUT_DIR = join(APP_DIR, 'resources', 'mod');

const PLATFORM_CONFIG: Record<string, { target: string; dylib: string }> = {
  darwin: {
    target: 'stfc-community-patch',
    dylib: 'build/macosx/arm64/release/libstfc-community-patch.dylib',
  },
  // win32: {
  //   target: 'stfc-community-patch',
  //   dylib: 'build/windows/x64/release/stfc-community-patch.dll',
  // },
};

/**
 * Build the mod dylib and copy it to app/resources/mod/.
 */
function buildMod(): void {
  const config = PLATFORM_CONFIG[process.platform];
  if (!config) {
    console.error(`Unsupported platform: ${process.platform}`);
    process.exit(1);
  }

  if (!existsSync(MOD_OUTPUT_DIR)) {
    mkdirSync(MOD_OUTPUT_DIR, { recursive: true });
    console.log(`Created ${MOD_OUTPUT_DIR}`);
  }

  console.log(`Building ${config.target}...`);
  execSync(`xmake build -y ${config.target}`, { cwd: MOD_DIR, stdio: 'inherit' });

  const src = join(MOD_DIR, config.dylib);
  const dest = join(MOD_OUTPUT_DIR, basename(src));
  cpSync(src, dest);
  console.log(`Copied ${dest}`);
}

/**
 * Build the Tauri app bundle (includes mod build).
 */
function buildApp(): void {
  buildMod();

  console.log('Building Skynet app...');
  execSync('pnpm tauri build', { cwd: APP_DIR, stdio: 'inherit' });
}

const command = process.argv[2];

switch (command) {
  case 'mod':
    buildMod();
    break;
  case 'app':
    buildApp();
    break;
  default:
    console.error('Usage: node scripts/build.ts <mod|app>');
    process.exit(1);
}
