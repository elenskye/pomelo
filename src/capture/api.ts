// 速记小窗 (Rust src-tauri/src/capture.rs): its own window; the draft is a setting (not undoable), a saved
// note / task is one ⌘Z step, 问柚子 chats in a conversation of the window's own.
import { invoke } from "@tauri-apps/api/core";
import type { FileInfo } from "@/shared/attachments/api";

export type CaptureTab = "note" | "todo" | "ask" | "translate";
/** tab "" until one was picked; due null: today; group null: the first. */
export type Draft = { tab: CaptureTab | ""; note: string; todo: string; due: string | null; group: number | null; ask: string };
export type Snapshot = {
  draft: Draft;
  files: FileInfo[];
  pinned: boolean;
  today: string;
  tags: string[];
  groups: { id: number; name: string }[];
  /** Module ids hidden in the 模块坞. */
  hidden: string[];
};

export const EMPTY: Draft = { tab: "", note: "", todo: "", due: null, group: null, ask: "" };

export const captureApi = {
  load: () => invoke<Snapshot>("capture_load"),
  draft: (draft: Draft) => invoke<void>("capture_draft", { draft }),
  files: (ids: number[]) => invoke<FileInfo[]>("capture_files", { ids }),
  pin: (on: boolean) => invoke<void>("capture_pin", { on }),
  note: (content: string) => invoke<void>("capture_note", { content }),
  /** 存为 .md (not a note; the draft stays): the file's name. */
  saveMarkdown: (path: string, markdown: string) => invoke<string>("capture_save_markdown", { path, markdown }),
  todo: (title: string, due: string | null, group: number) => invoke<void>("capture_todo", { title, due, group }),
  undo: () => invoke<{ label: string | null }>("capture_undo"),
  /** The window's conversation (out of 柚子's list), if any. */
  chat: () => invoke<{ id: number; thinking: boolean; effort?: string | null } | null>("capture_chat"),
  chatThinking: (on: boolean) => invoke<void>("capture_chat_thinking", { on }),
  chatEffort: (effort: string | null) => invoke<void>("capture_chat_effort", { effort }),
  /** 新对话: the conversation goes. */
  chatClear: () => invoke<void>("capture_chat_clear"),
  /** 在 Pomelo 里继续对话: listed and opened on the 柚子 page; the window hides. */
  chatKeep: () => invoke<void>("capture_chat_keep"),
  /** Grows the window for a conversation, or back to its height before. */
  fit: (chat: boolean) => invoke<void>("capture_fit", { chat }),
  /** A record opens in the main window (`path` + the hit, as from 全局搜索). */
  open: (path: string, hit: unknown) => invoke<void>("capture_open", { path, hit }),
  hide: (refocus: boolean) => invoke<void>("capture_hide", { refocus }),
};
