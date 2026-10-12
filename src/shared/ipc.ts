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

export type ExportSummary = {
  path: string;
  modules: { key: string; label: string; incoming: number }[];
};

/** `store`: pomelo (life data) | picky (小拾). `label`: daily | manual | before-import | before-restore | before-v<N>. `createdAt`: local "YYYY-MM-DD HH:MM:SS". */
export type BackupInfo = { fileName: string; store: "pomelo" | "picky"; label: string; createdAt: string; size: number };

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  importPreview: (path: string) => invoke<ImportSummary>("import_preview", { path }),
  importApply: (path: string, overwrite: boolean) => invoke<ImportSummary>("import_apply", { path, overwrite }),
  exportTo: (path: string) => invoke<ExportSummary>("export_to", { path }),
  backupsList: () => invoke<BackupInfo[]>("backups_list"),
  backupNow: () => invoke<BackupInfo>("backup_now"),
  /** No-op (null) if today's daily snapshot exists. */
  backupDaily: () => invoke<BackupInfo | null>("backup_daily"),
  backupRestore: (fileName: string) => invoke<{ safety: BackupInfo }>("backup_restore", { fileName }),
  revealBackupsDir: () => invoke<void>("reveal_backups_dir"),
  revealDataDir: () => invoke<void>("reveal_data_dir"),
  /** URL (any scheme), local path, or bare address (→ https); rules in Rust. */
  openLink: (link: string) => invoke<void>("open_link", { link }),
  /** Day-change chores in Rust: finished check-ins completed, due 固定支出 posted. */
  dailySweep: () => invoke<{ closed: number; posted: number }>("daily_sweep"),
  /** label = the step undone / redone; null = nothing to do. */
  undo: () => invoke<{ label: string | null }>("undo_last"),
  redo: () => invoke<{ label: string | null }>("redo_last"),
  settingsGet: <T>(key: string) => invoke<T | null>("settings_get", { key }),
  settingsSet: (key: string, value: unknown) => invoke<void>("settings_set", { key, value }),
};

