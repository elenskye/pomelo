// DevJanitor (Rust youzi/assistants/janitor/): the scan's result, the ring chart, cleaning, settings.
import { invoke } from "@tauri-apps/api/core";

export type JanitorScan = { running: boolean; phase: string; files: number; bytes: number; startedAt: number; error: string | null };

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

export type JanitorSummary = {
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
export type JanitorRingView = { node: JanitorRing; crumbs: [string, string][] };

/** One row of a 开发环境 (Rust janitor/env.rs). `part`: inside another row, not added to the size. */
export type JanitorEnvItem = { title: string; version: string | null; path: string | null; size: number; by: string | null; default: boolean; packages: number | null; top: [string, number][]; tag: string | null; part: boolean; protected: string | null; admin: boolean; newest: number };
export type JanitorEnv = { id: string; name: string; size: number; note: string | null; items: JanitorEnvItem[] };

export type JanitorOutcome = { freed: number; done: number; failed: [string, string][] };
export type JanitorMemory = { total: number; used: number; pressure: "normal" | "warn" | "critical"; top: { name: string; bytes: number }[] };
export type JanitorCategory = { id: string; name: string; rebuild: string; takes: string; network: boolean; download: boolean };
/** `names`: my names for the built-in categories (id → name). */
export type JanitorSettings = { off: string[]; staleDays: number; agent?: JanitorChoice; names?: Record<string, string> };
/** What the Agent is told: the SKILL (mine to rewrite; `custom`: it's mine) and the rest, written each time. */
export type JanitorPrompt = { skill: string; custom: boolean; head: string; situation: string };
export type JanitorSettingsView = { settings: JanitorSettings; categories: JanitorCategory[]; rules: JanitorRule[] };

/** The Agent's rules in 一键清理 (janitor/agent.rs `Rule`). */
export type JanitorRule = { id: number; name: string; pattern: string; recognize: string; rebuild: string; checked: boolean; createdAt: string };

export type JanitorTrashItem = { path: string; why: string; size: number; protected: string | null; kind: string; done: boolean; error: string | null };
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
export type JanitorAgent = { session: number; choice: JanitorChoice };

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
export function ago(t: number, now = Date.now() / 1000): string {
  if (!t) return "—";
  const days = Math.floor((now - t) / 86400);
  if (days < 1) return "今天";
  if (days < 31) return `${days} 天前`;
  if (days < 365) return `${Math.floor(days / 30)} 个月前`;
  return `${Math.floor(days / 365)} 年前`;
}

export const dayOf = (t: number) => {
  const d = new Date(t * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
