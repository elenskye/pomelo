// DEV-ONLY in-memory backend so the UI can be exercised in a plain browser: `pnpm dev` → http://localhost:1420/?mock
// Mirrors the Rust command surface loosely (not the source of truth — Rust rules/tests are).
// Keep in sync when adding commands. Never imported in production builds (guarded in main.tsx).
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Book } from "@/modules/books/api";

type Args = Record<string, unknown>;
const now = () => new Date().toISOString();
const month = () => now().slice(0, 7);

const settings = new Map<string, unknown>();
let nextId = 1;
const book = (p: Partial<Book>): Book => ({
  id: nextId++,
  title: "",
  author: null,
  country: null,
  status: "unmarked",
  rating: null,
  finishedMonth: null,
  note: null,
  createdAt: now(),
  updatedAt: now(),
  ...p,
});
let books: Book[] = [
  book({ title: "示例书 A", author: "作者甲", country: "中国", status: "done", rating: 5, finishedMonth: "2025-11" }),
  book({ title: "示例书 B", author: "作者乙", country: "日本", status: "doing", rating: 3 }),
  book({ title: "示例书 C", author: "作者甲", country: "中国", status: "todo" }),
  book({ title: "示例书 D", author: "作者丙 / 作者丁", country: "法国", status: "unmarked", note: "一条备注" }),
];

function applyPatch(old: Book | null, patch: Partial<Book>): Book {
  const b = { ...(old ?? book({})), ...patch, updatedAt: now() };
  if (!b.title?.trim()) throw "书名不能为空";
  b.title = b.title.trim();
  if (b.status === "done" && old?.status !== "done" && !b.finishedMonth && !("finishedMonth" in patch)) b.finishedMonth = month();
  return b;
}

const handlers: Record<string, (a: Args) => unknown> = {
  app_info: () => ({
    appVersion: "0.1.0-mock",
    isDev: true,
    dataDir: "/mock/Pomelo OS Dev",
    dbPath: "/mock/Pomelo OS Dev/pomelo.db",
    schemaVersion: 2,
    startupError: null,
  }),
  reveal_data_dir: () => null,
  settings_get: (a) => settings.get(a.key as string) ?? null,
  settings_set: (a) => void settings.set(a.key as string, a.value),
  books_list: () => [...books].sort((x, y) => x.id - y.id),
  books_create: (a) => {
    const b = applyPatch(null, a.patch as Partial<Book>);
    books.push(b);
    return b;
  },
  books_update: (a) => {
    const i = books.findIndex((b) => b.id === a.id);
    if (i < 0) throw "找不到这本书，可能已经被删除了";
    books[i] = applyPatch(books[i], a.patch as Partial<Book>);
    return books[i];
  },
  books_delete: (a) => {
    books = books.filter((b) => b.id !== a.id);
    return null;
  },
  "plugin:dialog|open": () => "/mock/pomelo-import.json",
  import_preview: () => ({
    modules: [{ key: "books", label: "摸鱼读读", incoming: 2, existing: books.length }],
    skipped: ["films"],
    backup: null,
  }),
  import_apply: () => {
    const existing = books.length;
    books = [book({ title: "导入书 1", status: "done", rating: 4 }), book({ title: "导入书 2" })];
    return { modules: [{ key: "books", label: "摸鱼读读", incoming: 2, existing }], skipped: ["films"], backup: existing ? "/mock/backup.db" : null };
  },
};

export function installMockIpc() {
  mockWindows("main");
  mockIPC((cmd, payload) => {
    const h = handlers[cmd];
    if (!h) {
      if (cmd.startsWith("plugin:window|") || cmd.startsWith("plugin:event|")) return null;
      throw `mock: unknown command ${cmd}`;
    }
    return h((payload ?? {}) as Args);
  });
  console.info("[mock] in-memory IPC installed");
}
