# Project Daystrom ![Version](https://img.shields.io/github/v/release/MBurchard/project-daystrom?color=4488FF&label=)

![Crafted with Rust](https://img.shields.io/badge/Crafted_with-Rust-000000?logo=rust&logoColor=white)
![Crafted with TypeScript](https://img.shields.io/badge/Crafted_with-TypeScript-3178C6?logo=typescript&logoColor=white)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg?logo=gnu&logoColor=white)](https://www.gnu.org/licenses/gpl-3.0)
[![CI](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml/badge.svg)](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml)

🇬🇧 English | [🇩🇪 Deutsch](README.de.md)

A companion app and custom game mod for
[Star Trek Fleet Command](https://www.scopely.com/games/star-trek-fleet-command) on macOS and Windows.

## What is this?

Project Daystrom is a native desktop app with an integrated game mod for STFC. The mod is built entirely in Rust
with a custom hook engine (ARM64 + x86_64) that intercepts the game's IL2CPP runtime directly.

**Key features:**

- **Multi-account support** on Windows and macOS
  - Each account gets its own TOML-based profile with isolated game settings
  - Custom PlayerPrefs interceptor redirects game settings into per-profile storage
  - Switching accounts is a single click in the launcher, profiles are portable across platforms
- **Game enhancements** powered by the Rust mod
  - Keyboard hotkeys: ESC to collect rewards, SPACE to engage, mine, or warp
  - Adjustable UI scale (50-200%), applied live to the running game
  - Auto-open chat sidebar on game start
  - Auto-expand job queue panel on game start
  - Configurable system view zoom distance and ship name visibility range
  - Toast banner suppression with per-type opt-out (combat, station, armada, etc.)
  - Automatic game update detection via the Scopely update API
- **Native cross-platform app** (Tauri 2 + Vue 3)
  - Unified launcher: entitlement patching on macOS, DLL proxy injection on Windows
  - Process monitoring with automatic detection of game and launcher activity
  - System tray integration with minimize-to-tray and quit protection
  - Live WebSocket bridge that syncs settings to the running game in real time

## Installation

Download the latest release for your platform from the
[Releases page](https://github.com/MBurchard/project-daystrom/releases/latest).

- **macOS**: Download the `.dmg` file, open it, and drag the app to your Applications folder.
- **Windows**: Download the `.exe` installer and run it. If Windows SmartScreen shows a warning, click
  "More info" and then "Run anyway" (the app is self-signed, not yet verified by Microsoft).

After installation, launch Project Daystrom and click the play button to start the game with the mod.

## Acknowledgements

This project was originally inspired by the [STFC Community Mod](https://github.com/netniV/stfc-mod) by
[netniV](https://github.com/netniV), [tashcan](https://github.com/tashcan), and contributors. The legacy C++ mod
code is kept in `stfc-mod/` as reference. Daystrom has since moved to its own Rust-based mod with a custom hook
engine and profile system.

## Built with

- [Tauri 2](https://tauri.app/) (Rust backend + native shell)
- [Vue 3](https://vuejs.org/) + [Vite](https://vite.dev/) (frontend)
- [@mburchard/bit-log](https://www.npmjs.com/package/@mburchard/bit-log) (structured logging)
- Custom IL2CPP hook engine in Rust (ARM64 + x86_64)

## Project Structure

```text
project-daystrom/
├── package.json            # Workspace root (orchestrating scripts)
├── pnpm-workspace.yaml     # Workspace config (members: app, scripts)
├── eslint.config.js        # Shared ESLint config (lints entire project)
├── tsconfig.base.json      # Shared TypeScript base config
├── scripts/                # Build and tooling scripts
│   ├── build.ts            #   Mod + app build orchestration
│   └── package.json        #   Script dependencies
├── rust-mod/               # Daystrom game mod (Rust, cdylib)
│   ├── src/hook/           #   Hook engine (inline hooks, ARM64 + x86_64)
│   ├── src/hooks/          #   IL2CPP hook implementations
│   ├── src/il2cpp/         #   IL2CPP runtime bindings
│   └── Cargo.toml          #   Crate config
├── stfc-mod/               # STFC Community Mod (legacy, kept as reference)
├── app/                    # Project Daystrom app (Tauri 2 + Vue 3)
│   ├── modules/
│   │   ├── app/            #   Vue 3 frontend
│   │   ├── backend/        #   Tauri/Rust backend
│   │   └── plugins/        #   Feature plugins (dashboard, alerts, advisor)
│   ├── resources/          #   Shared assets (logo, icons)
│   └── package.json        #   App dependencies + app-local scripts
└── README.md
```

## Prerequisites

- [Node.js](https://nodejs.org/) >= 24
- [pnpm](https://pnpm.io/) >= 10
- [Rust](https://www.rust-lang.org/tools/install) (stable)

### macOS

- **Apple Silicon** — local development assumes arm64; the CI handles universal builds
- Xcode Command Line Tools (`xcode-select --install`)

### Windows

- **Visual Studio Build Tools 2022** (or VS Community) — workload "Desktop development with C++"
  including a **Windows SDK** (not installed by default!)
- Rust: standard installation via [rustup-init.exe](https://rustup.rs/) (option 1 selects MSVC toolchain)

### Legacy C++ mod (optional)

Building the original Community Mod in `stfc-mod/` additionally requires
[XMake](https://xmake.io/) and [CMake](https://cmake.org/).

## Setup

```sh
nvm use
pnpm install
```

All commands run from the **workspace root** unless noted otherwise.

## Building the mod

The Rust mod in `rust-mod/` produces a shared library that gets injected into the game at launch.

```sh
pnpm build:mod
```

This compiles the mod for the current platform and copies the result to `app/resources/mod/`.

## Scripts

### Workspace root (run from the project root)

| Script                            | Description                                          |
|-----------------------------------|------------------------------------------------------|
| `pnpm install:all`                | Force-install all workspace dependencies             |
| `pnpm lint`                       | Run ESLint across the entire project                 |
| `pnpm lint:fix`                   | Run ESLint with auto-fix                             |
| `pnpm typecheck`                  | TypeScript + Rust type checks                        |
| `pnpm test`                       | Run all tests (frontend + backend)                   |
| `pnpm test:app`                   | Run all app tests (frontend + backend)               |
| `pnpm test:app:frontend`          | Run frontend tests only (vitest)                     |
| `pnpm test:app:backend`           | Run backend tests only (cargo test + ts-rs bindings) |
| `pnpm test:app:frontend:watch`    | Run frontend tests in watch mode                     |
| `pnpm test:app:frontend:coverage` | Run frontend tests with v8 coverage                  |
| `pnpm test:app:backend:coverage`  | Run backend tests with llvm-cov coverage             |
| `pnpm build`                      | Build everything (mod dylib → Tauri app)             |
| `pnpm build:mod`                  | Build mod dylib and copy to `app/resources/mod/`     |
| `pnpm build:app`                  | Build mod dylib + Tauri app bundle                   |
| `pnpm icons`                      | Generate Tauri icons from `resources/daystrom.png`   |
| `pnpm dev`                        | Build mod + start Tauri app with hot reload          |

### Path Aliases

| Alias          | Resolves to                   |
|----------------|-------------------------------|
| `@app/*`       | `modules/app/src/*`           |
| `@generated/*` | `modules/app/src/generated/*` |
| `@resources/*` | `resources/*`                 |

## Windows Code Signing (optional)

The GitHub workflow supports optional code signing for the NSIS installer. It requires a self-signed code signing
certificate and two repository secrets.

### Create a self-signed certificate

```powershell
New-SelfSignedCertificate -Type CodeSigningCert -Subject "CN=Your Name, Code Signing" `
  -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(5)
```

### Export as PFX

The thumbprint is shown when creating the certificate. You can also find it via:

```powershell
Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert
```

```powershell
$cert = Get-ChildItem Cert:\CurrentUser\My\<thumbprint>
$pw = Read-Host -AsSecureString "PFX password"
Export-PfxCertificate -Cert $cert -FilePath daystrom.pfx -Password $pw
```

### Verify the PFX

```powershell
certutil -dump daystrom.pfx
```

### Configure GitHub Secrets

Encode the PFX as Base64 and copy to the clipboard.\
Note that PowerShell resolves relative paths from the user's home directory, not from the current working directory.\
Use an absolute path to avoid surprises.\

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("<path-to-pfx>")) | Set-Clipboard
```

Then set these repository secrets in GitHub:

| Secret                 | Value                          |
|------------------------|--------------------------------|
| `WINDOWS_PFX_BASE64`   | Base64-encoded PFX (clipboard) |
| `WINDOWS_PFX_PASSWORD` | The password from the export   |

## App (Tauri + Vue 3 + Vite)

### Type generation (ts-rs)

Shared types between Rust backend and TypeScript frontend are auto-generated by
[ts-rs](https://github.com/Aleph-Alpha/ts-rs). Rust structs annotated with `#[derive(TS)]` produce
TypeScript interfaces in `app/modules/app/src/generated/` whenever `pnpm test:app:backend` runs.
Rust doc comments are carried over as JSDoc.

```rust
#[derive(Serialize, TS)]
#[ts(export)]
pub struct GameStatus { /* ... */ }
```

```typescript
import type {GameStatus} from '@generated/GameStatus';
```


Plugins live in `modules/plugins/` and are loaded by the main app. The architecture is
intentionally modular so that individual plugins can be developed and published independently.

### Environment Variables

| Variable            | Default | Description                                              |
|---------------------|---------|----------------------------------------------------------|
| `DAYSTROM_DEVTOOLS` | `1`     | Set to `0` to suppress DevTools in debug builds          |

## License

This project is licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).
