# Project Daystrom

![Crafted with Rust](https://img.shields.io/badge/Crafted_with-Rust-000000?logo=rust&logoColor=white)
![Crafted with TypeScript](https://img.shields.io/badge/Crafted_with-TypeScript-3178C6?logo=typescript&logoColor=white)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg?logo=gnu&logoColor=white)](https://www.gnu.org/licenses/gpl-3.0)
[![CI](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml/badge.svg)](https://github.com/MBurchard/project-daystrom/actions/workflows/ci.yml)

An assistant app and extended mod for [Star Trek Fleet Command](https://www.scopely.com/games/star-trek-fleet-command),
built on top of the [STFC Community Mod](https://github.com/netniV/stfc-mod) by netniV and contributors.

## Acknowledgements

This project would not exist without the work of [netniV](https://github.com/netniV),
[tashcan](https://github.com/tashcan), and the entire STFC Community Mod team. The mod code in `stfc-mod/`
is imported from [netniV/stfc-mod](https://github.com/netniV/stfc-mod) and kept as close to upstream
as practical, so that improvements can be shared with the community.

## What is this?

Project Daystrom picks up where the Community Mod leaves off. The mod already provides essential quality-of-life
improvements — hotkeys, UI tweaks, zoom presets, data sync, and more. Project Daystrom builds on that foundation
and adds:

- **A native app** (Tauri 2 + Vue 3) that runs alongside the game on macOS and Windows
- **A cross-platform launcher** replacing the platform-specific launchers (Swift on macOS,
  proxy DLL on Windows) with a single unified solution that handles entitlement patching,
  mod injection, and game launch
- **Game update detection** via the Scopely update API, with in-app update prompts
- **Process monitoring** that automatically detects game and launcher activity
- **System tray integration** with minimize-to-tray and quit protection
- **Dashboard, alerts, and advisor plugins** (planned) for live fleet overview, event
  notifications, and upgrade recommendations

The mod code lives in `stfc-mod/` and is kept in sync with the upstream Community Mod. Improvements and bug
fixes flow both ways — anything useful to the broader community gets contributed back.

## Built with

- [Tauri 2](https://tauri.app/) (Rust backend)
- [Vue 3](https://vuejs.org/) + [Vite](https://vite.dev/) (frontend)
- [@mburchard/bit-log](https://www.npmjs.com/package/@mburchard/bit-log) (structured logging)

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
├── stfc-mod/               # STFC Community Mod (from netniV/stfc-mod)
│   ├── mods/               #   Mod patches (C++23, IL2CPP hooks)
│   ├── macos-launcher/     #   Original Swift launcher (being replaced)
│   ├── macos-dylib/        #   macOS injection helper
│   ├── win-proxy-dll/      #   Windows proxy DLL loader
│   └── xmake.lua           #   Build configuration
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
- [XMake](https://xmake.io/) (for building the mod)
- [CMake](https://cmake.org/) (required by xmake to build C++ dependencies like spud)

### macOS

- **Apple Silicon** — local development assumes arm64; the CI handles universal builds
- Xcode Command Line Tools (`xcode-select --install`)

### Windows

- **Visual Studio Build Tools 2022** (or VS Community) — workload "Desktop development with C++"
  including a **Windows SDK** (not installed by default!)
- xmake: `irm https://xmake.io/psget.text | iex` in PowerShell
- Rust: standard installation via [rustup-init.exe](https://rustup.rs/) (option 1 selects MSVC toolchain)

## Setup

```sh
nvm use
pnpm install
```

All commands run from the **workspace root** unless noted otherwise.

## Building the mod

The mod code lives in `stfc-mod/` and produces a shared library that gets injected into the game
(`libstfc-community-patch.dylib` on macOS, `stfc-community-patch.dll` on Windows).

```sh
pnpm build:mod
```

This configures xmake for the current platform, builds only the mod target (`stfc-community-patch`),
and copies the result to `app/resources/mod/`. The full `xmake build` would also try to build the
original Swift launcher, which we don't need — Project Daystrom replaces it.

## Scripts

### Workspace root (run from project root)

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
