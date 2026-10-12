// 小拾 (Picky): typed wrappers for src-tauri/src/picky. Rules (model control, selection, keys) live in Rust.
import { Channel, invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export type Mode = "real" | "mock";
export type Readiness = "idle" | "loading" | "ready";
export type Dot = "off" | "loading" | "ready" | "error";
export type Service = "openRouter" | "deepSeek" | "openAI";

export type LocalModel = { key: string; name: string; title: string; note: string | null; quant: string; alias: string; context: number; thinking: boolean; listed: boolean };

export type IssueAction =
  | { kind: "retryStart"; key: string }
  | { kind: "retryStop" }
  | { kind: "refresh" }
  | { kind: "openServerLog" }
  | { kind: "openCatalog" }
  | { kind: "openEmbeddingLog" };

export type Issue = { kind: "operation" | "statusCheck" | "catalog" | "embedding"; title: string; message: string; details: string | null; actions: IssueAction[] };

export type CloudModel = { id: string; name: string; apiModel: string; service: Service; contextWindow: number; maxOutputTokens: number };

export type PickyStatus = {
  mode: Mode;
  /** picky.db could not be opened. */
  error: string | null;
  local: {
    models: LocalModel[];
    catalogError: string | null;
    active: LocalModel | null;
    readiness: Readiness;
    served: string | null;
    working: boolean;
    issue: Issue | null;
    presentation: { dot: Dot; title: string; menuTitle: string; detail: string };
  };
  embedding: { state: "stopped" | "starting" | "running"; working: boolean; text: string; issue: Issue | null };
  /** "local" | "cloud:<id>" */
  selection: string;
  cloud: CloudModel[];
  keys: { service: Service; name: string; saved: boolean }[];
  generating: boolean;
  activeRun: number | null;
  activeSession: number | null;
  activeKind: "reply" | "summary" | null;
  /** The reply reading material in segments: where it is. */
  activeReading: ReadingProgress | null;
};

export type PickyInfo = { mode: Mode; dbPath: string; filesDir: string; schemaVersion: number | null; error: string | null };

export type Session = { id: number; title: string; updatedAt: string };

export type SessionRow = { id: number; title: string; projectId: number | null; pinned: boolean; archived: boolean; updatedAt: string };
export type ProjectRow = { id: number; name: string; instructions: string; sessions: SessionRow[]; files: number; includeGlobalMemories: boolean };
/** The conversation list, each part in display order. */
export type Library = { pinned: SessionRow[]; projects: ProjectRow[]; recent: SessionRow[]; archived: SessionRow[] };
/** "pinned" | "recent" | "archived" | "project:<id>" */
export type ListKey = string;
export type ChatHit = { sessionId: number; title: string; messageId: number | null; snippet: string | null; at: string; model: string | null; archived: boolean };

export type ChatMessage = {
  id: number;
  role: "user" | "assistant";
  content: string;
  reasoning: string;
  reasoningMs: number | null;
  state: "complete" | "streaming" | "cancelled" | "failed" | "interrupted";
  createdAt: string;
  model: string | null;
  error: string | null;
  /** 1-based position among the message's versions, and how many there are. */
  versionIndex: number;
  versionCount: number;
  /** Questions: attachments, then project files. */
  files: MessageFile[];
  /** Replies: notes from reading material in segments. */
  notes: Note[];
  /** Replies: the memories their run was given. */
  memories: UsedMemory[];
  /** Replies: what their run looked up in Pomelo. */
  queries: Query[];
  /** Page only: a reply reading material right now. */
  reading?: ReadingProgress | null;
};

/** A record in a lookup's result (opened like a 全局搜索 hit). */
export type QueryRecord = { module: string; kind: string; id: number; title: string; parent?: number | null; day?: string | null };
/** One lookup in Pomelo (picky/data.rs): the 「查了」 line and what the model got. */
export type Query = { module: string | null; label: string; state: "ok" | "blocked" | "failed" | "unsupported"; result: string; records: QueryRecord[] };
/** 设置 → 小拾: which modules may go to cloud models. */
export type CloudData = { module: string; label: string; allowed: boolean };
export type FileInfo = { id: number; name: string; format: "PDF" | "Markdown" | "文本"; pageCount: number | null; byteCount: number; deleted: boolean };
/** A file as a question used it; project files carry their file, version and label ("资料 · 第 2 版"). */
export type MessageFile = FileInfo & { fileId: number | null; version: number | null; label: string | null };
export type Note = { label: string; content: string };
export type ReadingProgress = { current: number; total: number; label: string };
export type IndexState =
  | { kind: "none" }
  | { kind: "queued" }
  | { kind: "building"; done: number; total: number }
  | { kind: "ready" }
  | { kind: "failed"; message: string }
  | { kind: "incomplete" };
export type ProjectFile = {
  id: number;
  projectId: number;
  currentVersion: number;
  indexed: boolean;
  /** Oldest first. */
  versions: { version: number; file: FileInfo; addedAt: string }[];
  index: IndexState;
};
export type Memory = {
  id: number;
  content: string;
  projectId: number | null;
  sharedProjectIds: number[];
  enabled: boolean;
  localOnly: boolean;
  source: "manual" | "command" | "import";
  sourceSessionId: number | null;
  notedOn: string;
  createdAt: string;
  updatedAt: string;
};
export type MemoryInput = { content: string; projectId: number | null; sharedProjectIds: number[]; enabled: boolean; localOnly: boolean };
export type UsedMemory = { memoryId: number | null; content: string; notedOn: string };
export type MemoryPlan = { off: "everywhere" | "conversation" | "nextMessage" | null; count: number; global: number; project: number; shared: number; skippedLocalOnly: number; conversationOff: boolean };
export const MEMORY_MAX = 300;
export type Passage = { fileId: number; artifactId: number; segmentOrdinal: number; label: string; content: string };
export type RetrievalPlan = { readyNames: string[]; pending: number };

export type StreamEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "reading"; current: number; total: number; label: string }
  | { kind: "note"; label: string; content: string }
  | ({ kind: "query" } & Query)
  | { kind: "done"; state: string; error: string | null; reasoningMs: number | null };

/** What a question brings besides its text. */
export type Extras = { attachments: number[]; files: number[]; passages: Passage[]; memories: UsedMemory[] };

export type Sent = { sessionId: number; runId: number; userMessageId: number; messageId: number };

export type Template = { id: number; title: string; category: string; body: string; pinned: boolean; lastUsedAt: string | null };
export type TemplateInput = { title: string; category: string; body: string };
export type Preset = { id: number; name: string; content: string };
export type Layer = { kind: "global" | "project" | "session" | "preset"; label: string; content: string };
/** A conversation's own instructions and purpose preset. */
export type SessionSetting = { content: string; presetId: number | null };
export type InstructionPreview = { layers: Layer[]; session: SessionSetting };
export type Assessment = { percent: number; nearLimit: boolean; dropped: number; fits: boolean; summarizable: boolean };
export type SummaryEvent = { kind: "done"; state: string; error: string | null };
export type StoredFile = FileInfo & { bytes: number; messages: number; projectFiles: number };
export type StorageUsage = { dbBytes: number; filesBytes: number; files: StoredFile[]; strays: { name: string; bytes: number }[]; emptySessions: number };

export const SERVICE_NAMES: Record<Service, string> = { openRouter: "OpenRouter", deepSeek: "DeepSeek", openAI: "OpenAI" };

export const pickyApi = {
  info: () => invoke<PickyInfo>("picky_info"),
  status: () => invoke<PickyStatus>("picky_status"),
  /** "local:<key>" (starts / switches if needed) or "cloud:<id>". */
  select: (choice: string) => invoke<void>("picky_select", { choice }),
  stopModel: () => invoke<void>("picky_model_stop"),
  refresh: () => invoke<void>("picky_refresh"),
  embedding: (action: "start" | "stop") => invoke<void>("picky_embedding", { action }),
  issueAction: (action: IssueAction) => invoke<void>("picky_issue_action", { action }),
  issueDismiss: (kind: Issue["kind"]) => invoke<void>("picky_issue_dismiss", { kind }),
  cloudModelSave: (model: CloudModel) => invoke<CloudModel>("picky_cloud_model_save", { model }),
  cloudModelDelete: (id: string) => invoke<void>("picky_cloud_model_delete", { id }),
  keySave: (service: Service, key: string) => invoke<void>("picky_cloud_key_save", { service, key }),
  keyRemove: (service: Service) => invoke<void>("picky_cloud_key_remove", { service }),
  sessions: () => invoke<Session[]>("picky_sessions"),
  messages: (sessionId: number) => invoke<ChatMessage[]>("picky_messages", { sessionId }),
  /** Returns at once; the reply arrives on `onEvent` (deltas, then one `done`). */
  /** `projectId`: where a new conversation (sessionId null) goes. */
  /** `instructions`: a draft's own choice (saved with the new conversation). */
  /** `memoryOff`: a draft's 「本会话关闭记忆」 (saved with the new conversation). */
  send: (sessionId: number | null, projectId: number | null, instructions: SessionSetting | null, memoryOff: boolean, text: string, extras: Extras, onEvent: (e: StreamEvent) => void) =>
    invoke<Sent>("picky_send", { sessionId, projectId, instructions, memoryOff, text, ...extras, onEvent: channel(onEvent) }),
  /** A new version of a question (edited) and its reply (its files come along). */
  edit: (messageId: number, text: string, passages: Passage[], memories: UsedMemory[], onEvent: (e: StreamEvent) => void) =>
    invoke<Sent>("picky_edit", { messageId, text, passages, memories, onEvent: channel(onEvent) }),
  /** A new version of a reply (regenerate / retry). */
  regenerate: (messageId: number, passages: Passage[], memories: UsedMemory[], onEvent: (e: StreamEvent) => void) =>
    invoke<Sent>("picky_regenerate", { messageId, passages, memories, onEvent: channel(onEvent) }),
  /** delta -1 / +1; returns the shown version. */
  switchVersion: (messageId: number, delta: -1 | 1) => invoke<number>("picky_switch_version", { messageId, delta }),
  uiGet: <T>(key: string) => invoke<T | null>("picky_ui_get", { key }),
  uiSet: (key: string, value: unknown) => invoke<void>("picky_ui_set", { key, value }),
  copy: (text: string) => invoke<void>("copy_text", { text }),
  library: () => invoke<Library>("picky_library"),
  rename: (id: number, title: string) => invoke<void>("picky_rename", { id, title }),
  pin: (ids: number[], pinned: boolean) => invoke<void>("picky_pin", { ids, pinned }),
  archive: (ids: number[], archived: boolean) => invoke<void>("picky_archive", { ids, archived }),
  /** projectId null = out of any project. */
  move: (ids: number[], projectId: number | null) => invoke<void>("picky_move", { ids, projectId }),
  remove: (ids: number[]) => invoke<void>("picky_delete", { ids }),
  projectCreate: (name: string, instructions: string, includeGlobalMemories: boolean) => invoke<number>("picky_project_create", { name, instructions, includeGlobalMemories }),
  projectUpdate: (id: number, name: string, instructions: string, includeGlobalMemories: boolean) =>
    invoke<void>("picky_project_update", { id, name, instructions, includeGlobalMemories }),
  projectDelete: (id: number) => invoke<void>("picky_project_delete", { id }),
  projectsReorder: (ids: number[]) => invoke<void>("picky_projects_reorder", { ids }),
  /** A drag: into `list` before / after a member (neither = top). False = nothing changed. */
  drop: (id: number, list: ListKey, before: number | null, after: number | null) => invoke<boolean>("picky_drop", { id, list, before, after }),
  search: (q: string, model: string | null, since: string | null, archived: boolean) => invoke<ChatHit[]>("picky_search", { q, model, since, archived }),
  searchModels: () => invoke<string[]>("picky_search_models"),
  /** Shows the version a message is on; returns its conversation. */
  reveal: (messageId: number) => invoke<number>("picky_reveal", { messageId }),
  copyConversation: (id: number) => invoke<void>("picky_copy_conversation", { id }),
  exportMarkdown: (id: number, path: string) => invoke<void>("picky_export_markdown", { id, path }),
  exportPackage: (id: number, path: string) => invoke<string>("picky_export_package", { id, path }),
  stop: (runId: number) => invoke<void>("picky_stop", { runId }),
  templates: () => invoke<Template[]>("picky_templates"),
  /** id null = new; returns its id. */
  templateSave: (id: number | null, template: TemplateInput) => invoke<number>("picky_template_save", { id, template }),
  templatePin: (id: number, pinned: boolean) => invoke<void>("picky_template_pin", { id, pinned }),
  templateDelete: (id: number) => invoke<void>("picky_template_delete", { id }),
  templateUsed: (id: number) => invoke<void>("picky_template_used", { id }),
  /** The layers for a conversation, or a draft (sessionId null) in a project with its own choice. */
  instructions: (sessionId: number | null, projectId: number | null, draft: SessionSetting | null) =>
    invoke<InstructionPreview>("picky_instructions", { sessionId, projectId, draft }),
  sessionInstructionsSave: (sessionId: number, setting: SessionSetting) => invoke<void>("picky_session_instructions_save", { sessionId, setting }),
  globalInstructions: () => invoke<string>("picky_global_instructions"),
  globalInstructionsSave: (content: string) => invoke<void>("picky_global_instructions_save", { content }),
  presets: () => invoke<Preset[]>("picky_presets"),
  presetSave: (id: number | null, name: string, content: string) => invoke<number>("picky_preset_save", { id, name, content }),
  presetDelete: (id: number) => invoke<void>("picky_preset_delete", { id }),
  /** null while a reply is written or no model is ready. */
  context: (sessionId: number, prompt: string) => invoke<Assessment | null>("picky_context", { sessionId, prompt }),
  /** Returns the run at once; the outcome arrives on `onEvent`. */
  summarize: (sessionId: number, onEvent: (e: SummaryEvent) => void) => invoke<number>("picky_summarize", { sessionId, onEvent: channel(onEvent) }),
  /** Stores a copy of a chosen / dropped file. */
  importFile: (path: string) => invoke<FileInfo>("picky_import_file", { path }),
  attachmentLibrary: () => invoke<FileInfo[]>("picky_attachment_library"),
  openFile: (id: number) => invoke<void>("picky_open_file", { id }),
  storage: () => invoke<StorageUsage>("picky_storage"),
  storageDeleteFile: (id: number) => invoke<void>("picky_storage_delete_file", { id }),
  storageDeleteStray: (name: string) => invoke<void>("picky_storage_delete_stray", { name }),
  storageCleanEmpty: () => invoke<number>("picky_storage_clean_empty"),
  projectFiles: (projectId: number) => invoke<ProjectFile[]>("picky_project_files", { projectId }),
  projectFileAdd: (projectId: number, artifactId: number) => invoke<number>("picky_project_file_add", { projectId, artifactId }),
  projectFileReplace: (fileId: number, artifactId: number) => invoke<void>("picky_project_file_replace", { fileId, artifactId }),
  projectFileRemove: (fileId: number) => invoke<void>("picky_project_file_remove", { fileId }),
  projectFileIndex: (fileId: number, indexed: boolean) => invoke<void>("picky_project_file_index", { fileId, indexed }),
  indexRebuild: (fileId: number) => invoke<void>("picky_index_rebuild", { fileId }),
  indexStop: (fileId: number) => invoke<void>("picky_index_stop", { fileId }),
  /** What a question would search (a conversation, or a draft in a project). */
  retrievalPlan: (sessionId: number | null, projectId: number | null, excludeFiles: number[]) =>
    invoke<RetrievalPlan>("picky_retrieval_plan", { sessionId, projectId, excludeFiles }),
  /** Passages for a question; an error means 8081 couldn't be used. */
  retrieve: (sessionId: number | null, projectId: number | null, question: string, excludeFiles: number[]) =>
    invoke<Passage[]>("picky_retrieve", { sessionId, projectId, question, excludeFiles }),
  memories: () => invoke<Memory[]>("picky_memories"),
  /** id null = new; returns its id. */
  memorySave: (id: number | null, memory: MemoryInput) => invoke<number>("picky_memory_save", { id, memory }),
  /** Confirmed /记住 entries. */
  remember: (entries: string[], projectId: number | null, localOnly: boolean, sessionId: number | null) => invoke<void>("picky_remember", { entries, projectId, localOnly, sessionId }),
  memoryFlag: (id: number, flag: "enabled" | "localOnly", on: boolean) => invoke<void>("picky_memory_flag", { id, flag, on }),
  memoryDelete: (id: number) => invoke<void>("picky_memory_delete", { id }),
  memoryEnabled: () => invoke<boolean>("picky_memory_enabled"),
  dataCloud: () => invoke<CloudData[]>("picky_data_cloud"),
  dataCloudSet: (module: string, allowed: boolean) => invoke<void>("picky_data_cloud_set", { module, allowed }),
  memoryEnabledSet: (on: boolean) => invoke<void>("picky_memory_enabled_set", { on }),
  conversationMemory: (sessionId: number, off: boolean) => invoke<void>("picky_conversation_memory", { sessionId, off }),
  /** Which memories the next reply may use (a conversation, or a draft with its own switch). */
  memoryPlan: (sessionId: number | null, projectId: number | null, draftOff: boolean, nextOff: boolean) =>
    invoke<MemoryPlan>("picky_memory_plan", { sessionId, projectId, draftOff, nextOff }),
  /** The memories for a question; an error means 8081 couldn't be used. */
  chooseMemories: (sessionId: number | null, projectId: number | null, draftOff: boolean, nextOff: boolean, question: string) =>
    invoke<UsedMemory[]>("picky_choose_memories", { sessionId, projectId, draftOff, nextOff, question }),
};

function channel<E>(onEvent: (e: E) => void) {
  const ch = new Channel<E>();
  ch.onmessage = onEvent;
  return ch;
}

export const STATUS_KEY = ["picky", "status"];

/** Model status, polled (faster while something is changing). Rust also keeps the menu bar current. */
export function usePickyStatus() {
  return useQuery({
    queryKey: STATUS_KEY,
    queryFn: pickyApi.status,
    // the window may be hidden (Pomelo stays in the menu bar); the call only reads memory
    refetchIntervalInBackground: true,
    refetchInterval: (q) => {
      const s = q.state.data;
      const busy = !!s && (s.local.working || s.local.readiness === "loading" || s.embedding.working || s.embedding.state === "starting");
      return busy ? 700 : s?.generating ? 1000 : 2500;
    },
  });
}

export function useRefreshStatus() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: STATUS_KEY });
}

/** What the model button says for the current selection. */
export function selectedLabel(s: PickyStatus): { title: string; dot: Dot | "cloud"; missingKey: boolean } {
  const id = s.selection.startsWith("cloud:") ? s.selection.slice(6) : null;
  const cloud = id ? s.cloud.find((m) => m.id === id) : undefined;
  if (cloud) return { title: cloud.name, dot: "cloud", missingKey: !s.keys.find((k) => k.service === cloud.service)?.saved };
  return { title: s.local.presentation.title, dot: s.local.presentation.dot, missingKey: false };
}

/** 小拾's own UI state (picky.db `ui.*`), like useSetting. */
export function usePickyUi<T>(key: string, fallback: T, valid: (v: unknown) => v is T) {
  const [value, setValue] = useState<T>(fallback);
  useEffect(() => {
    let live = true;
    pickyApi
      .uiGet<unknown>(key)
      .then((v) => live && valid(v) && setValue(v))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (v: T) => {
    setValue(v);
    pickyApi.uiSet(key, v).catch(() => {});
  };
  return [value, set] as const;
}
export const isBool = (v: unknown): v is boolean => typeof v === "boolean";
export const isNumbers = (v: unknown): v is number[] => Array.isArray(v) && v.every((x) => typeof x === "number");

export const LIBRARY_KEY = ["picky", "library"];
export const useLibrary = () => useQuery({ queryKey: LIBRARY_KEY, queryFn: pickyApi.library });

export const TEMPLATES_KEY = ["picky", "templates"];
export const useTemplates = () => useQuery({ queryKey: TEMPLATES_KEY, queryFn: pickyApi.templates });
export const PRESETS_KEY = ["picky", "presets"];
export const usePresets = () => useQuery({ queryKey: PRESETS_KEY, queryFn: pickyApi.presets });
/** Prefix of every instruction preview (invalidate after editing any layer). */
export const INSTRUCTIONS_KEY = ["picky", "instructions"];
export const LIBRARY_FILES_KEY = ["picky", "attachment-library"];
export const projectFilesKey = (projectId: number | null) => ["picky", "project-files", projectId];
/** A project's files, polled while an index is being built. */
export function useProjectFiles(projectId: number | null) {
  return useQuery({
    queryKey: projectFilesKey(projectId),
    queryFn: () => (projectId == null ? Promise.resolve([] as ProjectFile[]) : pickyApi.projectFiles(projectId)),
    refetchInterval: (q) => (q.state.data?.some((f) => f.index.kind === "queued" || f.index.kind === "building") ? 800 : false),
    refetchIntervalInBackground: true,
  });
}
export const currentFile = (f: ProjectFile) => f.versions.find((v) => v.version === f.currentVersion)?.file ?? null;
export const usableFile = (f: ProjectFile) => !!currentFile(f) && !currentFile(f)!.deleted;
export function byteText(n: number) {
  if (n < 1000) return `${n} B`;
  if (n < 1000000) return `${Math.ceil(n / 1000)} KB`;
  return `${(n / 1000000).toFixed(1)} MB`;
}
/** "12 页" / "35 KB" / "文件已删除". */
export const fileDetail = (f: FileInfo) => (f.deleted ? "文件已删除" : f.pageCount != null ? `${f.pageCount} 页` : byteText(f.byteCount));
export const FILE_EXTENSIONS = ["txt", "text", "md", "markdown", "pdf"];
export const MEMORIES_KEY = ["picky", "memories"];
export const useMemories = () => useQuery({ queryKey: MEMORIES_KEY, queryFn: pickyApi.memories });
export const MEMORY_ENABLED_KEY = ["picky", "memory-enabled"];
/** 「/记住 …」: each paragraph starting with it is one memory; the rest is still sent. Null = none. */
export function parseRemember(text: string): { entries: string[]; remainder: string } | null {
  const entries: string[] = [];
  const rest: string[] = [];
  for (const para of text.replace(/\r\n/g, "\n").split("\n\n")) {
    const t = para.trim();
    if (t.startsWith("/记住")) entries.push(t.slice(3).replace(/^[\s：:，,]+/, "").trim());
    else rest.push(para);
  }
  return entries.length ? { entries, remainder: rest.join("\n\n").trim() } : null;
}
