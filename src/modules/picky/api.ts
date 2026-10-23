// 小拾 (Picky): typed wrappers for src-tauri/src/picky. Rules (model control, selection, keys) live in Rust.
import { Channel, invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

export type Mode = "real" | "mock";
export type Readiness = "idle" | "loading" | "ready";
export type Dot = "off" | "loading" | "ready" | "error";
export type Service = "openRouter" | "deepSeek" | "openAI" | "anthropic" | "google" | "aliyun";

export type LocalModel = { key: string; name: string; title: string; note: string | null; quant: string; alias: string; context: number; thinking: boolean; listed: boolean };

export type IssueAction =
  | { kind: "retryStart"; key: string }
  | { kind: "retryStop" }
  | { kind: "refresh" }
  | { kind: "openServerLog" }
  | { kind: "openCatalog" }
  | { kind: "openServiceLog"; service: ServiceKind };

export type Issue = { kind: "operation" | "statusCheck" | "catalog" | ServiceKind; title: string; message: string; details: string | null; actions: IssueAction[] };

/** The embedding (8081) and rerank (8082) services beside the chat model. */
export type ServiceKind = "embedding" | "rerank";
export type ServiceModel = { key: string; name: string; title: string; note: string | null; quant: string; context: number };
export type ServiceStatus = {
  state: "stopped" | "starting" | "running";
  working: boolean;
  text: string;
  issue: Issue | null;
  /** The chosen model (else the manifest's first of this kind); null when it lists none. */
  model: ServiceModel | null;
  /** The manifest's models of this kind. */
  models: ServiceModel[];
  /** Started by Pomelo: stopped after 30 idle minutes and when Pomelo quits. */
  ours: boolean;
};

/** What a model can do about thinking: 不能思考 / 能开关 / 只能思考. */
export type Think = "never" | "switch" | "always";
export type CloudModel = { id: string; name: string; apiModel: string; service: Service; contextWindow: number; maxOutputTokens: number; thinking: Think };

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
  embedding: ServiceStatus;
  rerank: ServiceStatus;
  /** "local" | "cloud:<id>" */
  selection: string;
  cloud: CloudModel[];
  keys: { service: Service; name: string; saved: boolean }[];
  /** Anything being written. */
  generating: boolean;
  /** What is being written, one per conversation. */
  active: ActiveRun[];
  /** Conversations from 定时任务 not opened yet (the dot on 小拾 in the sidebar). */
  unseen: number;
};

/** A reply or summary being written: model "local:<key>" | "cloud:<id>"; queued = waiting for the local model. */
export type ActiveRun = { run: number; session: number; kind: "reply" | "summary"; model: string; reading: ReadingProgress | null; queued: boolean };

export type PickyInfo = { mode: Mode; dbPath: string; filesDir: string; schemaVersion: number | null; error: string | null };

export type Session = { id: number; title: string; updatedAt: string };

/** `scheduled`: written by a 定时任务; `unseen`: not opened since; `private`: 私密 (年度总结 and 小拾's search leave it out); `thinking`: its 思考 switch;
 *  `knowledge`: its 知识库 switch, `libraries` the ones it chose (none: its project's, then all). */
export type SessionRow = { id: number; title: string; projectId: number | null; pinned: boolean; archived: boolean; updatedAt: string; scheduled: boolean; unseen: boolean; private: boolean; thinking: boolean; knowledge: boolean; libraries: number[] };
/** `libraries`: the 知识库 its conversations search (none: all). */
export type ProjectRow = { id: number; name: string; instructions: string; sessions: SessionRow[]; files: number; includeGlobalMemories: boolean; libraries: number[] };
/** The conversation list, each part in display order. */
export type Library = { pinned: SessionRow[]; projects: ProjectRow[]; scheduled: SessionRow[]; recent: SessionRow[]; archived: SessionRow[] };
/** "pinned" | "scheduled" | "recent" | "archived" | "project:<id>" */
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
  /** Replies: what their run was given, numbered for its [n] citations (picky/cite.rs). */
  sources: Source[];
  /** Replies: the card of what 小拾 proposed to write (picky/act.rs). */
  card?: Card | null;
  /** Questions asked in the 小窗: the page they went with. */
  page: Query | null;
  /** Questions: the skills they asked for (「/书单检索」, 联网), as names. */
  skills?: string[];
  /** Older questions: the file they picked to change (改文件 before the 工具箱). */
  editFile?: EditTarget | null;
  /** Replies: the changes they proposed to it (work.rs). */
  edit?: FileEdit | null;
  /** Page only: a reply reading material right now. */
  reading?: ReadingProgress | null;
  /** Being written, waiting for the local model. */
  queued?: boolean;
};

/** A record in a lookup's result (opened like a 全局搜索 hit). */
export type QueryRecord = { module: string; kind: string; id: number; title: string; parent?: number | null; day?: string | null };
/** One lookup in Pomelo (picky/data.rs): the 「查了」 line and what the model got. */
export type Query = { module: string | null; label: string; state: "ok" | "blocked" | "failed" | "unsupported"; result: string; records: QueryRecord[] };
/** A numbered source of a reply: a record (opens like a 全局搜索 hit), a project file or 知识库 passage (shows its text) or a web page. */
export type Source = {
  n: number;
  kind: "record" | "passage" | "web";
  module: string | null;
  recordKind: string | null;
  id: number | null;
  parent: number | null;
  day: string | null;
  artifactId: number | null;
  segment: number | null;
  title: string;
  detail: string | null;
  content: string;
  /** A web page (kind web: 联网搜索, 书单 / 影单检索), opened in the browser. */
  url?: string | null;
  /** A 知识库 passage: its source and file (relative to the folder; "" for a web page). */
  kbSource?: number | null;
  kbPath?: string | null;
};
/** 小拾's proposals to write (picky/act.rs): one tick each; confirming writes the ticked ones in one ⌘Z step. */
export type CardField = { label: string; before?: string | null; after: string; long?: boolean };
/** A written record (opened like a 全局搜索 hit); gone = taken back (⌘Z) or deleted. */
export type CardWritten = { module: string; kind: string; id: number; parent: number | null; day: string | null; gone: boolean };
/** Several records fit an edit: pick one (after confirming: `chosen`, its title and fields on the item). */
export type CardOption = { title: string; fields: CardField[] };
export type CardItem = { position: number; tool: string; module: string; label: string; title: string; fields: CardField[]; options: CardOption[] | null; chosen: number | null; checked: boolean; written: CardWritten | null };
export type CardPick = { position: number; option: number | null };
export type Card = { runId: number; state: "pending" | "confirmed" | "cancelled"; items: CardItem[] };
/** What a page tells the 小窗 (ids only; Rust fetches the content under its rules). */
export type PageContextPayload = { module: string; page: string | null; filter: string | null; record: { kind: string; id: number } | null; selected: { kind: string; id: number }[] };
/** 设置 → 小拾: which modules may go to cloud models. */
export type CloudData = { module: string; label: string; allowed: boolean };
export type FileFormat = "PDF" | "Markdown" | "文本" | "Word" | "PowerPoint" | "Excel" | "CSV" | "图片" | "网页";
/** pageCount: PDF pages / PowerPoint slides; rowCount: a table's rows; url: a saved web page's address. */
export type FileInfo = { id: number; name: string; format: FileFormat; pageCount: number | null; byteCount: number; deleted: boolean; rowCount: number | null; url: string | null };
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
/** `kb`: from a 知识库 (its source, file and library). */
export type Passage = { fileId: number; artifactId: number; segmentOrdinal: number; label: string; content: string; kb?: { source: number; path: string; library: string } | null };
/** `knowledge`: the conversation's 知识库 when its switch is on — files ready / still to update, sources not sent to a cloud model. */
export type RetrievalPlan = { readyNames: string[]; pending: number; knowledge: KbPlan | null };
export type KbPlan = { libraries: string[]; ready: number; pending: number; skipped: string[] };
/** A new conversation's 知识库 switch and libraries (saved with its first question). */
export type KbDraft = { on: boolean; libraries: number[] };
export type KbFailed = { path: string; error: string };
/** 改文件 (work.rs): the file a question asked to change, and the reply's proposed changes to it. */
export type EditTarget = { path: string; name: string };
export type EditItem = { position: number; paragraph: number; before: string; after: string; checked: boolean };
/** kind new-word (Word 工具箱 create): name, path = the folder it goes to (then the file), the Markdown in items[0].after. */
export type FileEdit = { runId: number; path: string; name: string; kind: "markdown" | "text" | "word" | "new-word"; total: number; state: "pending" | "written" | "cancelled"; error: string | null; items: EditItem[] };
/** 翻译 (assistants/translate.rs): its settings (`model` null = 小拾现在选的; `keywords`: group → choice). */
export type TranslateSettings = { source: string; target: string; model: string | null; thinking: boolean; prompt: string; keywords: Record<string, string> };
/** Pub2MD (assistants/pub2md/): how hard a model thinks. */
export type Level = "off" | "low" | "medium" | "high";
export type Pub2mdPart = "front" | "abstract" | "body" | "conclusion" | "acknowledgments" | "references" | "appendix";
export type Pub2mdFormat = "docx" | "pdf" | "latex" | "html";
/** A document picked but not sent yet (images together are one). */
export type Pub2mdStaged = { name: string; files: string[] };
export type Pub2mdSettings = {
  steps: "extract" | "clean" | "translate";
  md: boolean;
  englishOnly: boolean;
  worker: string | null;
  workerLevel: Level;
  supervisor: string | null;
  supervisorLevel: Level;
  /** cloud:<id> asked when a content check turns a request down; "" = 不用. */
  backup: string;
  parallel: number;
  folder: string | null;
  /** 英文 → 简体中文 / 简体 → 繁体 / 繁体 → 简体 */
  lang: "enZh" | "zhHant" | "hantZh";
  style: string;
  /** 领域: several for an interdisciplinary text; none = 通用. */
  domains: string[];
  prompt: string;
  /** 联网查术语; null = the Supervisor's service decides (`webDefault`). */
  web: boolean | null;
  refine: boolean;
  /** Parts not translated (front: only the title is). */
  skip: Pub2mdPart[];
  /** 另存为 (Mathpix converts at the end). */
  formats: Pub2mdFormat[];
};
export type Pub2mdRecent = { name: string; folder: string; at: string; state: "running" | "done" | "stopped" | "failed"; message: string | null; outputs: string[] };
export type Pub2mdProgress = { name: string; folder: string; step: string; chunks: number; done: number; workerRequests: number; supervisorRequests: number; elapsed: number; models: string | null };
export type Pub2mdView = {
  settings: Pub2mdSettings;
  recent: Pub2mdRecent[];
  queue: string[];
  current: Pub2mdProgress | null;
  mathpix: boolean;
  mock: boolean;
  folder: string;
  styles: [string, string][];
  domains: [string, string][];
  webDefault: boolean;
  tavily: boolean;
};
export type TranslateGroup = { key: string; name: string; choices: { key: string; label: string; says: string }[] };
/** The last time (input, output, how it ended) and, while it runs, its status. */
export type TranslateView = {
  settings: TranslateSettings;
  input: string;
  output: string;
  error: string | null;
  note: string | null;
  modelName: string | null;
  running: boolean;
  status: string | null;
  lists: { languages: { code: string; name: string }[]; groups: TranslateGroup[] };
};
/** A folder or saved web page of a library, and how its index stands. */
export type KbSource = {
  id: number;
  libraryId: number;
  kind: "folder" | "web";
  name: string;
  path: string | null;
  url: string | null;
  artifactId: number | null;
  deleted: boolean;
  cloud: boolean;
  files: number;
  images: number;
  ready: number;
  pending: number;
  failed: KbFailed[];
  building: boolean;
  missing: boolean;
  truncated: boolean;
};
export type KbLibrary = { id: number; name: string; sources: KbSource[] };
export type KbOverview = { libraries: KbLibrary[]; error: string | null };

export type StreamEvent =
  | { kind: "reasoning"; text: string }
  | { kind: "content"; text: string }
  | { kind: "reading"; current: number; total: number; label: string }
  | { kind: "note"; label: string; content: string }
  | ({ kind: "query" } & Query)
  | { kind: "sources"; list: Source[] }
  | { kind: "card"; card: Card }
  | { kind: "edit"; edit: FileEdit }
  | { kind: "queued"; on: boolean }
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
export type StoredFile = FileInfo & { bytes: number; messages: number; projectFiles: number; knowledge: number };
export type StorageUsage = { dbBytes: number; filesBytes: number; files: StoredFile[]; strays: { name: string; bytes: number }[]; emptySessions: number };

export const SERVICE_NAMES: Record<Service, string> = { openRouter: "OpenRouter", deepSeek: "DeepSeek", openAI: "OpenAI", anthropic: "Anthropic", google: "Google", aliyun: "Aliyun" };

export const pickyApi = {
  info: () => invoke<PickyInfo>("picky_info"),
  status: () => invoke<PickyStatus>("picky_status"),
  /** "local:<key>" (starts / switches if needed) or "cloud:<id>". */
  select: (choice: string) => invoke<void>("picky_select", { choice }),
  stopModel: () => invoke<void>("picky_model_stop"),
  refresh: () => invoke<void>("picky_refresh"),
  service: (service: ServiceKind, action: "start" | "stop") => invoke<void>("picky_service", { service, action }),
  serviceModel: (service: ServiceKind, key: string) => invoke<void>("picky_service_model", { service, key }),
  issueAction: (action: IssueAction) => invoke<void>("picky_issue_action", { action }),
  issueDismiss: (kind: Issue["kind"]) => invoke<void>("picky_issue_dismiss", { kind }),
  cloudModelSave: (model: CloudModel) => invoke<CloudModel>("picky_cloud_model_save", { model }),
  cloudModelDelete: (id: string) => invoke<void>("picky_cloud_model_delete", { id }),
  /** The whole list in its new order (the model menu follows it). */
  cloudModelsReorder: (ids: string[]) => invoke<void>("picky_cloud_models_reorder", { ids }),
  keySave: (service: Service, key: string) => invoke<void>("picky_cloud_key_save", { service, key }),
  keyRemove: (service: Service) => invoke<void>("picky_cloud_key_remove", { service }),
  // ---- v0.4 phase 6: 插件库 (skills/) ----
  skills: () => invoke<Skill[]>("picky_skills"),
  skillSet: (key: string, enabled: boolean | null, auto: boolean | null) => invoke<Skill[]>("picky_skill_set", { key, enabled, auto }),
  skillKeySave: (account: string, key: string) => invoke<Skill[]>("picky_skill_key_save", { account, key }),
  skillKeyRemove: (account: string) => invoke<Skill[]>("picky_skill_key_remove", { account }),
  skillContactSet: (contact: string) => invoke<Skill[]>("picky_skill_contact_set", { contact }),
  sessions: () => invoke<Session[]>("picky_sessions"),
  messages: (sessionId: number) => invoke<ChatMessage[]>("picky_messages", { sessionId }),
  /** Returns at once; the reply arrives on `onEvent` (deltas, then one `done`). */
  /** `projectId`: where a new conversation (sessionId null) goes. */
  /** `instructions`: a draft's own choice (saved with the new conversation). */
  /** `memoryOff`: a draft's 「本会话关闭记忆」; `thinking`: its 思考 switch (both saved with the new conversation). */
  /** `page`: the page it is asked on (the 小窗). */
  /** `skills`: what the question asks for (「/书单检索」, 联网; keys). */
  /** `knowledge`: a draft's 知识库 switch and libraries (saved with the new conversation). */
  send: (sessionId: number | null, projectId: number | null, instructions: SessionSetting | null, memoryOff: boolean, thinking: boolean | null, text: string, extras: Extras, page: PageContextPayload | null, skills: string[], knowledge: KbDraft | null, onEvent: (e: StreamEvent) => void) =>
    invoke<Sent>("picky_send", { sessionId, projectId, instructions, memoryOff, thinking, text, ...extras, page, skills, knowledge, onEvent: channel(onEvent) }),
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
  schedules: () => invoke<Schedule[]>("picky_schedules"),
  /** id null = a new one; returns its id. */
  scheduleSave: (id: number | null, input: ScheduleInput) => invoke<number>("picky_schedule_save", { id, input }),
  scheduleEnable: (id: number, enabled: boolean) => invoke<void>("picky_schedule_enable", { id, enabled }),
  scheduleDelete: (id: number) => invoke<void>("picky_schedule_delete", { id }),
  /** 立即运行: starts in the background; the conversation appears under 定时. */
  scheduleRun: (id: number) => invoke<void>("picky_schedule_run", { id }),
  /** A 定时 conversation was opened: its dot goes. */
  seen: (sessionId: number) => invoke<void>("picky_seen", { sessionId }),
  brief: () => invoke<Brief>("picky_brief"),
  /** force false: only when there's none today (首页 on showing); true: 重写 / 重试. Returns at once. */
  briefWrite: (force: boolean) => invoke<Brief>("picky_brief_write", { force }),
  yearCard: () => invoke<YearLetter | null>("picky_year_card"),
  briefSet: (enabled: boolean, model: string | null, thinking: boolean) => invoke<Brief>("picky_brief_set", { enabled, model, thinking }),
  rename: (id: number, title: string) => invoke<void>("picky_rename", { id, title }),
  pin: (ids: number[], pinned: boolean) => invoke<void>("picky_pin", { ids, pinned }),
  archive: (ids: number[], archived: boolean) => invoke<void>("picky_archive", { ids, archived }),
  setPrivate: (ids: number[], priv: boolean) => invoke<void>("picky_private", { ids, private: priv }),
  /** projectId null = out of any project. */
  move: (ids: number[], projectId: number | null) => invoke<void>("picky_move", { ids, projectId }),
  remove: (ids: number[]) => invoke<void>("picky_delete", { ids }),
  projectCreate: (name: string, instructions: string, includeGlobalMemories: boolean, libraries: number[]) => invoke<number>("picky_project_create", { name, instructions, includeGlobalMemories, libraries }),
  projectUpdate: (id: number, name: string, instructions: string, includeGlobalMemories: boolean, libraries: number[]) =>
    invoke<void>("picky_project_update", { id, name, instructions, includeGlobalMemories, libraries }),
  projectDelete: (id: number) => invoke<void>("picky_project_delete", { id }),
  projectsReorder: (ids: number[]) => invoke<void>("picky_projects_reorder", { ids }),
  /** A drag: into `list` before / after a member (neither = top). False = nothing changed. */
  drop: (id: number, list: ListKey, before: number | null, after: number | null) => invoke<boolean>("picky_drop", { id, list, before, after }),
  search: (q: string, model: string | null, since: string | null, archived: boolean) => invoke<ChatHit[]>("picky_search", { q, model, since, archived }),
  searchModels: () => invoke<string[]>("picky_search_models"),
  /** 意思相近 in the search above the conversation list (may start the embedding service). */
  confirmCard: (runId: number, picks: CardPick[]) => invoke<Card>("picky_card_confirm", { runId, picks }),
  cancelCard: (runId: number) => invoke<Card>("picky_card_cancel", { runId }),
  searchSimilar: (q: string, model: string | null, since: string | null, archived: boolean) => invoke<ChatHit[]>("picky_search_similar", { q, model, since, archived }),
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
  /** A pasted image (⌘V). */
  importImage: (name: string, bytes: number[]) => invoke<FileInfo>("picky_import_image", { name, bytes }),
  /** A web page by address: its article's text is saved. */
  importWeb: (url: string) => invoke<FileInfo>("picky_import_web", { url }),
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
  /** What a question would search (a conversation, or a draft in a project with its 知识库 `knowledge`). */
  retrievalPlan: (sessionId: number | null, projectId: number | null, excludeFiles: number[], knowledge: KbDraft | null) =>
    invoke<RetrievalPlan>("picky_retrieval_plan", { sessionId, projectId, excludeFiles, knowledge }),
  /** Passages for a question (project files, then the 知识库); an error means 8081 couldn't be used. */
  retrieve: (sessionId: number | null, projectId: number | null, question: string, excludeFiles: number[], knowledge: KbDraft | null) =>
    invoke<Passage[]>("picky_retrieve", { sessionId, projectId, question, excludeFiles, knowledge }),
  // ---- v0.4.5 阶段 9: Work in conversations (work.rs) ----
  fileEditConfirm: (runId: number, positions: number[]) => invoke<FileEdit>("picky_file_edit_confirm", { runId, positions }),
  fileEditCancel: (runId: number) => invoke<FileEdit>("picky_file_edit_cancel", { runId }),
  /** Saves a new Word document a reply proposed (never over an existing file). */
  fileCreateConfirm: (runId: number, folder: string, name: string) => invoke<FileEdit>("picky_file_create_confirm", { runId, folder, name }),
  wordDefaults: (messageId: number) => invoke<{ folder: string; name: string }>("picky_word_defaults", { messageId }),
  /** The path written (never over an existing file). */
  saveWord: (messageId: number, folder: string, name: string) => invoke<string>("picky_save_word", { messageId, folder, name }),
  // ---- v0.4.5 阶段 8: 助手 · 翻译 (assistants/translate.rs) ----
  translate: () => invoke<TranslateView>("picky_translate"),
  translateSettings: (settings: TranslateSettings) => invoke<TranslateView>("picky_translate_settings", { settings }),
  translateInput: (input: string) => invoke<void>("picky_translate_input", { input }),
  translatePreview: (settings: TranslateSettings) => invoke<string>("picky_translate_preview", { settings }),
  /** Returns at once; poll `translate` while it runs. */
  translateRun: (input: string, settings: TranslateSettings) => invoke<TranslateView>("picky_translate_run", { input, settings }),
  translateStop: () => invoke<TranslateView>("picky_translate_stop"),
  // ---- v0.6: 助手 · Pub2MD (assistants/pub2md/) ----
  pub2md: () => invoke<Pub2mdView>("picky_pub2md"),
  pub2mdSettings: (settings: Pub2mdSettings) => invoke<Pub2mdView>("picky_pub2md_settings", { settings }),
  /** Picked or dropped files as the documents they will be; nothing starts. */
  pub2mdGroup: (paths: string[]) => invoke<Pub2mdStaged[]>("picky_pub2md_group", { paths }),
  /** 发送 (images together are one document); returns at once, poll `pub2md`. */
  pub2mdAdd: (paths: string[]) => invoke<Pub2mdView>("picky_pub2md_add", { paths }),
  pub2mdResume: (folder: string) => invoke<Pub2mdView>("picky_pub2md_resume", { folder }),
  pub2mdUnqueue: (index: number) => invoke<Pub2mdView>("picky_pub2md_unqueue", { index }),
  pub2mdStop: () => invoke<Pub2mdView>("picky_pub2md_stop"),
  /** A document's folder, or one of its files. */
  /** A stopped or failed document out of the list, its folder to the Trash. */
  pub2mdDelete: (folder: string) => invoke<Pub2mdView>("picky_pub2md_delete", { folder }),
  pub2mdOpen: (folder: string, file?: string) => invoke<void>("picky_pub2md_open", { folder, file: file ?? null }),
  pub2mdKeysSave: (id: string, key: string) => invoke<Pub2mdView>("picky_pub2md_keys_save", { id, key }),
  pub2mdKeysRemove: () => invoke<Pub2mdView>("picky_pub2md_keys_remove"),
  /** An .mmd / .md output with its images inlined, for the preview. */
  pub2mdPreview: (folder: string, file: string) => invoke<string>("picky_pub2md_preview", { folder, file }),
  /** What the two models are told with these settings: [who · what, the rules]. */
  pub2mdPrompts: (settings: Pub2mdSettings) => invoke<[string, string][]>("picky_pub2md_prompts", { settings }),
  // ---- v0.4.5 阶段 7: 知识库 (knowledge.rs) ----
  kbOverview: () => invoke<KbOverview>("picky_kb_overview"),
  kbLibraryCreate: (name: string) => invoke<number>("picky_kb_library_create", { name }),
  kbLibraryRename: (id: number, name: string) => invoke<void>("picky_kb_library_rename", { id, name }),
  kbLibraryDelete: (id: number) => invoke<void>("picky_kb_library_delete", { id }),
  kbLibrariesReorder: (ids: number[]) => invoke<void>("picky_kb_libraries_reorder", { ids }),
  kbFolderAdd: (libraryId: number, path: string) => invoke<number>("picky_kb_folder_add", { libraryId, path }),
  /** After importWeb saved the page. */
  kbWebAdd: (libraryId: number, artifactId: number) => invoke<number>("picky_kb_web_add", { libraryId, artifactId }),
  kbSourceRemove: (id: number) => invoke<void>("picky_kb_source_remove", { id }),
  kbSourceCloud: (id: number, on: boolean) => invoke<void>("picky_kb_source_cloud", { id, on }),
  kbSourceRebuild: (id: number) => invoke<void>("picky_kb_source_rebuild", { id }),
  kbScan: () => invoke<void>("picky_kb_scan"),
  /** A folder's file (path "" = the folder), or a web page in the browser. */
  kbOpen: (sourceId: number, path: string) => invoke<void>("picky_kb_open", { sourceId, path }),
  conversationKnowledge: (sessionId: number, on: boolean) => invoke<void>("picky_conversation_knowledge", { sessionId, on }),
  /** [] = follow the project (or all); choosing also turns the switch on. */
  conversationLibraries: (sessionId: number, libraryIds: number[]) => invoke<void>("picky_conversation_libraries", { sessionId, libraryIds }),
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
  conversationThinking: (sessionId: number, on: boolean) => invoke<void>("picky_conversation_thinking", { sessionId, on }),
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
      const busy = !!s && (s.local.working || s.local.readiness === "loading" || [s.embedding, s.rerank].some((x) => x.working || x.state === "starting"));
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
export const isStrings = (v: unknown): v is string[] => Array.isArray(v) && v.every((x) => typeof x === "string");

/** 定时任务 (rules in Rust schedule.rs). every: day | week | month; day: 周几 1–7 / 几号 1–31; at: "HH:MM";
 * model: local:<key> | cloud:<id> | null (= 小拾现在选的). next: "YYYY-MM-DD HH:MM" (null when off). */
export type ScheduleRule = { every: "day" | "week" | "month" | "year"; day: number | null; month?: number | null; at: string };
export type ScheduleInput = ScheduleRule & { name: string; prompt: string; model: string | null; thinking: boolean };
export type Schedule = ScheduleInput & { id: number; enabled: boolean; builtin: string | null; next: string | null; lastSession: number | null; lastAt: string | null; running: boolean };
export const SCHEDULES_KEY = ["picky", "schedules"];
export const useSchedules = () => useQuery({ queryKey: SCHEDULES_KEY, queryFn: pickyApi.schedules, refetchInterval: (q) => (q.state.data?.some((s) => s.running) ? 1500 : 30_000) });

const WEEKDAY_NAMES = ["一", "二", "三", "四", "五", "六", "日"];
/** 「每周一 09:00」 */
export function ruleText(r: ScheduleRule): string {
  if (r.every === "week") return `每周${WEEKDAY_NAMES[(r.day ?? 1) - 1]} ${r.at}`;
  if (r.every === "month") return `每月 ${r.day ?? 1} 号 ${r.at}`;
  if (r.every === "year") return `每年 ${r.month ?? 1} 月 ${r.day ?? 1} 日 ${r.at}`;
  return `每天 ${r.at}`;
}

/** 今日简报 (rules in Rust brief.rs). state: none | writing | done | failed; content stays while rewriting / after a failure. */
export type Brief = {
  enabled: boolean;
  model: string | null;
  thinking: boolean;
  day: string;
  content: string;
  state: "none" | "writing" | "done" | "failed";
  error: string | null;
  modelName: string | null;
  leftOut: string[];
  phase: "starting" | "writing" | null;
};
export const BRIEF_KEY = ["picky", "brief"];
export const useBrief = (enabled = true) =>
  useQuery({ queryKey: BRIEF_KEY, queryFn: pickyApi.brief, enabled, refetchInterval: (q) => (q.state.data?.state === "writing" ? 1500 : false) });

/** 年度总结 (Rust yearly.rs): 首页 shows last year's in the new year's first week. */
export type YearLetter = { year: string; sessionId: number; content: string };
export const YEAR_CARD_KEY = ["picky", "year-card"];
export const useYearCard = () => useQuery({ queryKey: YEAR_CARD_KEY, queryFn: pickyApi.yearCard });

export const LIBRARY_KEY = ["picky", "library"];
export const useLibrary = () => useQuery({ queryKey: LIBRARY_KEY, queryFn: pickyApi.library });

/** A skill of the 插件库 (rules in Rust skills/). */
export type SkillKey = { account: string; label: string; required: boolean; saved: boolean };
/** `contact`: what 书单 / 影单检索 send to Wikidata / Open Library (one setting for both). */
export type Skill = { key: string; name: string; command: string; summary: string; detail: string; enabled: boolean; auto: boolean; ready: boolean; keys: SkillKey[]; usesContact: boolean; contact: string | null };
export const SKILLS_KEY = ["picky", "skills"];
export const useSkills = () => useQuery({ queryKey: SKILLS_KEY, queryFn: pickyApi.skills });
/** The skill the 联网 button asks for. */
export const WEB_SKILL = "web";

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
export const fileDetail = (f: FileInfo) =>
  f.deleted
    ? "文件已删除"
    : f.url
      ? (f.url.split("://")[1] ?? f.url).split("/")[0].replace(/^www\./, "")
      : f.rowCount != null
        ? `${f.rowCount} 行`
        : f.pageCount != null
          ? `${f.pageCount} 页`
          : byteText(f.byteCount);
export const FILE_EXTENSIONS = ["txt", "text", "md", "markdown", "pdf", "docx", "doc", "pptx", "xlsx", "xlsm", "xls", "ods", "csv", "png", "jpg", "jpeg", "gif", "webp", "heic", "heif"];
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
