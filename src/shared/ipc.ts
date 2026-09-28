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

export type ImportSummary = {
  modules: { key: string; label: string; incoming: number; existing: number }[];
  skipped: string[];
  backup: string | null;
};

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  importPreview: (path: string) => invoke<ImportSummary>("import_preview", { path }),
  importApply: (path: string, overwrite: boolean) => invoke<ImportSummary>("import_apply", { path, overwrite }),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  settingsGet: <T>(key: string) => invoke<T | null>("settings_get", { key }),
  settingsSet: (key: string, value: unknown) => invoke<void>("settings_set", { key, value }),
};

export const SETTING_LAST_ROUTE = "ui.last_route";
