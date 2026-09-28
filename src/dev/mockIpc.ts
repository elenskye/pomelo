// DEV-ONLY in-memory backend so the UI can be exercised in a plain browser: `pnpm dev` → http://localhost:1420/?mock
// Mirrors the Rust command surface loosely (not the source of truth — Rust rules/tests are).
// Keep in sync when adding commands. Never imported in production builds (guarded in main.tsx).
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

type Args = Record<string, unknown>;
type Row = Record<string, unknown> & { id: number; title: string; status: string; finishedMonth: string | null };

const now = () => new Date().toISOString();
const month = () => now().slice(0, 7);
const settings = new Map<string, unknown>();
let nextId = 1;

/** In-memory `<prefix>_list/_create/_update/_delete` with the Rust patch semantics (incl. auto month). */
function collection(prefix: string, defaults: Record<string, unknown>, seed: Record<string, unknown>[]) {
  const make = (p: Record<string, unknown>): Row =>
    ({ id: nextId++, title: "", status: "unmarked", rating: null, finishedMonth: null, note: null, country: null, ...defaults, createdAt: now(), updatedAt: now(), ...p }) as Row;
  let rows = seed.map(make);
  const apply = (old: Row | null, patch: Args): Row => {
    const r = { ...(old ?? make({})), ...patch, updatedAt: now() } as Row;
    if (!r.title?.trim()) throw "名称不能为空";
    r.title = r.title.trim();
    if (r.status === "done" && old?.status !== "done" && !r.finishedMonth && !("finishedMonth" in patch)) r.finishedMonth = month();
    return r;
  };
  return {
    [`${prefix}_list`]: () => [...rows].sort((a, b) => a.id - b.id),
    [`${prefix}_create`]: (a: Args) => {
      const r = apply(null, a.patch as Args);
      rows.push(r);
      return r;
    },
    [`${prefix}_update`]: (a: Args) => {
      const i = rows.findIndex((r) => r.id === a.id);
      if (i < 0) throw "找不到这条记录，可能已经被删除了";
      rows[i] = apply(rows[i], a.patch as Args);
      return rows[i];
    },
    [`${prefix}_delete`]: (a: Args) => {
      rows = rows.filter((r) => r.id !== a.id);
      return null;
    },
    [`${prefix}__count`]: () => rows.length,
    [`${prefix}__replace`]: (a: Args) => {
      rows = (a.rows as Record<string, unknown>[]).map(make);
      return rows.length;
    },
  };
}

const handlers: Record<string, (a: Args) => unknown> = {
  app_info: () => ({
    appVersion: "0.1.0-mock",
    isDev: true,
    dataDir: "/mock/Pomelo OS Dev",
    dbPath: "/mock/Pomelo OS Dev/pomelo.db",
    schemaVersion: 4,
    startupError: null,
  }),
  reveal_data_dir: () => null,
  settings_get: (a) => settings.get(a.key as string) ?? null,
  settings_set: (a) => void settings.set(a.key as string, a.value),
  ...collection("books", { author: null, kind: "fiction" }, [
    { title: "示例书 A", author: "作者甲", country: "中国", status: "done", rating: 5, finishedMonth: "2025-11" },
    { title: "示例书 B", author: "作者乙", country: "日本", status: "doing", rating: 3, kind: "nonfiction" },
    { title: "示例书 C", author: "作者甲", country: "中国", status: "todo", kind: "comic" },
    { title: "示例书 D", author: "作者丙 / 作者丁", country: "法国", note: "一条备注" },
  ]),
  ...collection("films", { director: null, kind: "movie" }, [
    { title: "示例电影", director: "导演甲", country: "美国", status: "done", rating: 5, finishedMonth: "2026-01" },
    { title: "示例剧集", director: "导演乙", country: "韩国", status: "doing", kind: "series" },
    { title: "示例动漫", director: "导演丙", country: "日本", kind: "anime" },
  ]),
  "plugin:dialog|open": () => "/mock/pomelo-import.json",
  import_preview: () => ({
    modules: [
      { key: "books", label: "摸鱼读读", incoming: 2, existing: handlers["books__count"]({}) },
      { key: "films", label: "好奇看看", incoming: 1, existing: handlers["films__count"]({}) },
    ],
    skipped: ["drawing"],
    backup: null,
  }),
  import_apply: () => {
    const existing = { books: handlers["books__count"]({}) as number, films: handlers["films__count"]({}) as number };
    handlers["books__replace"]({ rows: [{ title: "导入书 1", status: "done", rating: 4 }, { title: "导入书 2" }] });
    handlers["films__replace"]({ rows: [{ title: "导入电影 1" }] });
    return {
      modules: [
        { key: "books", label: "摸鱼读读", incoming: 2, existing: existing.books },
        { key: "films", label: "好奇看看", incoming: 1, existing: existing.films },
      ],
      skipped: ["drawing"],
      backup: existing.books + existing.films ? "/mock/backup.db" : null,
    };
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
