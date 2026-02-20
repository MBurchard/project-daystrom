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
│   │   ├── src/
│   │   ├── Cargo.toml
│   │   └── tauri.conf.json
│   ├── common/src/          # Shared TypeScript types
│   └── plugins/             # Public plugin placeholders
│       ├── dashboard/
│       ├── alerts/
│       └── advisor/
├── resources/               # Shared assets (logo, images)
├── eslint.config.js
├── vite.config.ts
├── tsconfig.json
└── package.json
```

## Path Aliases

| Alias        | Resolves to              |
|--------------|--------------------------|
| `@app/*`     | `modules/app/src/*`      |
| `@common/*`  | `modules/common/src/*`   |
| `@resources/*` | `resources/*`          |
