/**
 * Game detection result returned by the `get_game_status` Tauri command.
 * Mirrors the Rust `GameStatus` struct in `commands.rs`.
 */
export interface GameStatus {
  installed: boolean;
  install_dir: string | null;
  executable: string | null;
  entitlements_ok: boolean;
  granted_entitlements: string[];
  missing_entitlements: string[];
}
