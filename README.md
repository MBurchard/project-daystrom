# Skynet — STFC Companion

Companion app for Star Trek Fleet Command, built with Tauri 2 + Vue 3.

## Prerequisites

- [Node.js](https://nodejs.org/) >= 24 (pinned via `.nvmrc`)
- [pnpm](https://pnpm.io/) >= 10 (pinned via `packageManager` in `package.json`)
- [Rust](https://www.rust-lang.org/tools/install) (stable)

## Setup

```sh
nvm use
pnpm install
```

## Scripts

| Script                | Description                                      |
|-----------------------|--------------------------------------------------|
| `pnpm lint`           | Run ESLint                                       |
| `pnpm lint:fix`       | Run ESLint with auto-fix                         |
| `pnpm typecheck`      | TypeScript check (Vue + TS)                      |
| `pnpm typecheck:rust` | Rust type check (`cargo check`)                  |
| `pnpm dev`            | Start Tauri app (Vite + Rust) with hot reload    |
| `pnpm dev:web`        | Start Vite dev server only (browser on :1420)    |
| `pnpm icons`          | Generate Tauri icons from `resources/skynet.png` |
| `pnpm build:web`      | Production build (icons + typecheck + Vite)      |
| `pnpm preview:web`    | Preview production build in browser              |
| `pnpm tauri <cmd>`    | Run any Tauri CLI command                        |

## Project Structure

```
companion/
├── modules/
│   ├── app/                 # Vue 3 frontend
│   │   ├── index.html
│   │   └── src/
│   ├── backend/             # Tauri/Rust backend
│   │   └── src/
│   └── plugins/             # Plugin modules (see below)
│       ├── dashboard/
│       ├── alerts/
│       └── advisor/
├── resources/               # Shared assets (logo, icons)
├── eslint.config.js
├── vite.config.ts
├── tsconfig.json
└── package.json
```

## Path Aliases

| Alias          | Resolves to         |
|----------------|---------------------|
| `@app/*`       | `modules/app/src/*` |
| `@resources/*` | `resources/*`       |

## Logging

Unified logging across frontend (TypeScript) and backend (Rust) via `tauri-plugin-log`,
formatted in the style of [bit-log](https://github.com/AmerBiro/bit-log):

```
{ISO 8601 timestamp} {LEVEL} [{loggerName}] ({origin}: {filePath}: {line}): {message}
```

Example output:

```
2026-02-20T16:50:03.399+01:00 INFO  [skynet_companion_lib] (Backend : lib.rs   :  120): Skynet 0.1.0 initialised
2026-02-20T16:50:04.112+01:00 DEBUG [Main                ] (Frontend: main.ts  :   13): Connected to backend
```

### Frontend usage

```typescript
import {createLogger} from '@app/log';

const log = createLogger('Auth');
log.info('User logged in');
log.debug('Session details:', {token: '...'});
log.error('Login failed');
```

### Log output targets

| Target          | Description                                                |
|-----------------|------------------------------------------------------------|
| Stdout          | Terminal / IDE console                                     |
| Browser console | Via `attachConsole()` — Rust and frontend logs in DevTools |
| Log file        | Platform log directory (see below)                         |

### App directories

All runtime data uses platform-standard locations based on the app identifier `mbur.skynet`:

| Purpose | macOS                                        | Windows                            |
|---------|----------------------------------------------|------------------------------------|
| Logs    | `~/Library/Logs/mbur.skynet/`                | `%LOCALAPPDATA%\mbur.skynet\logs\` |
| Config  | `~/Library/Application Support/mbur.skynet/` | `%APPDATA%\mbur.skynet\`           |

### Format constants (backend)

Adjustable in `modules/backend/src/lib.rs`:

| Constant            | Default | Description                                    |
|---------------------|---------|------------------------------------------------|
| `LOGGER_NAME_WIDTH` | 20      | Display width for the `[loggerName]` column    |
| `FILE_PATH_WIDTH`   | 30      | Display width for the file path (mid-truncated)|

## Plugins

The app is designed around a plugin architecture. Each plugin is a self-contained Vue module
that provides a specific feature set:

| Plugin        | Purpose                                                          |
|---------------|------------------------------------------------------------------|
| **dashboard** | Live overview of fleet, resources, and base status               |
| **alerts**    | Configurable notifications for in-game events                    |
| **advisor**   | Recommendations for research, officer assignments, and upgrades  |

Plugins live in `modules/plugins/` and are loaded by the main app. The architecture is
intentionally modular so that individual plugins can be developed and published independently.

## Environment Variables

| Variable          | Default | Description                                              |
|-------------------|---------|----------------------------------------------------------|
| `SKYNET_DEVTOOLS` | `1`     | Set to `0` to suppress DevTools in debug builds          |
