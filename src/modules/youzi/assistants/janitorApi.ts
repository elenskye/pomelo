// DevJanitor (Rust youzi/assistants/janitor/): the scan's result, the ring chart, cleaning, settings.
import { invoke } from "@tauri-apps/api/core";
import { toYmd } from "@/shared/dates";

type JanitorScan = { running: boolean; phase: string; files: number; bytes: number; startedAt: number; error: string | null };

export type JanitorItem = {
  id: number;
  title: string;
  paths: string[];
  size: number;
  /** Newest file inside (unix seconds; 0 = none). */
  newest: number;
  project: string | null;
  checked: boolean;
  inUse: string | null;
  protected: string | null;
  note: string | null;
  tag: string | null;
  /** Duplicates: which group. */
  dup: number | null;
};

export type JanitorGroup = { id: string; name: string; rebuild: string | null; takes: string | null; network: boolean | null; size: number; items: JanitorItem[] };

type JanitorSummary = {
  scannedAt: number;
  took: number;
  disk: { total: number; free: number; scanned: number; system: number } | null;
  trash: { size: number; count: number };
  docker: { size: number; parts: { name: string; size: number; reclaimable: number }[] | null } | null;
  denied: number;
  fullDiskAccess: boolean;
  files: number;
};

export type JanitorOverview = {
  mode: "real" | "sandbox";
  home: string;
  scan: JanitorScan;
  summary: JanitorSummary | null;
  categories: JanitorGroup[];
  groups: JanitorGroup[];
  cleanable: number;
};

export type JanitorRing = { path: string; name: string; kind: "disk" | "dir" | "file" | "system"; size: number; files: number; denied: number; newest: number; rest: number; children: JanitorRing[] };
type JanitorRingView = { node: JanitorRing; crumbs: [string, string][] };

/** One row of a 开发环境 (Rust janitor/env.rs). `part`: inside another row, not added to the size. */
export type JanitorEnvItem = { title: string; version: string | null; path: string | null; size: number; by: string | null; default: boolean; packages: number | null; top: [string, number][]; tag: string | null; part: boolean; protected: string | null; admin: boolean; newest: number };
export type JanitorEnv = { id: string; name: string; size: number; note: string | null; items: JanitorEnvItem[] };

export type JanitorOutcome = { freed: number; done: number; failed: [string, string][] };
type JanitorMemory = { total: number; used: number; pressure: "normal" | "warn" | "critical"; top: { name: string; bytes: number }[] };
type JanitorCategory = { id: string; name: string; rebuild: string; takes: string; network: boolean; download: boolean };
/** `names`: my names for the built-in categories (id → name). */
export type JanitorSettings = { off: string[]; staleDays: number; agent?: JanitorChoice; names?: Record<string, string> };
/** What the Agent is told: the SKILL (mine to rewrite; `custom`: it's mine) and the rest, written each time. */
export type JanitorPrompt = { skill: string; custom: boolean; head: string; situation: string };
type JanitorSettingsView = { settings: JanitorSettings; categories: JanitorCategory[]; rules: JanitorRule[] };

/** The Agent's rules in 一键清理 (janitor/agent.rs `Rule`). */
export type JanitorRule = { id: number; name: string; pattern: string; recognize: string; rebuild: string; checked: boolean; createdAt: string };

type JanitorTrashItem = { path: string; why: string; size: number; protected: string | null; kind: string; done: boolean; error: string | null };
/** One of the Agent's cards (janitor/agent.rs `Card`): trash | command | rule. */
export type JanitorCard = {
  id: number;
  runId: number;
  kind: "trash" | "command" | "rule";
  state: "pending" | "running" | "done" | "failed" | "stopped" | "cancelled";
  title: string;
  items: JanitorTrashItem[];
  command: string;
  what: string;
  deletes: string;
  recoverable: string;
  frees: string;
  sudo: boolean;
  protected: string | null;
  rule: JanitorRule | null;
  matches: number;
  matchSize: number;
  output: string;
  code: number | null;
  error: string | null;
};
export type JanitorChoice = { model: string | null; thinking: boolean; effort?: string | null };
type JanitorAgent = { session: number; choice: JanitorChoice };

export const JANITOR_KEY = ["janitor"] as const;

export const janitorApi = {
  overview: () => invoke<JanitorOverview>("janitor_overview"),
  scan: (force: boolean) => invoke<JanitorScan>("janitor_scan", { force }),
  stop: () => invoke<void>("janitor_stop"),
  ring: (path: string) => invoke<JanitorRingView | null>("janitor_ring", { path }),
  clean: (ids: number[]) => invoke<JanitorOutcome>("janitor_clean", { ids }),
  trash: (ids: number[], protectedOk: boolean) => invoke<JanitorOutcome>("janitor_trash", { ids, protectedOk }),
  emptyTrash: () => invoke<JanitorOutcome>("janitor_empty_trash"),
  memory: () => invoke<JanitorMemory>("janitor_memory"),
  envs: () => invoke<JanitorEnv[]>("janitor_envs"),
  reveal: (path: string) => invoke<void>("janitor_reveal", { path }),
  settings: () => invoke<JanitorSettingsView>("janitor_settings"),
  settingsSet: (settings: JanitorSettings) => invoke<JanitorSettingsView>("janitor_settings_set", { settings }),
  openPrivacy: () => invoke<void>("janitor_open_privacy"),
  ruleDelete: (id: number) => invoke<JanitorSettingsView>("janitor_rule_delete", { id }),
  ruleRename: (id: number, name: string) => invoke<JanitorSettingsView>("janitor_rule_rename", { id, name }),
  agent: () => invoke<JanitorAgent>("janitor_agent"),
  agentNew: () => invoke<JanitorAgent>("janitor_agent_new"),
  agentSet: (choice: JanitorChoice) => invoke<JanitorAgent>("janitor_agent_set", { choice }),
  agentPrompt: () => invoke<JanitorPrompt>("janitor_agent_prompt"),
  /** null: back to the built-in SKILL. */
  agentSkill: (skill: string | null) => invoke<JanitorPrompt>("janitor_agent_skill", { skill }),
  card: (id: number) => invoke<JanitorCard>("janitor_card", { id }),
  cardTrash: (id: number, picked: number[], protectedOk: boolean) => invoke<JanitorCard>("janitor_card_trash", { id, picked, protectedOk }),
  cardRun: (id: number, protectedOk: boolean) => invoke<JanitorCard>("janitor_card_run", { id, protectedOk }),
  cardStop: (id: number) => invoke<JanitorCard>("janitor_card_stop", { id }),
  cardRule: (id: number) => invoke<JanitorCard>("janitor_card_rule", { id }),
  cardCancel: (id: number) => invoke<JanitorCard>("janitor_card_cancel", { id }),
};

/** Decimal, as Finder counts: 12.3 GB, 420 MB. */
export function bytes(n: number): string {
  if (n >= 1e12) return `${(n / 1e12).toFixed(1)} TB`;
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${n >= 1e8 ? Math.round(n / 1e6) : (n / 1e6).toFixed(1)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} KB`;
  return `${n} B`;
}

/** 「今天」「3 天前」「2 个月前」「1 年前」 (unix seconds; 0 = nothing). */
export function ago(t: number): string {
  if (!t) return "—";
  const days = Math.floor((Date.now() / 1000 - t) / 86400);
  if (days < 1) return "今天";
  if (days < 31) return `${days} 天前`;
  if (days < 365) return `${Math.floor(days / 30)} 个月前`;
  return `${Math.floor(days / 365)} 年前`;
}

export const dayOf = (t: number) => toYmd(new Date(t * 1000));

// ---- 备份 (Rust janitor/backup/) ----

type BackupRow = { path: string; size: number; mtime: number };
type BackupMove = { from: string; to: string; size: number };
type BackupHeld = { path: string; size: number; why: string };
/** A group of a comparison: how many, how big, the first 300. */
type BackupGroup<T> = { count: number; bytes: number; rows: T[] };
export type BackupDiff = { new: BackupGroup<BackupRow>; changed: BackupGroup<BackupRow>; moved: BackupGroup<BackupMove>; held: BackupGroup<BackupHeld>; recent: BackupGroup<BackupRow>; archived: BackupGroup<BackupRow>; same: number; sameBytes: number; denied: string[] };
/** `source`: a path on this Mac (virtual in the sandbox); `target`: relative to the drive's root. */
export type BackupTask = { id: number; source: string; target: string; excludes: string[]; last: string | null; diff: BackupDiff | null; problem: string | null };
type BackupDrive = { uuid: string; name: string; connected: boolean; fs: string | null; total: number; free: number; path: string | null; folders: string[] };
type BackupVolume = { name: string; uuid: string; fs: string; total: number; free: number };
type BackupRun = { running: boolean; filesTotal: number; filesDone: number; bytesTotal: number; bytesDone: number; task: string; current: string; ended: string | null };
type BackupRunTask = { id: number; source: string; target: string; files: number; bytes: number; updated: number; done: boolean };
export type BackupRecord = { id: number; drive: string; status: "running" | "done" | "stopped" | "failed"; tasks: BackupRunTask[]; files: number; bytes: number; errors: [string, string][]; note: string | null; startedAt: string; finishedAt: string | null };
export type BackupOverview = { mode: "real" | "sandbox"; home: string; drive: BackupDrive | null; volumes: BackupVolume[]; tasks: BackupTask[]; comparing: boolean; comparedAt: number | null; run: BackupRun; runs: BackupRecord[] };
export type BackupTaskInput = { id: number | null; source: string; target: string; excludes: string[] };

export const BACKUP_KEY = [...JANITOR_KEY, "backup"];

export const backupApi = {
  overview: () => invoke<BackupOverview>("janitor_backup_overview"),
  compare: () => invoke<BackupOverview>("janitor_backup_compare"),
  /** null: forget the drive. */
  drive: (uuid: string | null) => invoke<BackupOverview>("janitor_backup_drive", { uuid }),
  taskSave: (task: BackupTaskInput) => invoke<BackupOverview>("janitor_backup_task_save", { task }),
  taskDelete: (id: number) => invoke<BackupOverview>("janitor_backup_task_delete", { id }),
  start: (ids: number[]) => invoke<BackupOverview>("janitor_backup_start", { ids }),
  stop: () => invoke<void>("janitor_backup_stop"),
  /** Renames on the drive the moves picked (by their path on the Mac). */
  move: (id: number, paths: string[]) => invoke<{ done: number; failed: [string, string][]; overview: BackupOverview }>("janitor_backup_move", { id, paths }),
};
