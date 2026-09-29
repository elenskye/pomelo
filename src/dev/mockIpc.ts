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

// ---- kinds ----
type Kind = { id: number; module: string; name: string; sortOrder: number };
let kinds: Kind[] = [];
const addKind = (module: string, name: string) => {
  const k = { id: nextId++, module, name, sortOrder: kinds.filter((x) => x.module === module).length + 1 };
  kinds.push(k);
  return k;
};
(
  [
    ["books", ["虚构", "非虚构", "漫画", "其他"]],
    ["films", ["电影", "剧集", "动漫"]],
    ["drawing", ["正课", "DLC 跟画", "LIVE 辅导"]],
    ["packing", ["重要物品", "随身", "护理", "兴趣"]],
  ] as const
).forEach(([m, names]) => names.forEach((n) => addKind(m, n)));
const kindId = (module: string, name: string) => kinds.find((k) => k.module === module && k.name === name)!.id;
const itemsUsing: Record<string, () => { kindId: unknown }[]> = {};

function applyPatch<T extends Row>(old: T, patch: Args, titleLabel = "名称"): T {
  const r = { ...old, ...patch, updatedAt: now() } as T;
  if (!r.title?.trim()) throw `${titleLabel}不能为空`;
  r.title = r.title.trim();
  if (r.status === "done" && old.status !== "done" && !r.finishedMonth && !("finishedMonth" in patch)) r.finishedMonth = month();
  return r;
}

/** In-memory `<prefix>_list/_create/_update/_delete` with the Rust patch semantics (incl. auto month). */
function collection(prefix: string, defaults: Record<string, unknown>, seed: Record<string, unknown>[]) {
  const make = (p: Record<string, unknown>): Row =>
    ({ id: nextId++, title: "", status: "unmarked", rating: null, finishedMonth: null, note: null, country: null, kindId: null, ...defaults, createdAt: now(), updatedAt: now(), ...p }) as Row;
  let rows = seed.map(make);
  itemsUsing[prefix] = () => rows as never;
  return {
    [`${prefix}_list`]: () => [...rows].sort((a, b) => a.id - b.id),
    [`${prefix}_create`]: (a: Args) => {
      const r = applyPatch({ ...make({}), status: "unmarked" }, a.patch as Args);
      rows.push(r);
      return r;
    },
    [`${prefix}_update`]: (a: Args) => {
      const i = rows.findIndex((r) => r.id === a.id);
      if (i < 0) throw "找不到这条记录，可能已经被删除了";
      rows[i] = applyPatch(rows[i], a.patch as Args);
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

// ---- drawing ----
type Course = { id: number; name: string; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
let courses: Course[] = [];
let lessons: Row[] = [];
itemsUsing.drawing = () => lessons as never;
const addCourse = (name: string, note: string | null = null) => {
  const c = { id: nextId++, name, note, sortOrder: courses.length + 1, createdAt: now(), updatedAt: now() };
  courses.push(c);
  return c;
};
const addLesson = (courseId: number, p: Record<string, unknown>) => {
  const l = {
    id: nextId++, courseId, code: null, kindId: kindId("drawing", "正课"), title: "", status: "unmarked", finishedMonth: null, note: null,
    sortOrder: lessons.filter((x) => x.courseId === courseId).length + 1, createdAt: now(), updatedAt: now(), ...p,
  } as Row;
  lessons.push(l);
  return l;
};
const withCounts = (c: Course) => {
  const ls = lessons.filter((l) => l.courseId === c.id);
  return { ...c, total: ls.length, done: ls.filter((l) => l.status === "done").length, doing: ls.filter((l) => l.status === "doing").length };
};
{
  const a = addCourse("示例课程 A");
  ["绘画大地图", "工具介绍", "线条 - 控笔", "线条 - 排线"].forEach((t, i) =>
    addLesson(a.id, { title: t, code: String(i), status: i === 0 ? "done" : i === 1 ? "doing" : "todo", finishedMonth: i === 0 ? "2026-08" : null }),
  );
  addLesson(a.id, { title: "跟画：种子", kindId: kindId("drawing", "DLC 跟画"), status: "todo" });
  addLesson(a.id, { title: "辅导 - 运笔", kindId: kindId("drawing", "LIVE 辅导"), status: "todo" });
  const b = addCourse("示例课程 B", "下半年再学");
  addLesson(b.id, { title: "人体比例", code: "1", status: "todo" });
  addCourse("空课程");
}

// ---- packing ----
type Tpl = { id: number; name: string; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
type TripRow = { id: number; name: string; templateId: number | null; departOn: string | null; note: string | null; createdAt: string; updatedAt: string };
type PItem = { id: number; parentId: number; kindId: number | null; name: string; note: string | null; packed?: boolean; sortOrder: number; createdAt: string; updatedAt: string };
let tpls: Tpl[] = [];
let tripRows: TripRow[] = [];
let tplItems: PItem[] = [];
let tripItems: PItem[] = [];
itemsUsing.packing = () => [...tplItems, ...tripItems] as never;
const pk = (name: string) => kindId("packing", name);
const addTpl = (name: string, note: string | null = null) => {
  const t = { id: nextId++, name, note, sortOrder: tpls.length + 1, createdAt: now(), updatedAt: now() };
  tpls.push(t);
  return t;
};
const addPItem = (rows: PItem[], parentId: number, p: Partial<PItem>) => {
  const it = { id: nextId++, parentId, kindId: null, name: "", note: null, sortOrder: rows.filter((r) => r.parentId === parentId).length + 1, createdAt: now(), updatedAt: now(), ...p };
  rows.push(it);
  return it;
};
const tplOut = (t: Tpl) => ({ ...t, itemCount: tplItems.filter((i) => i.parentId === t.id).length });
const tripOut = (t: TripRow) => {
  const its = tripItems.filter((i) => i.parentId === t.id);
  return { ...t, templateName: tpls.find((x) => x.id === t.templateId)?.name ?? null, total: its.length, packed: its.filter((i) => i.packed).length };
};
const createTripRow = (patch: Args, templateId: number | null) => {
  if (!String(patch.name ?? "").trim()) throw "出行名称不能为空";
  const t = { id: nextId++, name: String(patch.name).trim(), templateId, departOn: (patch.departOn as string) ?? null, note: (patch.note as string) ?? null, createdAt: now(), updatedAt: now() };
  tripRows.push(t);
  tplItems.filter((i) => i.parentId === templateId).forEach((i) => addPItem(tripItems, t.id, { kindId: i.kindId, name: i.name, note: i.note, packed: false }));
  return t;
};
{
  const home = addTpl("回家");
  [["身份证", "重要物品"], ["家钥匙 🔑", "重要物品"], ["充电宝", "随身"], ["画纸 & 画笔", "兴趣"], ["纸巾", "护理"]].forEach(([n, k]) => addPItem(tplItems, home.id, { name: n, kindId: pk(k) }));
  const abroad = addTpl("出国", "护照有效期要半年以上");
  addPItem(tplItems, abroad.id, { name: "护照 / 签证", kindId: pk("重要物品") });
  addPItem(tplItems, abroad.id, { name: "护肤品", kindId: pk("护理"), note: "随身最多 1L" });
  const t = createTripRow({ name: "2026 国庆 · 回家", departOn: "2026-10-01" }, home.id);
  tripItems.filter((i) => i.parentId === t.id).slice(0, 2).forEach((i) => (i.packed = true));
}
const pItemHandlers = (list: "template" | "trip") => {
  const rows = () => (list === "template" ? tplItems : tripItems);
  const parentKey = list === "template" ? "templateId" : "tripId";
  const check = (p: Args) => {
    if (p.name !== undefined && !String(p.name).trim()) throw "物品不能为空";
  };
  return {
    [`packing_${list}_items`]: (a: Args) => rows().filter((i) => i.parentId === a[parentKey]).sort((x, y) => x.sortOrder - y.sortOrder),
    [`packing_${list}_item_create`]: (a: Args) => {
      const p = a.patch as Args;
      check({ name: p.name ?? "" });
      return addPItem(rows(), a[parentKey] as number, { ...(p as Partial<PItem>), name: String(p.name).trim(), ...(list === "trip" ? { packed: !!p.packed } : {}) });
    },
    [`packing_${list}_item_update`]: (a: Args) => {
      check(a.patch as Args);
      const it = rows().find((i) => i.id === a.id)!;
      Object.assign(it, a.patch, { updatedAt: now() });
      return it;
    },
    [`packing_${list}_item_delete`]: (a: Args) => {
      if (list === "template") tplItems = tplItems.filter((i) => i.id !== a.id);
      else tripItems = tripItems.filter((i) => i.id !== a.id);
      return null;
    },
  };
};

const handlers: Record<string, (a: Args) => unknown> = {
  app_info: () => ({
    appVersion: "0.1.0-mock",
    isDev: true,
    dataDir: "/mock/Pomelo OS Dev",
    dbPath: "/mock/Pomelo OS Dev/pomelo.db",
    schemaVersion: 7,
    startupError: null,
  }),
  reveal_data_dir: () => null,
  settings_get: (a) => settings.get(a.key as string) ?? null,
  settings_set: (a) => void settings.set(a.key as string, a.value),

  kinds_list: (a) =>
    kinds
      .filter((k) => k.module === a.module)
      .sort((x, y) => x.sortOrder - y.sortOrder)
      .map((k) => ({ ...k, count: itemsUsing[k.module]().filter((it) => it.kindId === k.id).length })),
  kinds_create: (a) => {
    const name = String(a.name).trim();
    if (!name) throw "类型名称不能为空";
    if (kinds.some((k) => k.module === a.module && k.name === name)) throw `「${name}」已经有了，换个名字吧`;
    return addKind(a.module as string, name);
  },
  kinds_rename: (a) => {
    const k = kinds.find((x) => x.id === a.id)!;
    const name = String(a.name).trim();
    if (kinds.some((x) => x.module === k.module && x.name === name && x.id !== k.id)) throw `「${name}」已经有了，换个名字吧`;
    k.name = name;
    return k;
  },
  kinds_delete: (a) => {
    kinds = kinds.filter((k) => k.id !== a.id);
    Object.values(itemsUsing).forEach((f) => f().forEach((it) => it.kindId === a.id && (it.kindId = null)));
    return null;
  },
  kinds_reorder: (a) => {
    (a.ids as number[]).forEach((id, i) => (kinds.find((k) => k.id === id)!.sortOrder = i + 1));
    return null;
  },

  ...collection("books", { author: null, kindId: kindId("books", "虚构") }, [
    { title: "示例书 A", author: "作者甲", country: "中国", status: "done", rating: 5, finishedMonth: "2025-11" },
    { title: "示例书 B", author: "作者乙", country: "日本", status: "doing", rating: 3, kindId: kindId("books", "非虚构") },
    { title: "示例书 C", author: "作者甲", country: "中国", status: "todo", kindId: kindId("books", "漫画") },
    { title: "示例书 D", author: "作者丙 / 作者丁", country: "法国", note: "一条备注", kindId: null },
  ]),
  ...collection("films", { director: null, kindId: kindId("films", "电影") }, [
    { title: "示例电影", director: "导演甲", country: "美国", status: "done", rating: 5, finishedMonth: "2026-01" },
    { title: "示例剧集", director: "导演乙", country: "韩国", status: "doing", kindId: kindId("films", "剧集") },
    { title: "示例动漫", director: "导演丙", country: "日本", kindId: kindId("films", "动漫") },
  ]),

  drawing_courses: () => [...courses].sort((a, b) => a.sortOrder - b.sortOrder).map(withCounts),
  drawing_course_create: (a) => {
    const p = a.patch as { name?: string; note?: string | null };
    if (!p.name?.trim()) throw "课程名不能为空";
    if (courses.some((c) => c.name === p.name!.trim())) throw `已经有叫「${p.name}」的课程了`;
    return withCounts(addCourse(p.name.trim(), p.note ?? null));
  },
  drawing_course_update: (a) => {
    const c = courses.find((x) => x.id === a.id)!;
    Object.assign(c, a.patch, { updatedAt: now() });
    return withCounts(c);
  },
  drawing_course_delete: (a) => {
    courses = courses.filter((c) => c.id !== a.id);
    lessons = lessons.filter((l) => l.courseId !== a.id);
    return null;
  },
  drawing_lessons: (a) => lessons.filter((l) => l.courseId === a.courseId).sort((x, y) => (x.sortOrder as number) - (y.sortOrder as number)),
  drawing_lesson_create: (a) => {
    const l = applyPatch(addLesson(a.courseId as number, { title: "x" }), a.patch as Args, "课名");
    lessons[lessons.length - 1] = l;
    return l;
  },
  drawing_lesson_update: (a) => {
    const i = lessons.findIndex((l) => l.id === a.id);
    lessons[i] = applyPatch(lessons[i], a.patch as Args, "课名");
    return lessons[i];
  },
  drawing_lesson_delete: (a) => {
    lessons = lessons.filter((l) => l.id !== a.id);
    return null;
  },

  packing_templates: () => tpls.map(tplOut),
  packing_template_create: (a) => {
    const p = a.patch as { name?: string; note?: string | null };
    if (!p.name?.trim()) throw "模板名不能为空";
    if (tpls.some((t) => t.name === p.name!.trim())) throw `已经有叫「${p.name}」的模板了`;
    return tplOut(addTpl(p.name.trim(), p.note ?? null));
  },
  packing_template_update: (a) => {
    const t = tpls.find((x) => x.id === a.id)!;
    Object.assign(t, a.patch, { updatedAt: now() });
    return tplOut(t);
  },
  packing_template_delete: (a) => {
    tpls = tpls.filter((t) => t.id !== a.id);
    tplItems = tplItems.filter((i) => i.parentId !== a.id);
    tripRows.forEach((t) => t.templateId === a.id && (t.templateId = null));
    return null;
  },
  packing_trips: () => [...tripRows].sort((x, y) => y.id - x.id).map(tripOut),
  packing_trip_create: (a) => tripOut(createTripRow(a.patch as Args, (a.templateId as number | null) ?? null)),
  packing_trip_update: (a) => {
    const t = tripRows.find((x) => x.id === a.id)!;
    Object.assign(t, a.patch, { updatedAt: now() });
    return tripOut(t);
  },
  packing_trip_delete: (a) => {
    tripRows = tripRows.filter((t) => t.id !== a.id);
    tripItems = tripItems.filter((i) => i.parentId !== a.id);
    return null;
  },
  ...pItemHandlers("template"),
  ...pItemHandlers("trip"),

  "plugin:dialog|open": () => "/mock/pomelo-import.json",
  import_preview: () => ({
    modules: [
      { key: "books", label: "摸鱼读读", incoming: 2, existing: handlers["books__count"]({}) },
      { key: "films", label: "好奇看看", incoming: 1, existing: handlers["films__count"]({}) },
      { key: "drawing", label: "随意画画", incoming: 1, existing: courses.length },
    ],
    skipped: [],
    backup: null,
  }),
  import_apply: () => {
    const existing = { books: handlers["books__count"]({}) as number, films: handlers["films__count"]({}) as number, drawing: courses.length };
    handlers["books__replace"]({ rows: [{ title: "导入书 1", status: "done", rating: 4 }, { title: "导入书 2" }] });
    handlers["films__replace"]({ rows: [{ title: "导入电影 1" }] });
    courses = [];
    lessons = [];
    addLesson(addCourse("导入课程").id, { title: "导入课节", status: "todo" });
    return {
      modules: [
        { key: "books", label: "摸鱼读读", incoming: 2, existing: existing.books },
        { key: "films", label: "好奇看看", incoming: 1, existing: existing.films },
        { key: "drawing", label: "随意画画", incoming: 1, existing: existing.drawing },
      ],
      skipped: [],
      backup: "/mock/backup.db",
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
