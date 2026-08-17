# Project Daystrom ![Version](https://img.shields.io/github/v/release/MBurchard/project-daystrom?color=4488FF&label=)

![Crafted with Rust](https://img.shields.io/badge/Crafted_with-Rust-000000?logo=rust&logoColor=white)
![Crafted with TypeScript](https://img.shields.io/badge/Crafted_with-TypeScript-3178C6?logo=typescript&logoColor=white)
[![License: GPL v3][license-badge]][license]
[![CI][ci-badge]][ci]

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
  - Configurable keyboard shortcuts with game-binding conflict detection
  - Adjustable UI scale (50–200%), applied live to the running game
  - Auto-open chat sidebar on game start
  - Auto-expand the job queue panel on game start
  - Configurable system view zoom distance and ship name visibility range
  - Auto-open cargo view for hostiles, armadas, stations, and player ships
  - Skip loot box reveal animation (enabled by default)
  - Skip the first popup after game start (enabled by default)
  - One-button combat flow: press the Main Action shortcut repeatedly to select the next interceptable hostile and
    attack it without using the mouse
  - Configurable Main Action shortcut with support for keyboard keys and extra mouse buttons
  - Toast banner suppression with per-type opt-out (combat, station, armada, etc.)
  - Automatic STFC update detection via the Scopely update API
- **Native cross-platform app** (Tauri 2 + Vue 3)
  - Unified launcher: entitlement patching on macOS, DLL proxy injection on Windows
  - Process monitoring with automatic detection of game and launcher activity
  - System tray integration with minimize-to-tray and quit protection
  - Live WebSocket bridge that syncs settings to the running game in real time
  - Signed Daystrom updates with explicit installation and staged rollout
  - One-click rollback to the verified predecessor, including its bundled mod and settings

## Installation

Download the latest release for your platform from the
[Releases page](https://github.com/MBurchard/project-daystrom/releases/latest).

- **macOS**: Download the `.dmg` file, open it, and drag the app to your Applications folder.
- **Windows**: Download the `.exe` installer and run it. If Windows SmartScreen shows a warning, click
  "More info" and then "Run anyway" (the app is self-signed, not yet verified by Microsoft).

After installation, launch Project Daystrom and click the play button to start the game with the mod.

Daystrom offers signed application updates in the main window and installs them only after confirmation. A running game
remains open while Daystrom updates and reconnects. When a verified predecessor is available, the same window offers a
one-click rollback.

## Acknowledgements

This project was originally inspired by the [STFC Community Mod](https://github.com/netniV/stfc-mod) by
[netniV](https://github.com/netniV), [tashcan](https://github.com/tashcan), and contributors. Daystrom has since
moved to its own Rust-based mod with a custom hook engine and profile system.

## Built with

- [Tauri 2](https://tauri.app/) (Rust backend and native shell)
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
- [pnpm](https://pnpm.io/) >= 11
- [Rust](https://www.rust-lang.org/tools/install) (stable)

### macOS

- **Apple Silicon** — local development assumes arm64; the CI handles universal builds
- Xcode Command Line Tools (`xcode-select --install`)

### Windows

- **Visual Studio Build Tools 2022** (or VS Community) — workload "Desktop development with C++"
  including a **Windows SDK** (not installed by default!)
- Rust: standard installation via [rustup-init.exe](https://rustup.rs/) (option 1 selects MSVC toolchain)

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

| Script                                     | Description                                           |
|--------------------------------------------|-------------------------------------------------------|
| `pnpm install:all`                         | Force-install all workspace dependencies              |
| `pnpm lint`                                | Run ESLint across the entire project                  |
| `pnpm lint:fix`                            | Run ESLint with auto-fix                              |
| `pnpm typecheck`                           | TypeScript + Rust type checks                         |
| `pnpm test`                                | Run tooling, mod, frontend, and backend tests         |
| `pnpm test:app`                            | Run all app tests (frontend + backend)                |
| `pnpm test:app:frontend`                   | Run frontend tests only (vitest)                      |
| `pnpm test:app:backend`                    | Run backend tests only (cargo test + ts-rs bindings)  |
| `pnpm test:app:frontend:watch`             | Run frontend tests in watch mode                      |
| `pnpm test:app:frontend:coverage`          | Run frontend tests with v8 coverage                   |
| `pnpm test:app:backend:coverage`           | Run backend tests with llvm-cov coverage              |
| `pnpm check:mod:dump -- <paths>`           | Check IL2CPP dumps against the compatibility manifest |
| `pnpm release:verify -- <macOS> <Windows>` | Require compatible platform dumps before release      |
| `pnpm build`                               | Build everything (mod dylib → Tauri app)              |
| `pnpm build:mod`                           | Build mod dylib and copy to `app/resources/mod/`      |
| `pnpm build:app`                           | Build mod dylib + Tauri app bundle                    |
| `pnpm icons`                               | Generate Tauri icons from `resources/daystrom.png`    |
| `pnpm dev`                                 | Build mod + start Tauri app with hot reload           |

### Path Aliases

| Alias          | Resolves to                   |
|----------------|-------------------------------|
| `@app/*`       | `modules/app/src/*`           |
| `@generated/*` | `modules/app/src/generated/*` |
| `@resources/*` | `resources/*`                 |

## Release maintenance

Updater-enabled releases require Windows, Apple, and Tauri signing credentials. See
[release-signing.md](docs/release-signing.md) for credential setup and [auto-update.md](docs/auto-update.md) for the
release, update, rollout, and rollback contract.

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
Plugins live in `modules/plugins/` and are loaded by the main app. The architecture is intentionally modular so that
individual plugins can be developed and maintained independently.

### Environment Variables

| Variable                           | Default             | Description                                           |
|------------------------------------|---------------------|-------------------------------------------------------|
| `DAYSTROM_UPDATE_ENDPOINT`         | Configured endpoint | Debug only: override the Daystrom update manifest URL |
| `DAYSTROM_UPDATE_INTERVAL_SECONDS` | `21600`             | Debug only: set the periodic update-check interval    |

## Licence

This project is licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html).

[license-badge]: https://img.shields.io/badge/License-GPLv3-blue.svg?logo=gnu&logoColor=white
[license]: https://www.gnu.org/licenses/gpl-3.0
[ci-badge]: https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml/badge.svg
[ci]: https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml
