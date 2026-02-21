# Skynet — STFC Assistant

An assistant app and extended mod for [Star Trek Fleet Command](https://www.scopely.com/games/star-trek-fleet-command),
built on top of the fantastic [STFC Community Mod](https://github.com/netniV/stfc-mod) by netniV and contributors.

## What is this?

Skynet picks up where the Community Mod leaves off. The mod already provides essential quality-of-life
improvements — hotkeys, UI tweaks, zoom presets, data sync, and more. Skynet builds on that foundation
and adds:

- **A native app** (Tauri 2 + Vue 3) that runs alongside the game on macOS and Windows
- **A cross-platform launcher** replacing the platform-specific launchers (Swift on macOS, proxy DLL on Windows)
  with a single unified solution
- **Dashboard, alerts, and advisor plugins** for live fleet overview, event notifications, and upgrade
  recommendations

The mod code lives in `mod/` and is kept in sync with the upstream Community Mod. Improvements and bug
fixes flow both ways — anything useful to the broader community gets contributed back.

## Project Structure

```
skynet/
├── package.json            # Workspace root (orchestrating scripts)
├── pnpm-workspace.yaml     # Workspace config (members: app, scripts)
├── eslint.config.js        # Shared ESLint config (lints entire project)
├── tsconfig.base.json      # Shared TypeScript base config
├── scripts/                # Build and tooling scripts
│   ├── build.ts            #   Mod + app build orchestration
│   └── package.json        #   Script dependencies
├── mod/                    # STFC Community Mod (from netniV/stfc-mod)
│   ├── mods/               #   Mod patches (C++23, IL2CPP hooks)
│   ├── macos-launcher/     #   Original Swift launcher (being replaced)
│   ├── macos-dylib/        #   macOS injection helper
│   ├── win-proxy-dll/      #   Windows proxy DLL loader
│   └── xmake.lua           #   Build configuration
├── app/                    # Skynet app (Tauri 2 + Vue 3)
│   ├── modules/
│   │   ├── app/            #   Vue 3 frontend
│   │   ├── backend/        #   Tauri/Rust backend
│   │   └── plugins/        #   Feature plugins (dashboard, alerts, advisor)
│   ├── resources/          #   Shared assets (logo, icons)
│   └── package.json        #   App dependencies + app-local scripts
└── README.md
```

## Acknowledgements

This project would not exist without the work of [netniV](https://github.com/netniV),
[tashcan](https://github.com/tashcan), and the entire STFC Community Mod team. The mod code in `mod/`
is imported from [netniV/stfc-mod](https://github.com/netniV/stfc-mod) and kept as close to upstream
as practical, so that improvements can be shared with the community.

## Prerequisites

- [Node.js](https://nodejs.org/) >= 24 (pinned via `.nvmrc`)
- [pnpm](https://pnpm.io/) >= 10 (pinned via `packageManager` in root `package.json`)
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [XMake](https://xmake.io/) (for building the mod)
- [CMake](https://cmake.org/) (required by xmake to build C++ dependencies like spud)

## Setup

```sh
nvm use
pnpm install
```

All commands run from the **workspace root** unless noted otherwise.

## Building the mod (dylib)

The mod code lives in `mod/` and produces `libstfc-community-patch.dylib` — the shared library
that gets injected into the game via `DYLD_INSERT_LIBRARIES`.

```sh
pnpm build:mod
```

This builds only the dylib target (`stfc-community-patch`) and copies the result to
`app/resources/mod/`. The full `xmake build` would also try to build the original Swift launcher,
which we don't need — Skynet replaces it.

Alternatively, from the `mod/` directory directly:

```sh
xmake build -y stfc-community-patch
```

The built dylib lands at `mod/build/macosx/arm64/release/libstfc-community-patch.dylib` (~8 MB).

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
| `pnpm icons`                      | Generate Tauri icons from `resources/skynet.png`     |
| `pnpm dev`                        | Start Tauri app (Vite + Rust) with hot reload        |

### App-local (run from `app/` or via `pnpm --filter skynet-app`)

| Script             | Description                                         |
|--------------------|-----------------------------------------------------|
| `dev:frontend`     | Start Vite dev server only (browser on :1420)       |
| `preview:frontend` | Preview production build in browser                 |
| `build:frontend`   | Production build (icons + typecheck + Vite)         |

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

### Logging

Unified logging across frontend (TypeScript) and backend (Rust). The frontend uses
[@mburchard/bit-log](https://www.npmjs.com/package/@mburchard/bit-log) with a custom `TauriAppender`
that forwards log events to the Rust backend via `tauri-plugin-log` IPC:

```
{ISO 8601 timestamp} {LEVEL} [{loggerName}] ({origin}: {filePath}: {line}): {message}
```

Example output:

```
2026-02-20T16:50:03.399+01:00 INFO  [Startup             ] (Backend : lib.rs   :   28): Skynet 0.1.0 initialised
2026-02-20T16:50:04.112+01:00 DEBUG [Main                ] (Frontend: main.ts  :   13): Connected to backend
```

#### Frontend usage

```typescript
import {getLogger} from '@app/log';

const log = getLogger('Auth');
log.info('User logged in');
log.debug('Session details:', {token: '...'});
log.error('Login failed');
```

#### Log output targets

| Target          | Description                                                    |
|-----------------|----------------------------------------------------------------|
| Stdout          | Terminal / IDE console (frontend + backend via Rust formatter) |
| Browser console | Via bit-log `ConsoleAppender` (frontend logs only)             |
| Log file        | Platform log directory (see below)                             |

### App directories

All runtime data uses platform-standard locations based on the app identifier `mbur.skynet`:

| Purpose | macOS                                        | Windows                            |
|---------|----------------------------------------------|------------------------------------|
| Logs    | `~/Library/Logs/mbur.skynet/`                | `%LOCALAPPDATA%\mbur.skynet\logs\` |
| Config  | `~/Library/Application Support/mbur.skynet/` | `%APPDATA%\mbur.skynet\`           |

### Format constants (backend)

Adjustable in `modules/backend/src/logging.rs`:

| Constant            | Default | Description                                     |
|---------------------|---------|-------------------------------------------------|
| `LOGGER_NAME_WIDTH` | 20      | Display width for the `[loggerName]` column     |
| `FILE_PATH_WIDTH`   | 30      | Display width for the file path (mid-truncated) |

### Plugins

The app is designed around a plugin architecture. Each plugin is a self-contained Vue module
that provides a specific feature set:

| Plugin        | Purpose                                                          |
|---------------|------------------------------------------------------------------|
| **dashboard** | Live overview of fleet, resources, and base status               |
| **alerts**    | Configurable notifications for in-game events                    |
| **advisor**   | Recommendations for research, officer assignments, and upgrades  |

Plugins live in `modules/plugins/` and are loaded by the main app. The architecture is
intentionally modular so that individual plugins can be developed and published independently.

### Environment Variables

| Variable          | Default | Description                                              |
|-------------------|---------|----------------------------------------------------------|
| `SKYNET_DEVTOOLS` | `1`     | Set to `0` to suppress DevTools in debug builds          |

## License

This project is licensed under the [GNU General Public License v3.0](https://www.gnu.org/licenses/gpl-3.0.html),
the same license as the STFC Community Mod it builds upon.
