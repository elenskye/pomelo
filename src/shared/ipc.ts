// Typed wrappers for Rust commands (src-tauri/src/lib.rs). UI never touches SQL.
import { invoke } from "@tauri-apps/api/core";

export type AppInfo = {
  appVersion: string;
  isDev: boolean;
  dataDir: string;
  dbPath: string;
  schemaVersion: number | null;
  startupError: string | null;
};

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  settingsGet: <T>(key: string) => invoke<T | null>("settings_get", { key }),
  settingsSet: (key: string, value: unknown) => invoke<void>("settings_set", { key, value }),
};

export const SETTING_LAST_ROUTE = "ui.last_route";
