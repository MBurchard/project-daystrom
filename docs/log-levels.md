# Log Levels

Both the game mod and the Daystrom app write log files. The default log level is **Info**. Per-target overrides can be configured in `settings.toml` under the `[log_levels]` section.

## Configuration

Add a `[log_levels.game]` and/or `[log_levels.app]` section to your `settings.toml`:

```toml
[log_levels.game]
PlayerPrefs = "Debug"

[log_levels.app]
Settings = "Debug"
WebSocket = "Trace"
```

Valid levels (case-insensitive): `Off`, `Error`, `Warn`, `Info`, `Debug`, `Trace`.

Targets not listed here use the default level (Info). Changes take effect on the next start.

### Settings file location

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/mbur.project-daystrom/settings.toml` |
| Windows | `%APPDATA%/mbur.project-daystrom/settings.toml` |

## Game mod targets (`[log_levels.game]`)

| Target | Description |
|---|---|
| `ChatFrame` | Chat sidebar auto-open and resize |
| `HookEngine` | Hook installation, IL2CPP init |
| `HookSafety` | Hook panic detection and deactivation |
| `Hotkeys` | Space bar actions (engage, mine, warp) |
| `PlayerData` | User profile tracking |
| `PlayerPrefs` | PlayerPrefs get/set/has/delete operations |
| `Settings` | Game settings sync and updates |
| `Trace` | PlayerPrefs trace mode (deduped, all operations) |
| `UiScale` | UI scale factor detection and application |

## Daystrom app targets (`[log_levels.app]`)

| Target | Description |
|---|---|
| `Commands` | Tauri command invocations |
| `Entitlements` | Steam/Epic entitlement checks |
| `Game` | Game process detection |
| `GameDetect` | Platform-specific game detection |
| `GameState` | Game/launcher running state |
| `Launcher` | Launcher process management |
| `MacHooks` | macOS-specific window/quit hooks |
| `Monitor` | Background monitoring loop |
| `Profiles` | Player profile state |
| `Settings` | App settings load/save/events |
| `Startup` | Application bootstrap |
| `Version` | Game version checks |
| `WebSocket` | WebSocket IPC with game mod |
