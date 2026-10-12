// DEV-ONLY in-memory backend so the UI can be exercised in a plain browser: `pnpm dev` → http://localhost:1420/?mock
// Mirrors the Rust command surface loosely (not the source of truth — Rust rules/tests are).
// Keep in sync when adding commands. Never imported in production builds (guarded in main.tsx).
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { nextOnOrAfter } from "@/modules/finance/money";
import { excerpt, tagSpans } from "@/modules/notes/NoteBody";
import { addDays, parseYmd, toYmd } from "@/shared/dates";

type Args = Record<string, unknown>;
type Row = Record<string, unknown> & { id: number; title: string; status: string };

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

/** 完成时间: finishedMonth (随意画画) or finishedYear (书 / 影), auto-filled on → done unless given. */
function applyPatch<T extends Row>(old: T, patch: Args, titleLabel = "名称", finished: "finishedMonth" | "finishedYear" = "finishedMonth"): T {
  const r = { ...old, ...patch, updatedAt: now() } as T;
  if (!r.title?.trim()) throw `${titleLabel}不能为空`;
  r.title = r.title.trim();
  if (r.status === "done" && old.status !== "done" && !r[finished] && !(finished in patch))
    (r as Row)[finished] = finished === "finishedYear" ? month().slice(0, 4) : month();
  return r;
}

/** In-memory `<prefix>_list/_create/_update/_delete` with the Rust patch semantics (incl. auto month). */
function collection(prefix: string, defaults: Record<string, unknown>, seed: Record<string, unknown>[]) {
  const make = (p: Record<string, unknown>): Row =>
    ({ id: nextId++, title: "", status: "unmarked", rating: null, finishedYear: null, note: null, rereads: [], country: null, kindId: null, ...defaults, createdAt: now(), updatedAt: now(), ...p }) as Row;
  let rows = seed.map(make);
  itemsUsing[prefix] = () => rows as never;
  return {
    [`${prefix}_list`]: () => [...rows].sort((a, b) => a.id - b.id),
    [`${prefix}_create`]: (a: Args) => {
      const r = applyPatch({ ...make({}), status: "unmarked" }, a.patch as Args, "名称", "finishedYear");
      rows.push(r);
      return r;
    },
    [`${prefix}_update`]: (a: Args) => {
      const i = rows.findIndex((r) => r.id === a.id);
      if (i < 0) throw "找不到这条记录，可能已经被删除了";
      rows[i] = applyPatch(rows[i], a.patch as Args, "名称", "finishedYear");
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

// ---- backups (file names mirror backup.rs; restore only records the safety copy) ----
type Backup = { fileName: string; store: "pomelo" | "picky"; label: string; createdAt: string; size: number };
const stamp = (d: Date) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
};
const mkBackup = (label: string, d = new Date(), store: Backup["store"] = "pomelo"): Backup => {
  const s = stamp(d);
  return {
    fileName: `${store}-${s}-${label}.db`,
    store,
    label,
    createdAt: `${s.slice(0, 4)}-${s.slice(4, 6)}-${s.slice(6, 8)} ${s.slice(9, 11)}:${s.slice(11, 13)}:${s.slice(13, 15)}`,
    size: 98304 + Math.floor(Math.random() * 4096),
  };
};
let backups: Backup[] = [
  ...Array.from({ length: 10 }, (_, i) => mkBackup("daily", new Date(Date.now() - (i + 1) * 86400000))),
  mkBackup("before-v7", new Date(Date.now() - 2 * 86400000 + 60000)),
  mkBackup("before-import", new Date(Date.now() - 3 * 86400000 + 120000)),
  mkBackup("manual", new Date(Date.now() - 5 * 86400000 + 180000)),
  ...Array.from({ length: 3 }, (_, i) => mkBackup("daily", new Date(Date.now() - (i + 1) * 86400000 + 1000), "picky")),
];
const addBackup = (label: string, d = new Date(), store: Backup["store"] = "pomelo") => {
  const b = mkBackup(label, d, store);
  backups = [b, ...backups];
  return b;
};

// ---- 一步一步 (todo): mirrors todo.rs rules loosely (placement, cascade, types, 今天 / 已过期) ----
type TGroup = { id: number; name: string; sortOrder: number; createdAt: string; updatedAt: string };
type TGoal = { id: number; groupId: number; name: string; status: string; startDate: string | null; endDate: string | null; note: string | null; review: string | null; sortOrder: number; createdAt: string; updatedAt: string };
type TTask = {
  id: number; groupId: number; goalId: number | null; parentId: number | null; title: string; note: string | null; link: string | null;
  planDate: string | null; startDate: string | null; dueDate: string | null; done: boolean; doneAt: string | null; collapsed: boolean;
  sortOrder: number; createdAt: string; updatedAt: string;
  taskType: "normal" | "repeat" | "checkin" | "progress"; repeatRule: string | null; repeatTotal: number | null; repeatUntil: string | null;
  target: number | null; unit: string | null; period: string | null; doneBefore: number;
};
type TLog = { id: number; taskId: number; day: string; amount: number; createdAt: string };
let tGroups: TGroup[] = [];
let tGoals: TGoal[] = [];
let tTasks: TTask[] = [];
let tLogs: TLog[] = [];
const ymd = toYmd;
const mockToday = () => ymd(new Date());
const dayOffset = (n: number) => addDays(mockToday(), n);
const isoWeekday = (d: string) => ((parseYmd(d).getDay() + 6) % 7) + 1;
const isLeaf = (t: TTask) => !tTasks.some((c) => c.parentId === t.id);
const logsOf = (id: number) => tLogs.filter((l) => l.taskId === id);
const tGroupOut = (g: TGroup) => ({ ...g, open: tTasks.filter((t) => t.groupId === g.id && !t.done && isLeaf(t)).length });
const tGoalOut = (g: TGoal) => {
  const ls = tTasks.filter((t) => t.goalId === g.id && isLeaf(t));
  return { ...g, total: ls.length, done: ls.filter((t) => t.done).length };
};
const ruleNext = (rule: string, anchor: string, completed: string): string => {
  const [kind, arg = ""] = rule.split(":");
  if (kind === "daily") return addDays(anchor, 1);
  if (kind === "weekly") {
    const ds = arg.split(",").map(Number);
    for (let i = 1; i <= 7; i++) if (ds.includes(isoWeekday(addDays(anchor, i)))) return addDays(anchor, i);
  }
  if (kind === "monthly") {
    const d = parseYmd(anchor);
    const ds = arg.split(",").map(Number).sort((x, y) => x - y);
    for (let k = 0; k < 24; k++) {
      const last = new Date(d.getFullYear(), d.getMonth() + k + 1, 0).getDate();
      for (const day of ds) {
        const c = ymd(new Date(d.getFullYear(), d.getMonth() + k, Math.min(day, last)));
        if (c > anchor) return c;
      }
    }
  }
  if (kind === "yearly") {
    const [md, every = "1"] = arg.split("/");
    const [m, day] = md.split("-").map(Number);
    const on = (y: number) => ymd(new Date(y, m - 1, Math.min(day, new Date(y, m, 0).getDate())));
    const y = Number(anchor.slice(0, 4));
    return on((on(y) > anchor ? y : y + 1) + Number(every) - 1);
  }
  const n = Number(arg.slice(0, -1));
  if (kind === "every") return addDays(anchor, arg.endsWith("w") ? 7 * n : n);
  const u = arg.slice(-1); // after
  if (u === "m" || u === "y") {
    const d = parseYmd(completed);
    const k = u === "y" ? 12 * n : n;
    return ymd(new Date(d.getFullYear(), d.getMonth() + k, Math.min(d.getDate(), new Date(d.getFullYear(), d.getMonth() + k + 1, 0).getDate())));
  }
  return addDays(completed, u === "w" ? 7 * n : n);
};
const counted = (rule: string | null) => !!rule && !rule.startsWith("after");
const firstFrom = (rule: string, day: string) =>
  /^(weekly|monthly)/.test(rule) ? ruleNext(rule, addDays(day, -1), day) : rule.startsWith("yearly:") ? ruleNext(rule.split("/")[0], addDays(day, -1), day) : day;
const scheduled = (rule: string | null, d: string) => rule === "daily" || (!!rule?.startsWith("weekly:") && rule.slice(7).split(",").map(Number).includes(isoWeekday(d)));
const tTaskOut = (t: TTask) => {
  const today = mockToday();
  const ls = logsOf(t.id);
  const total = ls.reduce((n, l) => n + l.amount, 0);
  let streak = 0;
  if (t.taskType === "checkin") {
    const days = new Set(ls.map((l) => l.day));
    for (let i = 0; i < 400; i++) {
      const d = addDays(today, -i);
      if (t.startDate && d < t.startDate) break;
      if (!scheduled(t.repeatRule, d)) continue;
      if (days.has(d)) streak++;
      else if (i > 0) break;
    }
  }
  const count = t.taskType === "progress" ? total : t.taskType === "normal" ? 0 : t.doneBefore + ls.length;
  let lastDate: string | null = null;
  if (t.taskType === "repeat" && counted(t.repeatRule) && t.repeatTotal && !t.done && t.dueDate && t.repeatTotal - count - 1 >= 0) {
    lastDate = t.dueDate;
    for (let i = 0; i < t.repeatTotal - count - 1; i++) lastDate = ruleNext(t.repeatRule!, lastDate, lastDate);
  }
  return { ...t, count, lastDate, totalAmount: total, loggedToday: ls.some((l) => l.day === today), streak };
};
const subtree = (id: number): TTask[] => tTasks.filter((t) => t.parentId === id).flatMap((t) => [t, ...subtree(t.id)]);
const reopenUp = (t: TTask) => {
  for (let p = tTasks.find((x) => x.id === t.parentId); p; p = tTasks.find((x) => x.id === p!.parentId)) Object.assign(p, { done: false, doneAt: null });
};
const markDone = (t: TTask) => {
  for (const x of [t, ...subtree(t.id)]) if (!x.done) Object.assign(x, { done: true, doneAt: now() });
  for (let c = t; c.parentId; ) {
    const p = tTasks.find((x) => x.id === c.parentId)!;
    if (tTasks.some((x) => x.parentId === p.id && !x.done)) break;
    Object.assign(p, { done: true, doneAt: p.doneAt ?? now() });
    c = p;
  }
};
/** Check-in: every scheduled day from the start (or the day it was added) to the end is checked. */
const allChecked = (t: TTask) => {
  const days: string[] = [];
  for (let d = t.startDate ?? toYmd(new Date(t.createdAt)); d <= t.dueDate! && days.length < 3700; d = addDays(d, 1)) if (scheduled(t.repeatRule, d)) days.push(d);
  return days.length > 0 && days.every((d) => logsOf(t.id).some((l) => l.day === d));
};
const settle = (t: TTask) => {
  const o = tTaskOut(t);
  const reached =
    t.taskType === "repeat"
      ? t.repeatTotal
        ? o.count >= t.repeatTotal
        : null
      : t.taskType === "checkin"
        ? t.dueDate
          ? t.dueDate < mockToday() || allChecked(t)
          : null
        : t.taskType === "progress"
          ? o.totalAmount >= (t.target ?? Infinity)
          : null;
  if (reached === true && !t.done) markDone(t);
  if (reached === false && t.done) Object.assign(t, { done: false, doneAt: null });
};
const advance = (t: TTask, completed: string) => {
  const anchor = t.dueDate!;
  const next = ruleNext(t.repeatRule!, anchor > completed ? anchor : completed, completed);
  if ((t.repeatTotal && t.doneBefore + logsOf(t.id).length >= t.repeatTotal) || (t.repeatUntil && next > t.repeatUntil)) return markDone(t);
  t.dueDate = next;
};
/** Mirrors TaskFields::validated: clears what doesn't belong to the type (mutates, may throw). */
const normalize = (t: TTask) => {
  if (!t.title.trim()) throw "标题不能为空";
  if (t.taskType !== "repeat") t.doneBefore = 0;
  if (t.taskType === "normal" || t.taskType === "progress") {
    if (t.planDate) {
      if (!t.startDate && (!t.dueDate || t.planDate <= t.dueDate)) t.startDate = t.planDate;
      t.dueDate ??= t.planDate;
      t.planDate = null;
    }
    Object.assign(t, { repeatRule: null, repeatTotal: null, repeatUntil: null });
    if (t.taskType === "normal") Object.assign(t, { target: null, unit: null, period: null });
    else if (!t.target) throw "请设置目标数量（1～100000）";
    else t.period = "all";
  } else if (t.taskType === "repeat") {
    if (!t.repeatRule) throw "请设置重复规则";
    if (counted(t.repeatRule)) t.planDate = null;
    else t.repeatTotal = null;
    Object.assign(t, { startDate: null, repeatUntil: null, target: null, unit: null, period: null });
  } else {
    Object.assign(t, { planDate: null, repeatTotal: null, repeatUntil: null, target: null, unit: null, period: null });
  }
  if (t.startDate && !t.dueDate) throw "时间段要有结束日期";
};
const firstDue = (t: TTask) => {
  const today = mockToday();
  const d = firstFrom(t.repeatRule!, t.planDate && t.planDate > today ? t.planDate : today);
  if (t.repeatUntil && d > t.repeatUntil) throw "时间段里没有要做的日子";
  return d;
};
const setDoneCount = (t: TTask, n: number | undefined) => {
  const logged = logsOf(t.id).length;
  if (n !== undefined) {
    if (n < logged) throw `已经记了 ${logged} 次，不能少于这个数`;
    t.doneBefore = n - logged;
  }
  if (t.repeatTotal && t.doneBefore + logged > t.repeatTotal) throw "已经做了的次数不能多于一共几次";
};
const addLog = (taskId: number, day: string, amount = 1) => tLogs.push({ id: nextId++, taskId, day, amount, createdAt: now() });
const addTGroup = (name: string) => {
  const g = { id: nextId++, name, sortOrder: tGroups.length + 1, createdAt: now(), updatedAt: now() };
  tGroups.push(g);
  return g;
};
const addTGoal = (groupId: number, name: string, p: Partial<TGoal> = {}) => {
  const g = { id: nextId++, groupId, name, status: "active", startDate: null, endDate: null, note: null, review: null, sortOrder: tGoals.filter((x) => x.groupId === groupId).length + 1, createdAt: now(), updatedAt: now(), ...p };
  tGoals.push(g);
  return g;
};
const addTTask = (p: Partial<TTask> & { title: string; doneCount?: number }) => {
  let place = { groupId: p.groupId ?? 0, goalId: p.goalId ?? null, parentId: p.parentId ?? null };
  if (p.parentId) {
    const parent = tTasks.find((t) => t.id === p.parentId);
    if (!parent) throw "找不到上一层任务，可能已经被删除了";
    if (parent.taskType !== "normal") throw "重复、打卡、进度条任务下面不能加子任务";
    place = { groupId: parent.groupId, goalId: parent.goalId, parentId: parent.id };
  } else if (p.goalId) {
    place = { groupId: tGoals.find((g) => g.id === p.goalId)!.groupId, goalId: p.goalId, parentId: null };
  } else if (!tGroups.some((g) => g.id === place.groupId)) throw "请选择分组";
  const sibs = tTasks.filter((t) => t.groupId === place.groupId && t.goalId === place.goalId && t.parentId === place.parentId);
  const { doneCount, ...fields } = p;
  const t: TTask = {
    note: null, link: null, planDate: null, startDate: null, dueDate: null, done: false, doneAt: null, collapsed: false, createdAt: now(), updatedAt: now(),
    taskType: "normal", repeatRule: null, repeatTotal: null, repeatUntil: null, target: null, unit: null, period: null, doneBefore: 0,
    ...fields, ...place, id: nextId++, title: p.title.trim(), sortOrder: sibs.length + 1,
  };
  normalize(t);
  if (t.taskType === "repeat") {
    t.doneBefore = doneCount ?? 0;
    setDoneCount(t, undefined);
    t.dueDate = firstDue(t);
  }
  tTasks.push(t);
  reopenUp(t);
  settle(t);
  return t;
};
const placedOut = (t: TTask) => ({
  ...tTaskOut(t),
  groupName: tGroups.find((g) => g.id === t.groupId)?.name ?? "",
  goalName: tGoals.find((g) => g.id === t.goalId)?.name ?? null,
  parentTitle: tTasks.find((x) => x.id === t.parentId)?.title ?? null,
});
const placeKey = (t: TTask) => {
  const g = tGroups.find((x) => x.id === t.groupId)!;
  const o = tGoals.find((x) => x.id === t.goalId);
  return [g.sortOrder, o ? o.sortOrder : -1, t.sortOrder, t.id];
};
const byPlace = (a: TTask, b: TTask) => {
  const [x, y] = [placeKey(a), placeKey(b)];
  for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return x[i] - y[i];
  return 0;
};
/** Mirrors common::reorder: `ids` (part of one list) swap among the slots they hold; others stay put. */
const reorderPart = <T extends { id: number; sortOrder: number }>(scope: T[], ids: number[]) => {
  const order = [...scope].sort((a, b) => a.sortOrder - b.sortOrder || a.id - b.id);
  if (new Set(ids).size !== ids.length || !ids.every((id) => order.some((r) => r.id === id))) throw "列表已经变了，请刷新后再拖";
  const queue = [...ids];
  const next = order.map((r) => (ids.includes(r.id) ? queue.shift()! : r.id));
  next.forEach((id, i) => (order.find((r) => r.id === id)!.sortOrder = i + 1));
};
const doneItems = (from: string, to: string) => {
  const out = tTasks
    .filter((t) => t.done && t.doneAt && ymd(new Date(t.doneAt)) >= from && ymd(new Date(t.doneAt)) <= to)
    .map((t) => ({ ...placedOut(t), day: ymd(new Date(t.doneAt!)), at: t.doneAt!, detail: t.taskType === "normal" ? null : "全部完成" }));
  for (const l of tLogs.filter((x) => x.day >= from && x.day <= to)) {
    const t = tTasks.find((x) => x.id === l.taskId);
    if (!t) continue;
    const nth = t.doneBefore + logsOf(t.id).filter((x) => x.day < l.day || (x.day === l.day && x.id <= l.id)).length;
    const detail = t.taskType === "repeat" ? `第 ${nth} 次` : t.taskType === "checkin" ? "打卡" : `+${l.amount}${t.unit ? ` ${t.unit}` : ""}`;
    out.push({ ...placedOut(t), day: l.day, at: l.createdAt, detail });
  }
  return out.sort((a, b) => (a.day + a.at).localeCompare(b.day + b.at));
};
{
  const study = addTGroup("🖥️ 学业");
  const proj = addTGroup("📌 个人项目");
  const later = addTGroup("📈 长期");
  addTTask({ groupId: study.id, title: "交 **课程报告** 第一版", done: true, doneAt: new Date(Date.now() - 2 * 86400000).toISOString() });
  addTTask({ groupId: study.id, title: "**周五** 交实验记录", dueDate: dayOffset(-2) });
  addTTask({ groupId: study.id, title: "和导师约进度会", dueDate: dayOffset(0), link: "https://example.com" });
  addTTask({ groupId: study.id, title: "报告提交窗口", startDate: dayOffset(-1), dueDate: dayOffset(2) });
  const weekly = addTTask({ groupId: study.id, title: "交 Weekly Progress Report", taskType: "repeat", repeatRule: "weekly:6", repeatTotal: 20, doneCount: 2 });
  for (let i = 1; i <= 6; i++) addLog(weekly.id, dayOffset(-7 * i));
  const before = addTTask({ groupId: study.id, title: "**答辩前**提交：" });
  ["最终报告", "个人总结", "展示 PPT"].forEach((t, i) => addTTask({ parentId: before.id, title: t, done: i === 0, doneAt: i === 0 ? now() : null }));
  const site = addTGoal(proj.id, "个人网站上线", { startDate: dayOffset(-10), endDate: dayOffset(40), note: "先做简单版" });
  ["选框架", "写首页", "买域名"].forEach((t, i) => addTTask({ goalId: site.id, title: t, done: i === 0, doneAt: i === 0 ? now() : null, dueDate: i === 1 ? dayOffset(0) : null }));
  const jp = addTGoal(proj.id, "学日语");
  const words = addTTask({ goalId: jp.id, title: "背单词", taskType: "checkin", repeatRule: "daily", startDate: dayOffset(-10), dueDate: dayOffset(20) });
  [1, 2, 3, 5, 6].forEach((i) => addLog(words.id, dayOffset(-i)));
  const read = addTTask({ groupId: proj.id, title: "今年读 20 本书", taskType: "progress", target: 20, unit: "本", startDate: `${mockToday().slice(0, 4)}-01-01`, dueDate: `${mockToday().slice(0, 4)}-12-31` });
  addLog(read.id, dayOffset(-20), 5);
  addLog(read.id, dayOffset(-3), 2);
  const apply = addTTask({ groupId: proj.id, title: "投简历", taskType: "progress", target: 5, unit: "份", startDate: dayOffset(-2), dueDate: dayOffset(4) });
  addLog(apply.id, dayOffset(0), 1);
  addTTask({ groupId: proj.id, title: "整理照片", dueDate: dayOffset(-3), note: "挑 20 张" });
  addTTask({ groupId: proj.id, title: "交房租", taskType: "repeat", repeatRule: "monthly:1,15", planDate: dayOffset(0) });
  addTTask({ groupId: later.id, title: "办护照" });
}
// Undo / redo mock: whole-state snapshots of the todo tables around each edit.
type Snap = { label: string; before: string; after: string };
const snapState = () => JSON.stringify({ tGroups, tGoals, tTasks, tLogs, fin, jb, nt, vItems: vt.items });
const loadState = (s: string) => {
  let vItems: VRow[];
  ({ tGroups, tGoals, tTasks, tLogs, fin, jb, nt, vItems } = JSON.parse(s));
  vt.items = vItems;
};
let undoStack: Snap[] = [];
let redoStack: Snap[] = [];
const undoable = (label: string, f: (a: Args) => unknown) => (a: Args) => {
  const before = snapState();
  const out = f(a);
  const after = snapState();
  if (after !== before) {
    undoStack.push({ label, before, after });
    redoStack = [];
  }
  return out;
};
const todoHandlers: Record<string, (a: Args) => unknown> = {
  todo_groups: () => [...tGroups].sort((a, b) => a.sortOrder - b.sortOrder).map(tGroupOut),
  todo_group_create: undoable("添加分组", (a) => {
    const name = String(a.name).trim();
    if (!name) throw "分组名不能为空";
    if (tGroups.some((g) => g.name === name)) throw `已经有叫「${name}」的分组了`;
    return tGroupOut(addTGroup(name));
  }),
  todo_group_rename: undoable("重命名分组", (a) => {
    const g = tGroups.find((x) => x.id === a.id)!;
    g.name = String(a.name).trim();
    return tGroupOut(g);
  }),
  todo_group_delete: undoable("删除分组", (a) => {
    tGroups = tGroups.filter((g) => g.id !== a.id);
    tGoals = tGoals.filter((g) => g.groupId !== a.id);
    tTasks = tTasks.filter((t) => t.groupId !== a.id);
    return null;
  }),
  todo_goals: () => [...tGoals].sort((a, b) => a.sortOrder - b.sortOrder).map(tGoalOut),
  todo_goal_create: undoable("添加目标", (a) => {
    const p = a.patch as Partial<TGoal>;
    if (!p.groupId) throw "请选择分组";
    if (!p.name?.trim()) throw "目标名不能为空";
    return tGoalOut(addTGoal(p.groupId, p.name.trim(), p));
  }),
  todo_goal_update: undoable("修改目标", (a) => {
    const g = tGoals.find((x) => x.id === a.id)!;
    const p = a.patch as Partial<TGoal>;
    if (p.groupId && p.groupId !== g.groupId) tTasks.filter((t) => t.goalId === g.id).forEach((t) => (t.groupId = p.groupId!));
    Object.assign(g, p, { updatedAt: now() });
    return tGoalOut(g);
  }),
  todo_goal_delete: undoable("删除目标", (a) => {
    tGoals = tGoals.filter((g) => g.id !== a.id);
    tTasks = tTasks.filter((t) => t.goalId !== a.id);
    return null;
  }),
  todo_tasks: (a) =>
    tTasks
      .filter((t) => (a.goalId ? t.goalId === a.goalId : t.groupId === a.groupId && t.goalId === null))
      .sort((x, y) => x.sortOrder - y.sortOrder || x.id - y.id)
      .map(tTaskOut),
  todo_task_create: undoable("添加任务", (a) => tTaskOut(addTTask(a.patch as TTask & { doneCount?: number }))),
  todo_task_update: undoable("修改任务", (a) => {
    const t = tTasks.find((x) => x.id === a.id);
    if (!t) throw "找不到这个任务，可能已经被删除了";
    const { groupId, goalId, doneCount, ...p } = a.patch as Partial<TTask> & { doneCount?: number };
    if ((goalId !== undefined && goalId !== t.goalId) || (groupId !== undefined && groupId !== t.groupId)) {
      if (t.parentId) throw "子任务跟着上一层任务走，不能单独移动";
      const g = goalId ? tGoals.find((x) => x.id === goalId)!.groupId : (groupId ?? t.groupId);
      for (const x of [t, ...subtree(t.id)]) Object.assign(x, { groupId: g, goalId: goalId ?? null });
    }
    if (p.taskType && p.taskType !== "normal" && tTasks.some((x) => x.parentId === t.id)) throw "有子任务的任务只能是普通任务";
    const next: TTask = { ...t, ...p, updatedAt: now() };
    normalize(next);
    if (next.taskType === "repeat") {
      setDoneCount(next, doneCount);
      if (t.taskType !== "repeat" || next.repeatRule !== t.repeatRule || next.planDate !== t.planDate || !next.dueDate) next.dueDate = firstDue(next);
    }
    Object.assign(t, next);
    settle(t);
    return tTaskOut(t);
  }),
  todo_task_set_done: undoable("勾选任务", (a) => {
    const t = tTasks.find((x) => x.id === a.id)!;
    const today = mockToday();
    if (t.taskType === "normal") {
      if (a.done) markDone(t);
      else Object.assign(t, { done: false, doneAt: null }), reopenUp(t);
    } else if (t.taskType === "repeat") {
      if (a.done && !t.done) addLog(t.id, today), advance(t, today);
      else if (!a.done && t.done) Object.assign(t, { done: false, doneAt: null });
    } else if (t.taskType === "checkin") {
      if (a.done && !logsOf(t.id).some((l) => l.day === today)) addLog(t.id, today);
      if (!a.done) tLogs = tLogs.filter((l) => !(l.taskId === t.id && l.day === today));
      settle(t);
    } else if (a.done && !t.done) {
      addLog(t.id, today);
      settle(t);
    } else if (!a.done && t.done) Object.assign(t, { done: false, doneAt: null });
    return null;
  }),
  todo_task_skip: undoable("跳过一次", (a) => {
    advance(tTasks.find((x) => x.id === a.id)!, mockToday());
    return null;
  }),
  todo_logs: (a) => logsOf(a.taskId as number).sort((x, y) => y.day.localeCompare(x.day) || y.id - x.id),
  todo_log: undoable("记录", (a) => {
    const t = tTasks.find((x) => x.id === a.taskId)!;
    const day = (a.day as string) ?? mockToday();
    if (day > mockToday()) throw "不能记未来的日子";
    if (t.taskType === "checkin") {
      const had = logsOf(t.id).some((l) => l.day === day);
      if (had) tLogs = tLogs.filter((l) => !(l.taskId === t.id && l.day === day));
      else addLog(t.id, day);
    } else addLog(t.id, day, (a.amount as number) ?? 1);
    settle(t);
    return null;
  }),
  todo_log_delete: undoable("删除记录", (a) => {
    const l = tLogs.find((x) => x.id === a.id)!;
    tLogs = tLogs.filter((x) => x.id !== a.id);
    settle(tTasks.find((t) => t.id === l.taskId)!);
    return null;
  }),
  todo_task_delete: undoable("删除任务", (a) => {
    const gone = new Set([a.id as number, ...subtree(a.id as number).map((t) => t.id)]);
    tTasks = tTasks.filter((t) => !gone.has(t.id));
    tLogs = tLogs.filter((l) => !gone.has(l.taskId));
    return null;
  }),
  todo_groups_reorder: undoable("调整顺序", (a) => reorderPart(tGroups, a.ids as number[])),
  todo_goals_reorder: undoable("调整顺序", (a) => reorderPart(tGoals.filter((g) => g.groupId === a.groupId), a.ids as number[])),
  todo_tasks_reorder: undoable("调整顺序", (a) => {
    const first = tTasks.find((t) => t.id === (a.ids as number[])[0])!;
    reorderPart(tTasks.filter((t) => t.groupId === first.groupId && t.goalId === first.goalId && t.parentId === first.parentId), a.ids as number[]);
  }),
  todo_agenda: (a) => {
    const today = String(a.today);
    const active = (t: TTask) => !t.goalId || tGoals.find((g) => g.id === t.goalId)?.status === "active";
    const hit = (t: TTask) => (t.dueDate !== null && t.dueDate <= today) || (t.startDate !== null && t.startDate <= today && today <= t.dueDate!);
    const late = (t: TTask) => t.dueDate !== null && t.dueDate < today;
    const overdue: ReturnType<typeof placedOut>[] = [];
    const due: ReturnType<typeof placedOut>[] = [];
    const doneToday: ReturnType<typeof placedOut>[] = [];
    for (const t of tTasks.filter((x) => !x.done && active(x)).sort(byPlace)) {
      const o = placedOut(t);
      const started = (!t.startDate || t.startDate <= today) && (!t.dueDate || today <= t.dueDate);
      if (t.taskType === "checkin") {
        if (o.loggedToday) doneToday.push(o);
        else if (started && scheduled(t.repeatRule, today)) due.push(o);
      } else {
        if (o.loggedToday) doneToday.push(o);
        if (hit(t)) (late(t) ? overdue : due).push(o);
      }
    }
    const key = (t: TTask) => t.dueDate ?? "";
    overdue.sort((x, y) => key(x).localeCompare(key(y)));
    for (const t of tTasks.filter((t) => t.done && t.doneAt && ymd(new Date(t.doneAt)) === today)) if (!doneToday.some((d) => d.id === t.id)) doneToday.push(placedOut(t));
    return { today, overdue, due, doneToday };
  },
  todo_done_between: (a) => doneItems(String(a.from), String(a.to)),
  todo_month: (a) => {
    const [from, to] = [String(a.from), String(a.to)];
    const out: object[] = [];
    const inside = (d: string | null) => !!d && d >= from && d <= to;
    for (const t of tTasks.filter((x) => !x.done && x.taskType !== "checkin")) {
      const o = placedOut(t);
      if (t.startDate && t.dueDate) for (let d = t.startDate; d <= t.dueDate; d = addDays(d, 1)) inside(d) && out.push({ ...o, day: d, what: "range", detail: null });
      else if (inside(t.dueDate)) out.push({ ...o, day: t.dueDate, what: "due", detail: null });
      if (t.taskType === "repeat") {
        let anchor = t.dueDate!;
        for (let k = 1; k <= 62; k++) {
          const next = ruleNext(t.repeatRule!, anchor, anchor);
          const n = o.count + k + 1;
          if (next > to || (t.repeatTotal && n > t.repeatTotal) || (t.repeatUntil && next > t.repeatUntil)) break;
          if (inside(next)) out.push({ ...o, day: next, what: "next", detail: `第 ${n} 次` });
          anchor = next;
        }
      }
    }
    for (const d of doneItems(from, to)) out.push({ ...d, what: "done" });
    return out;
  },
  undo_last: () => {
    const s = undoStack.pop();
    if (!s) return { label: null };
    loadState(s.before);
    redoStack.push(s);
    return { label: s.label };
  },
  redo_last: () => {
    const s = redoStack.pop();
    if (!s) return { label: null };
    loadState(s.after);
    undoStack.push(s);
    return { label: s.label };
  },
  open_link: (a) => {
    const link = String(a.link).trim();
    if (link.startsWith("/") || link.startsWith("~/")) throw `找不到这个文件：${link}`;
    window.open(/^[a-z][a-z0-9+-]+:(?!\d)/i.test(link) ? link : `https://${link}`, "_blank");
  },
};

// ---- 钱来钱往 (finance): mirrors finance.rs loosely (dated snapshots, periods, posting 固定支出, totals) ----
type Cur = "CNY" | "SGD" | "HKD" | "USD";
type FAccount = { id: number; name: string; kind: string; currency: Cur; closed: boolean; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
type FEntry = { id: number; flow: "out" | "in"; amount: number; currency: Cur; day: string; endDay: string | null; kindId: number | null; fixedId: number | null; note: string | null; createdAt: string; updatedAt: string };
type FFixed = { id: number; name: string; flow: "out" | "in"; amount: number; currency: Cur; kindId: number | null; rule: string; nextDay: string; stopped: boolean; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
type FHolding = { id: number; accountId: number; day: string; name: string; quantity: number; price: number };
let fin = {
  accounts: [] as FAccount[],
  balances: [] as { accountId: number; day: string; amount: number }[],
  rates: [] as { day: string; currency: Cur; rate: number }[],
  holdings: [] as FHolding[],
  fixed: [] as FFixed[],
  entries: [] as FEntry[],
  budgets: [] as { kindId: number; currency: Cur; amount: number }[],
};
(
  [
    ["fin_out", ["餐饮", "交通", "购物", "住房", "通讯", "订阅", "学习", "娱乐", "医疗", "旅行", "其他"]],
    ["fin_in", ["津贴", "奖学金", "家里", "投资收益", "其他"]],
  ] as const
).forEach(([m, names]) => names.forEach((n) => addKind(m, n)));
itemsUsing.fin_out = () => [...fin.entries, ...fin.fixed] as never;
itemsUsing.fin_in = () => [...fin.entries, ...fin.fixed] as never;
const finAccount = (id: number) => {
  const a = fin.accounts.find((x) => x.id === id);
  if (!a) throw "找不到这个账户，可能已经被删除了";
  return a;
};
const byDay = <T extends { day: string }>(rows: T[]) => [...rows].sort((a, b) => a.day.localeCompare(b.day));
const accountOut = (a: FAccount) => {
  const last = byDay(fin.balances.filter((b) => b.accountId === a.id)).pop();
  return { ...a, lastDay: last?.day ?? null, lastAmount: last?.amount ?? null };
};
const checkKind = (flow: string, kindId: number | null) => {
  if (kindId !== null && kinds.find((k) => k.id === kindId)?.module !== (flow === "in" ? "fin_in" : "fin_out")) throw "选择的分类不存在，或者不是这一类（支出 / 收入）的";
};
const checkEntry = (e: FEntry) => {
  if (!(e.amount > 0)) throw "金额要大于 0";
  if (!/^\d{4}-\d{2}-\d{2}$/.test(e.day)) throw "日期不对";
  if (e.endDay === e.day) e.endDay = null;
  if (e.endDay !== null && e.endDay < e.day) throw "结束日期不能早于开始日期";
  checkKind(e.flow, e.kindId);
};
const newEntry = (p: Partial<FEntry>): FEntry => ({
  id: nextId++, flow: "out", amount: 0, currency: "CNY", day: mockToday(), endDay: null, kindId: null, fixedId: null, note: null, createdAt: now(), updatedAt: now(), ...p,
});
const postDue = (today: string) => {
  let n = 0;
  for (const f of fin.fixed.filter((x) => !x.stopped && x.nextDay <= today)) {
    for (let k = 0; k < 240 && f.nextDay <= today; k++) {
      fin.entries.push(newEntry({ flow: f.flow, amount: f.amount, currency: f.currency, day: f.nextDay, kindId: f.kindId, fixedId: f.id, note: f.name }));
      f.nextDay = nextOnOrAfter(f.rule, addDays(f.nextDay, 1));
      n++;
    }
  }
  return n;
};
/** Total on each day with a balance: latest balance per account × latest rate, up to that day. */
const assetsOut = () => {
  const balances = byDay(fin.balances).sort((a, b) => a.day.localeCompare(b.day) || a.accountId - b.accountId);
  const rates = byDay(fin.rates);
  const days = [...new Set(balances.map((b) => b.day))];
  const accounts = [...fin.accounts].sort((a, b) => a.sortOrder - b.sortOrder);
  const totals = days.map((day) => {
    let sum = 0;
    const missing: Cur[] = [];
    for (const a of accounts) {
      const mine = balances.filter((b) => b.accountId === a.id);
      const upto = mine.filter((b) => b.day <= day).pop();
      if (!upto || (a.closed && mine[mine.length - 1].day < day)) continue;
      const r = a.currency === "CNY" ? 1 : rates.filter((x) => x.currency === a.currency && x.day <= day).pop()?.rate;
      if (r === undefined) {
        if (!missing.includes(a.currency)) missing.push(a.currency);
      } else sum += Math.round(upto.amount * r);
    }
    return { day, totalCny: missing.length ? null : sum, missing };
  });
  return { balances, rates, totals };
};
const holdingOut = (h: FHolding) => ({ ...h, value: Math.round(h.quantity * h.price * 100) });
const dayNo = (d: string) => Date.UTC(Number(d.slice(0, 4)), Number(d.slice(5, 7)) - 1, Number(d.slice(8, 10))) / 864e5;
/** finance.rs spread: a period's amount by month, in proportion to its days there. */
const spread = (amount: number, day: string, end: string) => {
  const days = dayNo(end) - dayNo(day) + 1;
  const out: [string, number][] = [];
  let [cur, covered, given] = [day, 0, 0];
  while (cur <= end) {
    const monthEnd = `${cur.slice(0, 7)}-${String(new Date(Number(cur.slice(0, 4)), Number(cur.slice(5, 7)), 0).getDate()).padStart(2, "0")}`;
    const to = monthEnd < end ? monthEnd : end;
    covered += dayNo(to) - dayNo(cur) + 1;
    const upto = Math.floor((2 * amount * covered + days) / (2 * days));
    out.push([cur.slice(0, 7), upto - given]);
    given = upto;
    cur = addDays(to, 1);
  }
  return out;
};
{
  const fk = (n: string) => kindId("fin_out", n);
  const ik = (n: string) => kindId("fin_in", n);
  const mk = (name: string, kind: string, currency: Cur) => {
    const a: FAccount = { id: nextId++, name, kind, currency, closed: false, note: null, sortOrder: fin.accounts.length + 1, createdAt: now(), updatedAt: now() };
    fin.accounts.push(a);
    return a;
  };
  const cmb = mk("招行", "bank", "CNY");
  const dbs = mk("DBS", "bank", "SGD");
  const ali = mk("支付宝", "wallet", "CNY");
  const ib = mk("嘉信", "broker", "USD");
  const today = mockToday();
  // irregular update days: a week apart, a few days in a row, a couple of months apart
  [-170, -120, -113, -70, -69, -68, -40, -12, -5].forEach((back, i) => {
    const day = addDays(today, back);
    fin.balances.push(
      { accountId: cmb.id, day, amount: 2_850_000 + i * 120_000 },
      { accountId: dbs.id, day, amount: 1_240_000 - i * 45_000 + (i % 2) * 90_000 },
      { accountId: ali.id, day, amount: 180_000 + i * 15_000 },
      { accountId: ib.id, day, amount: 420_000 + i * 18_000 },
    );
    fin.rates.push({ day, currency: "SGD", rate: 5.31 + i * 0.02 }, { day, currency: "USD", rate: 7.18 - i * 0.01 });
  });
  fin.balances.push({ accountId: dbs.id, day: addDays(today, -90), amount: 1_100_000 }); // only DBS that day
  [-170, -113, -68, -40, -5].forEach((back, i) => {
    const day = addDays(today, back);
    const h = (name: string, quantity: number, price: number) => fin.holdings.push({ id: nextId++, accountId: ib.id, day, name, quantity, price });
    h("VOO", i < 3 ? 4 : 5, 480 + i * 12.5);
    h("AAPL", 6, 210 + (i % 2 ? -8 : 6) + i * 3);
    if (i >= 2) h("QQQ", 2, 440 + i * 9);
    h("现金", 1, 1000 - i * 120);
  });
  const fx = (name: string, amount: number, currency: Cur, kind: number, rule: string) =>
    fin.fixed.push({ id: nextId++, name, flow: "out", amount, currency, kindId: kind, rule, nextDay: nextOnOrAfter(rule, addDays(today, -95)), stopped: false, note: null, sortOrder: fin.fixed.length + 1, createdAt: now(), updatedAt: now() });
  fx("房租", 150_000, "SGD", fk("住房"), "monthly:1");
  fx("话费", 3_000, "CNY", fk("通讯"), "monthly:20");
  fx("iCloud", 299, "USD", fk("订阅"), "monthly:5");
  postDue(today);
  // 小额 as weekly periods (Monday … Sunday) for the last ~3 months
  const mon = addDays(today, -((parseYmd(today).getDay() + 6) % 7) - 91);
  for (let w = 0; w < 13; w++) {
    const day = addDays(mon, w * 7);
    const endDay = addDays(day, 6);
    if (endDay > today) break;
    fin.entries.push(
      newEntry({ day, endDay, amount: 9_000 + (w % 4) * 2_300, currency: "SGD", kindId: fk("餐饮") }),
      newEntry({ day, endDay, amount: 3_200 + (w % 3) * 700, currency: "SGD", kindId: fk("交通") }),
    );
  }
  const thisM = today.slice(0, 7);
  const ym = (k: number) => toYmd(new Date(Number(thisM.slice(0, 4)), Number(thisM.slice(5)) - 1 + k, 1)).slice(0, 7);
  for (let k = -3; k <= 0; k++) {
    const day = (d: number) => `${ym(k)}-${String(d).padStart(2, "0")}`;
    const add = (d: number, p: Partial<FEntry>) => day(d) <= today && fin.entries.push(newEntry({ day: day(d), ...p }));
    add(1, { flow: "in", amount: 200_000, currency: "SGD", kindId: ik("津贴"), note: "实习津贴" });
    add(12, { amount: 38_900 - (k + 3) * 5_000, currency: "SGD", kindId: fk("购物"), note: "显示器支架" });
    add(16, { amount: 45_000, currency: "CNY", kindId: fk("餐饮"), note: "请客" });
    if (k === -2) add(3, { flow: "in", amount: 300_000, currency: "CNY", kindId: ik("家里") });
    if (k === -1) add(9, { amount: 128_000, currency: "SGD", kindId: fk("旅行"), note: "机票" });
  }
  fin.budgets.push({ kindId: fk("餐饮"), currency: "SGD", amount: 60_000 }, { kindId: fk("交通"), currency: "SGD", amount: 15_000 }, { kindId: fk("购物"), currency: "SGD", amount: 30_000 });
}
const finEntry = (id: unknown) => {
  const e = fin.entries.find((x) => x.id === id);
  if (!e) throw "找不到这个记录，可能已经被删除了";
  return e;
};
const finHandlers: Record<string, (a: Args) => unknown> = {
  fin_accounts: () => [...fin.accounts].sort((a, b) => a.sortOrder - b.sortOrder).map(accountOut),
  fin_account_create: undoable("添加账户", (a) => {
    const p = a.patch as Partial<FAccount>;
    const name = (p.name ?? "").trim();
    if (!name) throw "账户名不能为空";
    if (fin.accounts.some((x) => x.name === name)) throw `已经有叫「${name}」的账户了`;
    if (!p.currency) throw "请选择币种";
    const acc: FAccount = { id: nextId++, name, kind: p.kind ?? "bank", currency: p.currency, closed: p.closed ?? false, note: p.note ?? null, sortOrder: fin.accounts.length + 1, createdAt: now(), updatedAt: now() };
    fin.accounts.push(acc);
    return accountOut(acc);
  }),
  fin_account_update: undoable("修改账户", (a) => {
    const acc = finAccount(a.id as number);
    const p = a.patch as Partial<FAccount>;
    const used = fin.balances.some((b) => b.accountId === acc.id) || fin.holdings.some((h) => h.accountId === acc.id);
    if (p.currency && p.currency !== acc.currency && used) throw "这个账户已经有记录了，不能改币种；可以新建一个账户";
    Object.assign(acc, p, { name: (p.name ?? acc.name).trim(), updatedAt: now() });
    return accountOut(acc);
  }),
  fin_account_delete: undoable("删除账户", (a) => {
    fin.accounts = fin.accounts.filter((x) => x.id !== a.id);
    fin.balances = fin.balances.filter((b) => b.accountId !== a.id);
    fin.holdings = fin.holdings.filter((h) => h.accountId !== a.id);
    return null;
  }),
  fin_accounts_reorder: undoable("调整顺序", (a) => reorderPart(fin.accounts, a.ids as number[])),
  fin_assets: () => assetsOut(),
  fin_balances_save: undoable("更新余额", (a) => {
    const day = String(a.day);
    if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) throw "日期不对";
    for (const b of a.balances as { accountId: number; amount: number | null }[]) {
      fin.balances = fin.balances.filter((x) => !(x.accountId === b.accountId && x.day === day));
      if (b.amount !== null) fin.balances.push({ accountId: b.accountId, day, amount: b.amount });
    }
    for (const r of a.rates as { currency: Cur; rate: number | null }[]) {
      if (r.rate !== null && !(r.rate > 0)) throw "汇率要大于 0";
      fin.rates = fin.rates.filter((x) => !(x.day === day && x.currency === r.currency));
      if (r.rate !== null) fin.rates.push({ day, currency: r.currency, rate: r.rate });
    }
    return null;
  }),
  fin_holdings: () => byDay(fin.holdings).sort((a, b) => a.accountId - b.accountId || a.day.localeCompare(b.day)).map(holdingOut),
  fin_holdings_save: undoable("更新持仓", (a) => {
    if (finAccount(a.accountId as number).kind !== "broker") throw "只有券商账户能记持仓";
    const rows = a.rows as { name: string; quantity: number; price: number }[];
    for (const r of rows) {
      if (!(r.quantity > 0)) throw `「${r.name}」的数量要大于 0`;
      if (!(r.price >= 0)) throw `「${r.name}」的单价不对`;
    }
    fin.holdings = fin.holdings.filter((h) => !(h.accountId === a.accountId && h.day === a.day));
    for (const r of rows) fin.holdings.push({ id: nextId++, accountId: a.accountId as number, day: String(a.day), name: r.name.trim(), quantity: r.quantity, price: r.price });
    return null;
  }),
  fin_entries: (a) => {
    const m = String(a.month);
    return fin.entries
      .filter((e) => e.day <= `${m}-31` && (e.endDay ?? e.day) >= `${m}-01`)
      .sort((x, y) => (y.endDay ?? y.day).localeCompare(x.endDay ?? x.day) || y.day.localeCompare(x.day) || y.id - x.id);
  },
  fin_entry_create: undoable("记一笔", (a) => {
    const e = newEntry(a.patch as Partial<FEntry>);
    checkEntry(e);
    fin.entries.push(e);
    return e;
  }),
  fin_entries_create: undoable("记小额", (a) => {
    const es = (a.patches as Partial<FEntry>[]).map(newEntry);
    es.forEach(checkEntry);
    fin.entries.push(...es);
    return es.length;
  }),
  fin_entry_update: undoable("修改记录", (a) => {
    const e = finEntry(a.id);
    const next = { ...e, ...(a.patch as Partial<FEntry>), updatedAt: now() };
    checkEntry(next);
    Object.assign(e, next);
    return e;
  }),
  fin_entry_delete: undoable("删除记录", (a) => void (fin.entries = fin.entries.filter((e) => e.id !== a.id))),
  fin_entries_update: undoable("修改几笔记录", (a) => {
    const ids = a.ids as number[];
    if (ids.length === 0) throw "没有选中的记录";
    const next = ids.map((id) => ({ ...finEntry(id), ...(a.patch as Partial<FEntry>), updatedAt: now() }));
    next.forEach(checkEntry); // all or nothing
    next.forEach((n) => Object.assign(finEntry(n.id), n));
    return ids.length;
  }),
  fin_entries_delete: undoable("删除几笔记录", (a) => {
    const ids = a.ids as number[];
    ids.forEach(finEntry);
    fin.entries = fin.entries.filter((e) => !ids.includes(e.id));
    return ids.length;
  }),
  fin_period_last: () => fin.entries.reduce<string | null>((m, e) => (e.endDay && (!m || e.endDay > m) ? e.endDay : m), null),
  fin_fixed: () => [...fin.fixed].sort((a, b) => a.sortOrder - b.sortOrder),
  fin_fixed_create: undoable("添加固定支出", (a) => {
    const p = a.patch as Partial<FFixed>;
    const name = (p.name ?? "").trim();
    if (!name) throw "名称不能为空";
    if (fin.fixed.some((f) => f.name === name)) throw `已经有叫「${name}」的固定支出了`;
    const f: FFixed = { id: nextId++, flow: "out", amount: 0, currency: "CNY", kindId: null, rule: "monthly:1", stopped: false, note: null, sortOrder: fin.fixed.length + 1, createdAt: now(), updatedAt: now(), ...p, name, nextDay: p.nextDay ?? nextOnOrAfter(p.rule ?? "monthly:1", mockToday()) } as FFixed;
    if (!(f.amount > 0)) throw "金额要大于 0";
    checkKind(f.flow, f.kindId);
    fin.fixed.push(f);
    postDue(mockToday());
    return f;
  }),
  fin_fixed_update: undoable("修改固定支出", (a) => {
    const f = fin.fixed.find((x) => x.id === a.id)!;
    const next = { ...f, ...(a.patch as Partial<FFixed>), updatedAt: now() };
    if (!(next.amount > 0)) throw "金额要大于 0";
    checkKind(next.flow, next.kindId);
    Object.assign(f, next);
    postDue(mockToday());
    return f;
  }),
  fin_fixed_delete: undoable("删除固定支出", (a) => {
    fin.fixed = fin.fixed.filter((f) => f.id !== a.id);
    for (const e of fin.entries) if (e.fixedId === a.id) e.fixedId = null;
    return null;
  }),
  fin_fixed_reorder: undoable("调整顺序", (a) => reorderPart(fin.fixed, a.ids as number[])),
  fin_budgets: () => [...fin.budgets],
  fin_budgets_save: undoable("设预算", (a) => {
    for (const r of a.rows as { kindId: number; amount: number | null }[]) {
      fin.budgets = fin.budgets.filter((b) => !(b.kindId === r.kindId && b.currency === a.currency));
      if (r.amount !== null) fin.budgets.push({ kindId: r.kindId, currency: a.currency as Cur, amount: r.amount });
    }
    return null;
  }),
  fin_totals: () => {
    const m = new Map<string, { month: string; currency: Cur; flow: string; kindId: number | null; amount: number; count: number }>();
    for (const e of fin.entries) {
      for (const [month, part] of e.endDay ? spread(e.amount, e.day, e.endDay) : [[e.day.slice(0, 7), e.amount] as [string, number]]) {
        const k = `${month}|${e.currency}|${e.flow}|${e.kindId}`;
        const t = m.get(k) ?? { month, currency: e.currency, flow: e.flow, kindId: e.kindId, amount: 0, count: 0 };
        t.amount += part;
        t.count += 1;
        m.set(k, t);
      }
    }
    return [...m.values()].sort((a, b) => a.month.localeCompare(b.month));
  },
  daily_sweep: () => {
    const today = mockToday();
    let closed = 0;
    for (const t of tTasks) if (t.taskType === "checkin" && !t.done && t.dueDate && (t.dueDate < today || allChecked(t))) (markDone(t), closed++);
    return { closed, posted: postDue(today) };
  },
};

// ---- 投石问路 (jobs): mirrors jobs.rs loosely (own steps per application, current = last checked, follow-up) ----
type JStage = "wish" | "applied" | "active" | "got" | "closed";
type JStatus = { id: number; name: string; stage: JStage; sortOrder: number };
type JStep = { statusId: number; done: boolean; doneOn: string | null };
type JRow = {
  id: number; company: string; position: string | null; location: string | null; link: string | null; kindId: number | null;
  resume: string | null; note: string | null; followedOn: string | null; steps: JStep[]; createdAt: string; updatedAt: string;
};
let jb = { statuses: [] as JStatus[], jobs: [] as JRow[] };
(
  [["想投", "wish"], ["已投递", "applied"], ["网申测评 / 笔试", "active"], ["面试", "active"], ["Offer", "got"], ["已接受", "got"], ["被拒", "closed"]] as const
).forEach(([name, stage], i) => jb.statuses.push({ id: nextId++, name, stage, sortOrder: i + 1 }));
["官网", "内推", "Boss 直聘", "牛客", "LinkedIn", "学校", "其他"].forEach((n) => addKind("job_channel", n));
itemsUsing.job_channel = () => jb.jobs as never;
const jSorted = () => [...jb.statuses].sort((a, b) => a.sortOrder - b.sortOrder);
const jStatus = (id: number) => jb.statuses.find((x) => x.id === id)!;
const jStatusNamed = (n: string) => jb.statuses.find((x) => x.name === n)!;
const jOrder = (id: number) => jSorted().findIndex((s) => s.id === id);
const jobOut = (j: JRow) => {
  const steps = [...j.steps].sort((a, b) => jOrder(a.statusId) - jOrder(b.statusId));
  const last = steps.filter((s) => s.done).pop();
  const statusId = last?.statusId ?? jSorted().find((s) => s.stage === "wish")?.id ?? null;
  const stage = statusId !== null ? jStatus(statusId).stage : "wish";
  const appliedOn = steps.filter((s) => s.done && s.doneOn).map((s) => s.doneOn!).sort()[0] ?? null;
  const since = [last?.doneOn, j.followedOn].filter(Boolean).sort().pop() ?? null;
  const waitingDays = stage === "applied" && since ? Math.round((parseYmd(mockToday()).getTime() - parseYmd(since).getTime()) / 86400000) : null;
  return { ...j, steps, statusId, stage, appliedOn, waitingDays, followUp: waitingDays !== null && waitingDays >= 14 };
};
const jDefaultSteps = (): JStep[] => jSorted().filter((s) => s.stage !== "wish" && s.stage !== "closed").map((s) => ({ statusId: s.id, done: false, doneOn: null }));
const jobSave = (j: JRow, p: Partial<JRow>) => {
  Object.assign(j, p, { updatedAt: now() });
  if (!j.company?.trim()) throw "公司不能为空";
  j.company = j.company.trim();
  for (const s of j.steps) {
    const st = jb.statuses.find((x) => x.id === s.statusId);
    if (!st) throw "找不到这个状态，可能已经被删除了";
    if (st.stage === "wish") throw `「${st.name}」是准备阶段，不能放进时间线`;
    if (!s.done) s.doneOn = null;
  }
};
{
  const ch = (n: string) => kindId("job_channel", n);
  const step = (name: string, doneOn: string | null | false = false) => ({ statusId: jStatusNamed(name).id, done: doneOn !== false, doneOn: doneOn || null });
  const rest = (...names: string[]) => names.map((n) => step(n));
  const add = (p: Partial<JRow>) => {
    const j: JRow = { id: nextId++, company: "", position: null, location: null, link: null, kindId: null, resume: null, note: null, followedOn: null, steps: jDefaultSteps(), createdAt: now(), updatedAt: now() };
    jobSave(j, p);
    jb.jobs.push(j);
  };
  add({ company: "KeyReply", position: "软件开发实习", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/template-it-cv-v1.docx", note: "CV 阶段被拒",
    steps: [step("已投递", dayOffset(-60)), ...rest("网申测评 / 笔试", "面试", "Offer", "已接受"), step("被拒", dayOffset(-45))] });
  add({ company: "中国银行新加坡分行", position: "Application Development Assistant（实习）", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/template-it-cv-v1.docx", note: "2026-09-07 入职，金融科技部",
    steps: [step("已投递", "2026-07-20"), step("面试", "2026-08-13"), step("Offer", "2026-08-20"), step("已接受", null)] });
  add({ company: "NUS-ISS 学院代投", position: "实习（学院转投）", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/seek-it-iss-cv-v3.docx",
    steps: [step("已投递", dayOffset(-30)), ...rest("面试", "Offer", "已接受")] });
  add({ company: "腾讯", position: "后台开发", location: "深圳", kindId: ch("官网"), link: "https://join.qq.com",
    steps: [step("已投递", dayOffset(-9)), step("网申测评 / 笔试", dayOffset(-4)), step("面试", dayOffset(-1)), ...rest("Offer", "已接受")] });
  add({ company: "字节跳动", position: "后端开发", location: "北京", kindId: ch("内推"), steps: [step("已投递", dayOffset(-16)), ...rest("面试", "Offer", "已接受")] });
  add({ company: "美团", position: "Java 开发", location: "北京", kindId: ch("牛客"), steps: [step("已投递", dayOffset(-5)), step("网申测评 / 笔试", dayOffset(-2)), ...rest("面试", "Offer", "已接受")] });
  add({ company: "阿里巴巴", position: "Java 开发", location: "杭州", kindId: ch("官网") });
  add({ company: "招商银行", position: "信息技术岗", location: "深圳", kindId: ch("官网"),
    steps: [step("已投递", dayOffset(-20)), step("网申测评 / 笔试", dayOffset(-12)), ...rest("面试", "Offer", "已接受"), step("被拒", dayOffset(-8))] });
}
const jCount = (id: number) => jb.jobs.filter((j) => jobOut(j).statusId === id).length;
const jUsing = (id: number) => jb.jobs.filter((j) => j.steps.some((s) => s.statusId === id)).length;
const jobHandlers: Record<string, (a: Args) => unknown> = {
  job_statuses: () => jSorted().map((st) => ({ ...st, count: jCount(st.id), used: jUsing(st.id) })),
  job_status_create: undoable("添加状态", (a) => {
    const name = String(a.name).trim();
    if (!name) throw "状态名不能为空";
    if (jb.statuses.some((x) => x.name === name)) throw `已经有「${name}」这个状态了`;
    const st = { id: nextId++, name, stage: (a.stage as JStage | undefined) ?? "active", sortOrder: jb.statuses.length + 1 };
    jb.statuses.push(st);
    return { ...st, count: 0, used: 0 };
  }),
  job_status_update: undoable("修改状态", (a) => {
    const st = jStatus(a.id as number);
    if (a.stage === "wish" && st.stage !== "wish" && jUsing(st.id)) throw `有 ${jUsing(st.id)} 条投递的时间线上有「${st.name}」，不能改成准备阶段`;
    if (a.name != null) st.name = String(a.name).trim();
    if (a.stage != null) st.stage = a.stage as JStage;
    return { ...st, count: jCount(st.id), used: jUsing(st.id) };
  }),
  job_status_delete: undoable("删除状态", (a) => {
    const st = jStatus(a.id as number);
    if (jCount(st.id)) throw `还有 ${jCount(st.id)} 条投递是「${st.name}」，先把它们改成别的状态`;
    if (jUsing(st.id)) throw `还有 ${jUsing(st.id)} 条投递的时间线上有「${st.name}」，先从这些投递里去掉它`;
    jb.statuses = jb.statuses.filter((x) => x.id !== a.id);
    return null;
  }),
  job_statuses_reorder: undoable("调整顺序", (a) => reorderPart(jb.statuses, a.ids as number[])),
  jobs_list: () => {
    const moved = (j: ReturnType<typeof jobOut>) => j.steps.map((s) => s.doneOn ?? "").sort().pop() || j.createdAt.slice(0, 10);
    const ord = (j: ReturnType<typeof jobOut>) => (j.statusId === null ? 999 : jOrder(j.statusId));
    return jb.jobs.map(jobOut).sort((x, y) => ord(x) - ord(y) || moved(y).localeCompare(moved(x)) || y.id - x.id);
  },
  /** jobs.rs stats_of */
  job_stats: () => {
    const today = mockToday();
    const week0 = addDays(today, 1 - isoWeekday(today));
    const js = jb.jobs.map(jobOut);
    const sts = jSorted().map((st) => ({ ...st, count: jCount(st.id), used: jUsing(st.id) }));
    const between = (from: string, to: string) => js.filter((j) => j.appliedOn && j.appliedOn >= from && j.appliedOn <= to).length;
    const started = js.filter((j) => j.steps.some((x) => x.done));
    const firstGot = sts.find((x) => x.stage === "got")?.id;
    const funnel = sts
      .filter((x) => x.stage === "applied" || x.stage === "active" || x.id === firstGot)
      .map((x) => {
        const mine = started.flatMap((j) => j.steps.filter((y) => y.statusId === x.id));
        return { statusId: x.id, name: x.name, have: mine.length, done: mine.filter((y) => y.done).length };
      });
    return {
      today,
      applied: js.filter((j) => j.appliedOn).length,
      thisWeek: js.filter((j) => j.appliedOn && j.appliedOn >= week0).length,
      ongoing: js.filter((j) => j.stage === "applied" || j.stage === "active").length,
      got: js.filter((j) => j.stage === "got").length,
      funnel,
      days: Array.from({ length: 30 }, (_, i) => addDays(today, i - 29)).map((d) => ({ from: d, to: d, count: between(d, d) })),
      weeks: Array.from({ length: 12 }, (_, i) => addDays(week0, (i - 11) * 7)).map((d) => ({ from: d, to: addDays(d, 6), count: between(d, addDays(d, 6)) })),
      statuses: sts,
    };
  },
  job_create: undoable("添加投递", (a) => {
    const j: JRow = { id: nextId++, company: "", position: null, location: null, link: null, kindId: null, resume: null, note: null, followedOn: null, steps: jDefaultSteps(), createdAt: now(), updatedAt: now() };
    jobSave(j, a.patch as Partial<JRow>);
    jb.jobs.push(j);
    return jobOut(j);
  }),
  job_update: undoable("修改投递", (a) => {
    const j = jb.jobs.find((x) => x.id === a.id);
    if (!j) throw "找不到这个投递，可能已经被删除了";
    const copy = { ...j, steps: j.steps.map((s) => ({ ...s })) };
    jobSave(copy, a.patch as Partial<JRow>);
    Object.assign(j, copy);
    return jobOut(j);
  }),
  job_follow_up: undoable("已跟进", (a) => {
    const j = jb.jobs.find((x) => x.id === a.id)!;
    j.followedOn = mockToday();
    return jobOut(j);
  }),
  job_delete: undoable("删除投递", (a) => {
    jb.jobs = jb.jobs.filter((x) => x.id !== a.id);
    return null;
  }),
};

// ---- 念念不忘 (notes): mirrors notes.rs loosely (inline tags incl. sub-tags, refs both ways, rename) ----
type NRow = { id: number; content: string; pinned: boolean; refs: number[]; createdAt: string; updatedAt: string };
let nt = { notes: [] as NRow[], meta: new Map<string, { pin: number | null; icon: string | null }>() };
const nTags = (c: string) => [...new Set(c.split("\n").flatMap((l) => tagSpans(l).map((t) => t.value)))];
const nUnder = (t: string, root: string) => t === root || t.startsWith(`${root}/`);
const nBrief = (n: NRow) => ({ id: n.id, createdAt: n.createdAt, excerpt: excerpt(n.content) });
const nOut = (n: NRow) => ({
  ...n,
  tags: nTags(n.content),
  refs: n.refs.map((r) => nt.notes.find((x) => x.id === r)).filter((x): x is NRow => !!x).map(nBrief),
  backrefs: nt.notes.filter((x) => x.refs.includes(n.id)).sort((a, b) => a.createdAt.localeCompare(b.createdAt)).map(nBrief),
});
const nClean = (c: unknown) => {
  const v = String(c ?? "").replace(/\r\n?/g, "\n").trim();
  if (!v) throw "笔记不能是空的";
  return v;
};
{
  const at = (daysAgo: number, hm: string) => {
    const d = parseYmd(dayOffset(-daysAgo));
    const [h, m] = hm.split(":").map(Number);
    d.setHours(h, m, Math.floor(Math.random() * 60));
    return d.toISOString();
  };
  const add = (daysAgo: number, hm: string, content: string, extra: Partial<NRow> = {}) => {
    const n: NRow = { id: nextId++, content, pinned: false, refs: [], createdAt: at(daysAgo, hm), updatedAt: at(daysAgo, hm), ...extra };
    nt.notes.push(n);
    return n;
  };
  const a = add(80, "21:10", "#读书/摘抄 \n「真正的发现之旅不在于寻找新风景，而在于拥有新眼光。」\n——普鲁斯特");
  add(64, "09:30", "#工作/会议 周会\n- 下周一交 **需求文档**\n  - 先给组长看一眼\n- 整理 ==上线清单==\n1. 测试\n2. 发布");
  const c = add(41, "23:02", "#想法 每天花五分钟回看今天记下的东西，比记更多东西有用。");
  add(40, "08:15", "#想法 关于 C++ 和 Rust 的一点 ++随想++：工具只是工具。", { refs: [c.id] });
  add(33, "12:40", "#读书/书单 想读：《置身事内》《枪炮、病菌与钢铁》 https://book.douban.com/");
  add(20, "19:05", "#生活/新加坡 周末去植物园，雨后很凉快。");
  add(12, "22:30", "#读书/摘抄 \n" + Array.from({ length: 14 }, (_, i) => `第 ${i + 1} 段：很长的摘抄，用来看折叠效果。这一段写得长一点，让卡片超过折叠的高度。`).join("\n"), { refs: [a.id] });
  add(3, "10:00", "#想法 #工作 把「念念不忘」当收件箱：想到就记，周末再整理。", { pinned: true });
  add(1, "16:20", "随手记一句，没有标签。");
  add(0, "09:05", "#生活 今天早点睡。");
}
const noteHandlers: Record<string, (a: Args) => unknown> = {
  notes_list: (a) => {
    const f = (a.filter ?? {}) as { tag?: string; day?: string; q?: string; id?: number };
    const words = (f.q ?? "").toLowerCase().split(/\s+/).filter(Boolean);
    const filtered = f.tag || f.day || words.length || f.id != null;
    return nt.notes
      .filter((n) => f.id == null || n.id === f.id)
      .filter((n) => !f.tag || nTags(n.content).some((t) => nUnder(t, f.tag!.replace(/^#/, ""))))
      .filter((n) => !f.day || toYmd(new Date(n.createdAt)) === f.day)
      .filter((n) => words.every((w) => n.content.toLowerCase().includes(w)))
      .sort((x, y) => (filtered ? 0 : Number(y.pinned) - Number(x.pinned)) || y.createdAt.localeCompare(x.createdAt))
      .map(nOut);
  },
  notes_overview: () => {
    const days = new Map<string, number>();
    const under = new Map<string, Set<number>>();
    for (const n of nt.notes) {
      const d = toYmd(new Date(n.createdAt));
      days.set(d, (days.get(d) ?? 0) + 1);
      for (const t of nTags(n.content)) {
        const segs = t.split("/");
        segs.forEach((_, i) => {
          const p = segs.slice(0, i + 1).join("/");
          under.set(p, (under.get(p) ?? new Set()).add(n.id));
        });
      }
    }
    return {
      total: nt.notes.length,
      days: [...days].sort().map(([day, count]) => ({ day, count })),
      tags: [...under].sort().map(([tag, ids]) => ({ tag, count: ids.size, icon: nt.meta.get(tag)?.icon ?? null })),
      pinned: [...nt.meta].filter(([t, m]) => m.pin !== null && !t.includes("/") && under.has(t)).sort((a, b) => a[1].pin! - b[1].pin!).map(([t]) => t),
    };
  },
  notes_tag_set: undoable("标签设置", (a) => {
    const tag = String(a.tag).replace(/^#/, "");
    if (!nt.notes.some((n) => nTags(n.content).some((t) => nUnder(t, tag)))) throw `没有笔记用到「#${tag}」`;
    const p = a.patch as { pinned?: boolean; icon?: string | null };
    if (p.pinned === true && tag.includes("/")) throw "只能置顶第一级标签";
    const m = { ...(nt.meta.get(tag) ?? { pin: null, icon: null }) };
    if (p.pinned === true && m.pin === null) m.pin = Math.max(0, ...[...nt.meta.values()].map((x) => x.pin ?? 0)) + 1;
    if (p.pinned === false) m.pin = null;
    if ("icon" in p) m.icon = p.icon?.trim() || null;
    if (m.pin === null && m.icon === null) nt.meta.delete(tag);
    else nt.meta.set(tag, m);
    return null;
  }),
  note_create: undoable("添加笔记", (a) => {
    const p = a.patch as { content?: string; pinned?: boolean; refs?: number[] };
    const n: NRow = { id: nextId++, content: nClean(p.content), pinned: !!p.pinned, refs: [...new Set(p.refs ?? [])], createdAt: now(), updatedAt: now() };
    nt.notes.push(n);
    return nOut(n);
  }),
  note_update: undoable("修改笔记", (a) => {
    const n = nt.notes.find((x) => x.id === a.id);
    if (!n) throw "找不到这条笔记，可能已经被删除了";
    const p = a.patch as { content?: string; pinned?: boolean; refs?: number[] };
    if (p.content !== undefined) Object.assign(n, { content: nClean(p.content), updatedAt: now() });
    if (p.refs) {
      if (p.refs.includes(n.id)) throw "笔记不能引用它自己";
      n.refs = [...new Set(p.refs)];
    }
    if (p.pinned !== undefined) n.pinned = p.pinned;
    return nOut(n);
  }),
  note_delete: undoable("删除笔记", (a) => {
    nt.notes = nt.notes.filter((x) => x.id !== a.id);
    for (const n of nt.notes) n.refs = n.refs.filter((r) => r !== a.id);
    return null;
  }),
  notes_tag_rename: undoable("标签改名", (a) => {
    const from = String(a.from).replace(/^#/, "");
    const to = String(a.to).trim().replace(/^#/, "").replace(/\/+$/, "");
    if (!to) throw "标签不能为空";
    if (/[\s#]/.test(to)) throw "标签里不能有空格或 #";
    if (to === from) return 0;
    let k = 0;
    for (const n of nt.notes) {
      if (!nTags(n.content).some((t) => nUnder(t, from))) continue;
      n.content = n.content
        .split("\n")
        .map((l) => {
          let out = "";
          let last = 0;
          for (const t of tagSpans(l)) {
            if (!nUnder(t.value, from)) continue;
            out += l.slice(last, t.start + 1) + to + t.value.slice(from.length);
            last = t.end;
          }
          return out + l.slice(last);
        })
        .join("\n");
      k++;
    }
    if (!k) throw `没有笔记用到「#${from}」`;
    for (const [t, m] of [...nt.meta]) {
      if (!nUnder(t, from)) continue;
      nt.meta.delete(t);
      const target = to + t.slice(from.length);
      const old = nt.meta.get(target);
      nt.meta.set(target, { pin: target.includes("/") ? null : (old?.pin ?? m.pin), icon: old?.icon ?? m.icon });
    }
    return k;
  }),
};

// ---- 芝麻开门 (vault): no crypto here — a known test master password and plain rows. URL:
// `&vault=new|open` picks the start (default locked), `&touch=fail|none` how Touch ID behaves.
type VRow = { id: number; name: string; link: string | null; username: string | null; account: string | null; password: string | null; note: string | null; createdAt: string; updatedAt: string };
const vParams = new URLSearchParams(location.search);
const vt = {
  master: vParams.get("vault") === "new" ? null : ("mock-master-1" as string | null),
  open: vParams.get("vault") === "open",
  failed: false,
  touch: vParams.get("touch") !== "none",
  items: [] as VRow[],
};
{
  const add = (p: Partial<VRow>) => vt.items.push({ id: nextId++, name: "", link: null, username: null, account: null, password: null, note: null, createdAt: now(), updatedAt: now(), ...p });
  add({ name: "GitHub", link: "https://github.com/login", username: "pomelo-dev", account: "me@example.com", password: "kfwmqa-Pzhtdv-xr4ena" });
  add({ name: "网易云音乐", link: "music.163.com", account: "138 0000 0000", password: "示例密码 123" });
  add({ name: "学校 VPN", username: "e1234567", password: "abc", note: "每学期要改一次" });
  add({ name: "Wi-Fi 家里", password: "home-wifi-2026" });
}
const vStatus = () =>
  vt.master === null
    ? { state: "new", touch: false, reason: null }
    : vt.open
      ? { state: "open", touch: false, reason: null }
      : { state: "locked", touch: vt.touch && !vt.failed, reason: !vt.touch ? "none" : vt.failed ? "failed" : null };
const vOpen = () => {
  if (!vt.open) throw "芝麻开门已经锁上了";
};
const vFields = (f: Partial<VRow>) => {
  const t = (s: unknown) => (typeof s === "string" && s.trim() ? s.trim() : null);
  if (!String(f.name ?? "").trim()) throw "名称不能为空";
  return { name: String(f.name).trim(), link: t(f.link), username: t(f.username), account: t(f.account), password: f.password || null, note: t(f.note) };
};
const vaultHandlers: Record<string, (a: Args) => unknown> = {
  vault_status: vStatus,
  vault_setup: (a) => {
    if (vt.master !== null) throw "已经设置过主密码了";
    if (String(a.password).length < 8) throw "主密码至少要 8 个字符";
    vt.master = String(a.password);
    vt.open = true;
    return vStatus();
  },
  vault_unlock: (a) => {
    if (a.password !== vt.master) throw "主密码不对";
    Object.assign(vt, { open: true, failed: false });
    return vStatus();
  },
  vault_unlock_touch: async () => {
    await new Promise((r) => setTimeout(r, 600)); // the system prompt
    if (vParams.get("touch") === "fail") {
      vt.failed = true;
      throw "Touch ID 没有通过，请输入主密码";
    }
    vt.open = true;
    return vStatus();
  },
  vault_lock: () => void (vt.open = false),
  vault_touch: () => vt.open,
  vault_change_password: (a) => {
    if (a.old !== vt.master) throw "原来的主密码不对";
    if (String(a.new).length < 8) throw "主密码至少要 8 个字符";
    if (a.old === a.new) throw "新主密码和原来的一样";
    Object.assign(vt, { master: String(a.new), open: true, failed: false });
    return vStatus();
  },
  vault_list: () => {
    vOpen();
    return vt.items.map(({ password, note, createdAt: _c, ...r }) => ({ ...r, hasPassword: !!password, hasNote: !!note }));
  },
  vault_get: (a) => {
    vOpen();
    const r = vt.items.find((x) => x.id === a.id);
    if (!r) throw "找不到这条密码，可能已经被删除了";
    return { ...r };
  },
  vault_create: undoable("添加密码", (a) => {
    vOpen();
    const r: VRow = { id: nextId++, ...vFields(a.fields as Partial<VRow>), createdAt: now(), updatedAt: now() };
    vt.items.push(r);
    return r;
  }),
  vault_update: undoable("修改密码", (a) => {
    vOpen();
    const r = vt.items.find((x) => x.id === a.id)!;
    Object.assign(r, vFields(a.fields as Partial<VRow>), { updatedAt: now() });
    return r;
  }),
  vault_delete: undoable("删除密码", (a) => {
    vOpen();
    vt.items = vt.items.filter((x) => x.id !== a.id);
    return null;
  }),
  vault_copy: (a) => {
    vOpen();
    const r = vt.items.find((x) => x.id === a.id)!;
    (window as unknown as { __mockClipboard: unknown }).__mockClipboard = r[a.field as "password"];
    return null;
  },
  vault_generate: () => "kfwmqa-Pzhtdv-xr4ena",
};

// ---- 全局搜索: mirrors search.rs loosely (all words in the text, ≤ 6 per kind, 芝麻开门 never) ----
function searchAllMock(q: string) {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  type H = { module: string; kind: string; id: number; title: string; detail?: string | null; parent?: number | null; day?: string | null; amount?: number | null; currency?: string | null; flow?: string | null };
  const out: H[] = [];
  const add = (module: string, kind: string, rows: { hay: string; hit: Omit<H, "module" | "kind"> }[]) =>
    out.push(
      ...rows
        .filter((r) => words.every((w) => r.hay.toLowerCase().includes(w)))
        .sort((a, b) => Number(b.hit.title.toLowerCase().startsWith(words[0])) - Number(a.hit.title.toLowerCase().startsWith(words[0])) || b.hit.id - a.hit.id)
        .slice(0, 6)
        .map((r) => ({ detail: null, parent: null, day: null, amount: null, currency: null, flow: null, ...r.hit, module, kind })),
    );
  const t = (...xs: unknown[]) => xs.filter(Boolean).join(" ");
  add("todo", "group", tGroups.map((g) => ({ hay: g.name, hit: { id: g.id, title: g.name } })));
  add("todo", "goal", tGoals.map((g) => ({ hay: t(g.name, g.note, g.review), hit: { id: g.id, title: g.name, detail: tGroups.find((x) => x.id === g.groupId)?.name, parent: g.groupId } })));
  add("todo", "task", tTasks.map((x) => ({ hay: t(x.title, x.note), hit: { id: x.id, title: x.title.replace(/\*\*/g, ""), detail: (tGoals.find((g) => g.id === x.goalId)?.name ?? tGroups.find((g) => g.id === x.groupId)?.name ?? "") + (x.done ? " · 已完成" : ""), parent: x.goalId ? -x.goalId : x.groupId, day: x.dueDate } })));
  for (const [m, k, c] of [["books", "book", "author"], ["films", "film", "director"]] as const)
    add(m, k, (itemsUsing[m]() as unknown as Row[]).map((r) => ({ hay: t(r.title, r[c], r.country, r.note), hit: { id: r.id, title: r.title, detail: r[c] as string | null } })));
  add("drawing", "course", courses.map((c) => ({ hay: t(c.name, c.note), hit: { id: c.id, title: c.name } })));
  add("drawing", "lesson", lessons.map((l) => ({ hay: t(l.code, l.title, l.note), hit: { id: l.id, title: t(l.code, l.title), detail: courses.find((c) => c.id === l.courseId)?.name, parent: l.courseId as number } })));
  add("packing", "trip", tripRows.map((r) => ({ hay: t(r.name, r.note), hit: { id: r.id, title: r.name, day: r.departOn } })));
  add("packing", "template", tpls.map((r) => ({ hay: t(r.name, r.note), hit: { id: r.id, title: r.name, detail: "模板" } })));
  add("packing", "trip_item", tripItems.map((i) => ({ hay: t(i.name, i.note), hit: { id: i.id, title: i.name, detail: tripRows.find((r) => r.id === i.parentId)?.name, parent: i.parentId } })));
  add("packing", "template_item", tplItems.map((i) => ({ hay: t(i.name, i.note), hit: { id: i.id, title: i.name, detail: `模板 · ${tpls.find((r) => r.id === i.parentId)?.name}`, parent: i.parentId } })));
  const kname = (id: number | null) => kinds.find((k) => k.id === id)?.name ?? null;
  add("finance", "entry", fin.entries.map((e) => ({ hay: t(e.note, kname(e.kindId)), hit: { id: e.id, title: e.note ?? kname(e.kindId) ?? "未分类", detail: kname(e.kindId), day: e.day, amount: e.amount, currency: e.currency, flow: e.flow } })));
  add("finance", "fixed", fin.fixed.map((f) => ({ hay: t(f.name, f.note), hit: { id: f.id, title: f.name, detail: "固定支出" } })));
  add("finance", "account", fin.accounts.map((a) => ({ hay: t(a.name, a.note), hit: { id: a.id, title: a.name, detail: "账户" } })));
  add("jobs", "job", jb.jobs.map((j) => ({ hay: t(j.company, j.position, j.location, j.note), hit: { id: j.id, title: j.position ? `${j.company} · ${j.position}` : j.company, detail: j.location } })));
  add("notes", "note", nt.notes.map((n) => ({ hay: n.content, hit: { id: n.id, title: excerpt(n.content), day: toYmd(new Date(n.createdAt)) } })));
  return out;
}

// ---- 首页: mirrors home.rs loosely ----
function homeMock(hidden: string[]) {
  const shown = (m: string) => !hidden.includes(m);
  const today = mockToday();
  const year = today.slice(0, 4);
  const hit = (module: string, kind: string, id: number, title: string, detail: string | null = null, parent: number | null = null) =>
    ({ module, kind, id, title, detail, parent, day: null as string | null, amount: null as number | null, currency: null as string | null, flow: null as string | null });
  const books = handlers.books_list({}) as Row[];
  const films = handlers.films_list({}) as Row[];
  const read = (rows: Row[]) => rows.filter((r) => (r.status === "done" && r.finishedYear === year) || (r.rereads as { year: string | null }[]).some((x) => x.year === year)).length;
  const jobs = handlers.jobs_list({}) as ReturnType<typeof jobOut>[];
  const week0 = addDays(today, 1 - isoWeekday(today));
  const jobLine = (j: ReturnType<typeof jobOut>) => ({
    ...hit("jobs", "job", j.id, j.position ? `${j.company} · ${j.position}` : j.company, j.location),
    status: j.statusId !== null ? jStatus(j.statusId).name : "",
    stage: j.stage,
    waitingDays: j.waitingDays,
  });
  const recent = [
    ...nt.notes.map((n) => ({ ...hit("notes", "note", n.id, excerpt(n.content)), at: n.createdAt, done: false })),
    ...tTasks.filter((t) => t.done && t.doneAt).map((t) => ({ ...hit("todo", "task", t.id, t.title.replace(/\*\*/g, ""), (t.goalId ? tGoals.find((g) => g.id === t.goalId)?.name : tGroups.find((g) => g.id === t.groupId)?.name) ?? null, t.goalId ? -t.goalId : t.groupId), at: t.doneAt!, done: true })),
    ...books.map((b) => ({ ...hit("books", "book", b.id, b.title, (b.author as string) ?? null), at: b.createdAt as string, done: false })),
    ...films.map((f) => ({ ...hit("films", "film", f.id, f.title, (f.director as string) ?? null), at: f.createdAt as string, done: false })),
    ...courses.map((c) => ({ ...hit("drawing", "course", c.id, c.name), at: c.createdAt, done: false })),
    ...tripRows.map((t) => ({ ...hit("packing", "trip", t.id, t.name), at: t.createdAt, done: false })),
    ...jb.jobs.map((j) => ({ ...hit("jobs", "job", j.id, j.position ? `${j.company} · ${j.position}` : j.company, j.location), at: j.createdAt, done: false })),
    ...fin.entries.map((e) => ({
      ...hit("finance", "entry", e.id, e.note ?? kinds.find((k) => k.id === e.kindId)?.name ?? "未分类", kinds.find((k) => k.id === e.kindId)?.name ?? null),
      day: e.day, amount: e.amount, currency: e.currency, flow: e.flow, at: e.createdAt, done: false,
    })),
  ]
    .filter((r) => shown(r.module))
    .sort((a, b) => b.at.localeCompare(a.at))
    .slice(0, 8);
  return {
    today,
    year: {
      year,
      books: shown("books") ? read(books) : null,
      films: shown("films") ? read(films) : null,
      lessons: shown("drawing") ? lessons.filter((l) => l.status === "done" && String(l.finishedMonth ?? "").startsWith(year)).length : null,
      todos: shown("todo") ? tTasks.filter((t) => t.done && t.doneAt?.startsWith(year)).length + tLogs.filter((l) => l.day.startsWith(year)).length : null,
      notes: shown("notes") ? nt.notes.filter((n) => n.createdAt.startsWith(year)).length : null,
      jobs: shown("jobs") ? jobs.filter((j) => j.appliedOn?.startsWith(year)).length : null,
    },
    assets: shown("finance")
      ? (handlers.fin_assets({}) as { totals: { day: string; totalCny: number | null }[] }).totals.filter((t) => t.totalCny !== null).map((t) => ({ day: t.day, total: t.totalCny }))
      : null,
    jobs: shown("jobs")
      ? {
          total: jobs.length,
          thisWeek: jobs.filter((j) => j.appliedOn && j.appliedOn >= week0).length,
          ongoing: jobs.filter((j) => j.stage === "applied" || j.stage === "active").length,
          got: jobs.filter((j) => j.stage === "got").length,
          follow: jobs.filter((j) => j.followUp).sort((a, b) => (b.waitingDays ?? 0) - (a.waitingDays ?? 0)).map(jobLine),
          active: jobs.filter((j) => j.stage === "active").map(jobLine),
        }
      : null,
    reading:
      shown("books") || shown("films")
        ? [
            ...(shown("books") ? books.filter((b) => b.status === "doing").map((b) => hit("books", "book", b.id, b.title, (b.author as string) ?? null)) : []),
            ...(shown("films") ? films.filter((f) => f.status === "doing").map((f) => hit("films", "film", f.id, f.title, (f.director as string) ?? null)) : []),
          ]
        : null,
    drawing: shown("drawing")
      ? (handlers.drawing_courses({}) as ReturnType<typeof withCounts>[])
          .filter((c) => c.total > 0 && c.done < c.total)
          .map((c) => {
            const ls = lessons.filter((l) => l.courseId === c.id && l.status !== "done").sort((a, b) => Number(b.status === "doing") - Number(a.status === "doing") || (a.sortOrder as number) - (b.sortOrder as number));
            const n = ls[0];
            return { id: c.id, name: c.name, total: c.total, done: c.done, doing: c.doing, next: n ? hit("drawing", "lesson", n.id, `${n.code ? `${n.code} ` : ""}${n.title}`, c.name, c.id) : null };
          })
      : null,
    recent,
  };
}

// ---- 小拾 (picky): simulated models (start 0.7 s → loading → ready 1.2 s), cloud list + keys, sessions with
// a linear path, replies streamed over the Channel. `&picky=ready` starts with a local model running.
type PModel = { key: string; name: string; title: string; note: string | null; quant: string; alias: string; context: number; thinking: boolean; listed: boolean };
type PIssue = { kind: string; title: string; message: string; details: string | null; actions: { kind: string; key?: string }[] };
type PMsg = { id: number; role: "user" | "assistant"; content: string; reasoning: string; reasoningMs: number | null; state: string; createdAt: string; model: string | null; error: string | null };
// ---- phase 5: stored copies, project files (files.rs / index.rs) ----
type PArtifact = { id: number; name: string; format: "PDF" | "Markdown" | "文本"; pageCount: number | null; byteCount: number; deleted: boolean; path: string; segments: string[] };
type PFile = { id: number; projectId: number; currentVersion: number; indexed: boolean; versions: { version: number; artifact: number; addedAt: string }[] };
type PIndex = { kind: string; done?: number; total?: number; message?: string };
type PUsed = { attachments: number[]; files: { fileId: number; version: number; artifact: number }[] };
let pPickCount = 0;
// ---- phase 6: memories (memory.rs) ----
type PMemory = { id: number; content: string; projectId: number | null; sharedProjectIds: number[]; enabled: boolean; localOnly: boolean; source: string; sourceSessionId: number | null; notedOn: string; createdAt: string; updatedAt: string };
const pToday = () => toYmd(new Date());
const pMemoryValid = (t: string) => {
  const v = t.trim();
  if (!v || [...v].length > 300) throw "记忆需要 1 至 300 个字。";
  return v;
};
const pModels: PModel[] = (
  [
    ["qwen14b", "Qwen3 14B", "Qwen3 14B Abliterated", "Q4_K_M", "qwen3-14b-abliterated", 16384, true],
    ["qwen359b", "Qwen3.5 9B", "Qwen3.5 9B", "Q8_0", "qwen3.5-9b", 32768, true],
    ["qwen35heretic9b", "Qwen3.5 9B Heretic", "Qwen3.5 9B Heretic ARA Refusals5", "Q8_0", "qwen3.5-9b-heretic-ara-refusals5", 32768, true],
    ["ministral14b", "Ministral 3 14B", "Ministral 3 14B Abliterated", "Q4_K_M", "ministral-3-14b-abliterated", 16384, false],
  ] as const
).map(([key, name, title, quant, alias, context, thinking]) => ({ key, name, title, note: "模拟清单", quant, alias, context, thinking, listed: true }));
const pky = {
  active: (new URLSearchParams(location.search).get("picky") === "ready" ? pModels[1] : null) as PModel | null,
  readiness: (new URLSearchParams(location.search).get("picky") === "ready" ? "ready" : "idle") as "idle" | "loading" | "ready",
  op: null as null | { kind: "starting" | "switching" | "stopping"; model: PModel | null },
  issue: null as PIssue | null,
  emb: { state: "stopped" as "stopped" | "starting" | "running", working: false },
  selection: "local",
  cloud: [
    { id: "deepseek-flash", name: "DeepSeek Flash", apiModel: "deepseek-flash", service: "deepSeek", contextWindow: 1048576, maxOutputTokens: 8192 },
    { id: "deepseek-v4-pro", name: "DeepSeek V4 Pro", apiModel: "deepseek-v4-pro", service: "deepSeek", contextWindow: 1048576, maxOutputTokens: 8192 },
  ] as { id: string; name: string; apiModel: string; service: string; contextWindow: number; maxOutputTokens: number }[],
  keys: new Set<string>(),
  sessions: [] as PSession[],
  projects: [] as { id: number; name: string; instructions: string; sortOrder: number; includeGlobalMemories?: boolean }[],
  memories: [] as PMemory[],
  memoryOn: true,
  memoryOffSessions: new Set<number>(),
  runMemories: new Map<number, { memoryId: number | null; content: string; notedOn: string }[]>(),
  manual: new Set<string>(),
  ui: new Map<string, unknown>(),
  run: null as null | { id: number; session: number; kind: "reply" | "summary"; msg: PMsg | null; timer: number; done: (state: string, error: string | null) => void; reading?: { current: number; total: number; label: string } | null },
  templates: [] as { id: number; title: string; category: string; body: string; pinnedAt: string | null; lastUsedAt: string | null }[],
  presets: [] as { id: number; name: string; content: string }[],
  global: "",
  summaries: [] as { id: number; sessionId: number; through: number; content: string }[],
  artifacts: [] as PArtifact[],
  files: [] as PFile[],
  /** project file → index job state; built indexes by artifact */
  jobs: new Map<number, PIndex>(),
  built: new Set<number>(),
  used: new Map<number, PUsed>(),
  notes: new Map<number, { label: string; content: string }[]>(),
  // ---- v0.3.5: lookups in Pomelo (data.rs) ----
  queries: new Map<number, PQuery[]>(),
  dataCloud: new Map<string, boolean>(),
  // ---- phase 7: space (storage.rs) ----
  strays: [{ name: "a3f9c2-导入失败留下的.pdf", bytes: 245000 }],
};
const pOldEmpty = (s: PSession) => !s.nodes.length && Date.now() - Date.parse(s.updatedAt) > 30 * 86400000;
const pInfo = (a: PArtifact) => ({ id: a.id, name: a.name, format: a.format, pageCount: a.pageCount, byteCount: a.byteCount, deleted: a.deleted });
const pArt = (id: number) => {
  const a = pky.artifacts.find((x) => x.id === id);
  if (!a || a.deleted) throw "附件不存在了，请重新选择";
  return a;
};
const pCurrent = (f: PFile) => pky.artifacts.find((a) => a.id === f.versions.find((v) => v.version === f.currentVersion)?.artifact);
const pMemoryPlan = (sessionId: number | null, projectId: number | null, draftOff: boolean, nextOff: boolean) => {
  const sess = sessionId != null ? pky.sessions.find((x) => x.id === sessionId) : null;
  const project = sess ? sess.projectId : projectId;
  const conversationOff = sess ? pky.memoryOffSessions.has(sess.id) : draftOff;
  const out = { off: null as string | null, count: 0, global: 0, project: 0, shared: 0, skippedLocalOnly: 0, conversationOff, candidates: [] as PMemory[] };
  if (!pky.memoryOn) out.off = "everywhere";
  else if (conversationOff) out.off = "conversation";
  else if (nextOff) out.off = "nextMessage";
  if (out.off) return out;
  const includeGlobal = pky.projects.find((x) => x.id === project)?.includeGlobalMemories ?? true;
  const cloud = pky.selection.startsWith("cloud:");
  for (const m of [...pky.memories].reverse()) {
    if (!m.enabled) continue;
    const applies = project == null ? m.projectId == null : m.projectId == null ? includeGlobal : m.projectId === project || m.sharedProjectIds.includes(project);
    if (!applies) continue;
    if (cloud && m.localOnly) {
      out.skippedLocalOnly++;
      continue;
    }
    if (m.projectId == null) out.global++;
    else if (m.projectId === project) out.project++;
    else out.shared++;
    out.candidates.push(m);
  }
  out.count = out.candidates.length;
  return out;
};
const pFileView = (f: PFile) => {
  const cur = pCurrent(f);
  const index = !f.indexed ? { kind: "none" } : (pky.jobs.get(f.id) ?? (cur && pky.built.has(cur.id) ? { kind: "ready" } : { kind: "incomplete" }));
  return { id: f.id, projectId: f.projectId, currentVersion: f.currentVersion, indexed: f.indexed, versions: f.versions.map((v) => ({ version: v.version, addedAt: v.addedAt, file: pInfo(pky.artifacts.find((a) => a.id === v.artifact)!) })), index };
};
const pBuild = (f: PFile) => {
  const total = 8;
  let done = 0;
  pky.jobs.set(f.id, { kind: "building", done, total });
  const tick = () => {
    if (pky.jobs.get(f.id)?.kind !== "building") return;
    done += 1;
    if (done >= total) {
      pky.jobs.delete(f.id);
      const cur = pCurrent(f);
      if (cur && f.indexed) pky.built.add(cur.id);
      return;
    }
    pky.jobs.set(f.id, { kind: "building", done, total });
    window.setTimeout(tick, 300);
  };
  window.setTimeout(tick, 300);
};
const pMessageFiles = (m: PNode) => {
  const u = pky.used.get(m.id);
  if (!u) return [];
  return [
    ...u.attachments.map((id) => ({ ...pInfo(pky.artifacts.find((a) => a.id === id)!), fileId: null, version: null, label: null })),
    ...u.files.map((f) => ({ ...pInfo(pky.artifacts.find((a) => a.id === f.artifact)!), fileId: f.fileId, version: f.version, label: f.version > 1 ? `资料 · 第 ${f.version} 版` : "资料" })),
  ];
};
/** What a question sends: attachments of earlier questions and its own, its project files. */
const pMaterial = (_s: PSession, chain: PNode[], used: PUsed) => {
  const ids: number[] = [];
  for (const m of chain) if (m.role === "user") for (const id of pky.used.get(m.id)?.attachments ?? []) if (!ids.includes(id)) ids.push(id);
  for (const id of used.attachments) if (!ids.includes(id)) ids.push(id);
  const arts = [...ids, ...used.files.map((f) => f.artifact).filter((id) => !ids.includes(id))].map((id) => pky.artifacts.find((a) => a.id === id)!).filter((a) => a && !a.deleted);
  return arts.flatMap((a) => a.segments.map((seg, i) => ({ label: `${a.name} · ${a.format === "PDF" ? `第 ${i + 1} 页` : `第 ${i + 1} 段`}`, content: seg })));
};
const pPresent = () => {
  const p = (dot: string, title: string, menuTitle = title, detail = "") => ({ dot, title, menuTitle, detail });
  if (pky.op?.kind === "switching") return p("loading", `正在切换到 ${pky.op.model!.name}…`);
  if (pky.op?.kind === "stopping") return p("loading", `正在停止 ${pky.op.model?.name ?? "聊天模型"}…`);
  if (pky.op?.kind === "starting") return p("loading", `正在加载 ${pky.op.model!.name}…`);
  if (pky.issue && (!pky.active || pky.issue.kind === "operation")) return p("error", "需要处理", "需要处理", pky.issue.title);
  if (pky.active && pky.readiness === "ready") return p("ready", pky.active.name, `${pky.active.name} · 已就绪`, "已就绪");
  if (pky.active) return p("loading", `正在加载 ${pky.active.name}…`);
  return p("off", "未运行聊天模型", "未运行聊天模型", "未运行");
};
const pStatus = () => ({
  mode: "mock",
  error: null,
  local: { models: pModels, catalogError: null, active: pky.active, readiness: pky.readiness, served: pky.readiness === "ready" ? pky.active?.alias ?? null : null, working: !!pky.op, issue: pky.issue, presentation: pPresent() },
  embedding: { ...pky.emb, text: pky.emb.working ? "正在处理…" : { stopped: "未运行", starting: "正在启动…", running: "已就绪" }[pky.emb.state], issue: null },
  selection: pky.cloud.some((m) => `cloud:${m.id}` === pky.selection) ? pky.selection : "local",
  cloud: pky.cloud,
  keys: (["openRouter", "deepSeek", "openAI"] as const).map((s) => ({ service: s, name: { openRouter: "OpenRouter", deepSeek: "DeepSeek", openAI: "OpenAI" }[s], saved: pky.keys.has(s) })),
  generating: !!pky.run,
  activeRun: pky.run?.id ?? null,
  activeSession: pky.run?.session ?? null,
  activeKind: pky.run?.kind ?? null,
  activeReading: pky.run?.reading ?? null,
});
const pActivate = (key: string) => {
  const m = pModels.find((x) => x.key === key);
  if (!m) return void (pky.issue = { kind: "operation", title: `无法启动 ${key}`, message: "这个模型已不在模型清单中，所以没有启动。", details: null, actions: [{ kind: "openCatalog" }] });
  pky.op = { kind: pky.active ? "switching" : "starting", model: m };
  pky.issue = null;
  pky.active = null;
  pky.readiness = "idle";
  setTimeout(() => ((pky.active = m), (pky.readiness = "loading")), 700);
  setTimeout(() => ((pky.readiness = "ready"), (pky.op = null)), 1900);
};
const MOCK_REASONING = "用户想检查排版。先列出要覆盖的格式：标题、列表、引用、表格、代码，\n再放几处不该生效的输入，比如原始 HTML。";
const MOCK_REPLY = "## 排版自检\n\n这是开发版的**模拟回复**，用来检查消息的显示；正式版才会连接真正的模型。\n这一行紧跟在上一行后面。\n\n### 列表\n\n1. 第一项：包含 `行内代码`\n2. 第二项，下面还有子项：\n   - 子项 A\n   - 子项 B，带一个[链接](https://example.com)\n3. 第三项\n\n> 引用段落：阅读旧消息时，界面不会被拉到底部。\n\n| 模型 | 量化 | 上下文 | 适合的任务 | 备注 |\n| --- | --- | --- | --- | --- |\n| Qwen3 14B | Q4_K_M | 16K | 中文写作、日常问答 | 会先思考再回答 |\n| Qwen3.5 9B | Q8_0 | 32K | 代码解释与长资料阅读 | 源自官方权重的转换版 |\n\n```rust\nfn greet(name: &str) -> String {\n    format!(\"你好，{name}\") // 这一行故意写得很长，用来检查长代码能否横向滚动而不撑破窗口\n}\n```\n\n行内公式 $E = mc^2$，金额 $5 和 $10 不是公式。独立公式：\n\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\n\n这些应该原样显示：<b>不是粗体</b>。\n\n最后一段。";
type Emit = (e: Record<string, unknown>) => void;
// ---- v0.3.5 lookups (picky/data.rs, its mock_calls / mock_answer): a few views for real, the rest generic ----
type PQuery = { module: string | null; label: string; state: string; result: string; records: { module: string; kind: string; id: number; title: string; day?: string }[] };
const P_MODULES: [string, string, boolean][] = [
  ["todo", "一步一步", true], ["books", "摸鱼读读", true], ["films", "好奇看看", true], ["drawing", "随意画画", true],
  ["packing", "出门走走", true], ["finance", "钱来钱往", false], ["jobs", "投石问路", true], ["notes", "念念不忘", true],
];
const pDataCloud = () => P_MODULES.map(([module, label, def]) => ({ module, label, allowed: pky.dataCloud.get(module) ?? def }));
const pMoney = (cents: number, cur: Cur = "CNY") => {
  const sym = { CNY: "¥", SGD: "S$", HKD: "HK$", USD: "US$" }[cur];
  const a = Math.abs(cents);
  const units = Math.floor(a / 100).toLocaleString("en-US");
  return `${cents < 0 ? "-" : ""}${sym}${units}${a % 100 ? "." + String(a % 100).padStart(2, "0") : ""}`;
};
const pMd = (d: string) => `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8))} 日`;
const pSpan = (from: string, to: string) => (from.slice(0, 4) === to.slice(0, 4) ? `${from.slice(0, 4)} 年 ${pMd(from)} – ${pMd(to)}` : `${from.slice(0, 4)} 年 ${pMd(from)} – ${to.slice(0, 4)} 年 ${pMd(to)}`);
const pMockCalls = (q: string): [string, Record<string, string>][] => {
  const has = (...w: string[]) => w.some((x) => q.includes(x));
  const out: [string, Record<string, string>][] = [];
  if (has("跟进")) out.push(["jobs", { view: "follow_up" }]);
  else if (has("进展")) out.push(["jobs", { view: "moves" }]);
  else if (has("漏斗", "投递")) out.push(["jobs", { view: "stats" }]);
  if (has("资产")) out.push(["finance", { view: "assets" }]);
  else if (has("花", "支出", "钱")) out.push(["finance", { view: "entries" }]);
  if (has("拖", "没动")) out.push(["todo", { view: "stuck" }]);
  else if (has("待办", "做完", "任务")) out.push(["todo", { view: q.includes("今天") ? "today" : "done" }]);
  if (has("书")) out.push(["books", { view: has("推荐") ? "taste" : "list" }]);
  if (has("电影", "剧", "片")) out.push(["films", { view: has("推荐") ? "taste" : "list" }]);
  if (has("画")) out.push(["drawing", { view: "courses" }]);
  if (has("出门", "行李", "漏带")) out.push(["packing", { view: "trip" }]);
  if (has("笔记")) out.push(["notes", { view: "recent" }]);
  const hidden = (settings.get("ui.module_hidden") as string[] | undefined) ?? [];
  return out.filter(([m]) => !hidden.includes(m));
};
const pLookup = (module: string, args: Record<string, string>): PQuery => {
  const label = P_MODULES.find((m) => m[0] === module)![1];
  const today = mockToday();
  if (pky.selection.startsWith("cloud:") && !pDataCloud().find((m) => m.module === module)!.allowed)
    return { module, label: `${label} · 不发给云端，没看`, state: "blocked", result: `没有查：${label}的数据不发给云端模型。请在回答开头写明「${label}不发给云端，这部分没看」，并提醒可以换本地模型再问。`, records: [] };
  const q = (l: string, lines: string[], records: PQuery["records"] = []): PQuery => ({ module, label: `${label} · ${l}`, state: "ok", result: lines.join("\n"), records });
  if (module === "jobs" && args.view === "follow_up") {
    const js = (jobHandlers.jobs_list({}) as ReturnType<typeof jobOut>[]).filter((j) => j.stage === "applied" && j.waitingDays !== null);
    const sunday = addDays(addDays(today, 1 - isoWeekday(today)), 6);
    const now = js.filter((j) => j.followUp).sort((a, b) => b.waitingDays! - a.waitingDays!);
    const soon = js.filter((j) => !j.followUp && addDays(today, 14 - j.waitingDays!) <= sunday);
    const line = (j: ReturnType<typeof jobOut>) => `- ${j.position ? `${j.company} · ${j.position}` : j.company}${j.location ? ` · ${j.location}` : ""}：等了 ${j.waitingDays} 天`;
    const rec = (j: ReturnType<typeof jobOut>) => ({ module: "jobs", kind: "job", id: j.id, title: j.position ? `${j.company} · ${j.position}` : j.company });
    return q(
      `该跟进的投递（${now.length} 份${soon.length ? `，本周还有 ${soon.length} 份` : ""}）`,
      [`今天 ${today}。在等回音的投递，离那一步（或上次跟进）满 14 天就该跟进。现在该跟进 ${now.length} 份；本周内（到 ${sunday}）还会到期 ${soon.length} 份。`, now.length ? "现在该跟进：" : "现在该跟进：没有", ...now.map(line), soon.length ? "本周内会到期：" : "本周内会到期：没有", ...soon.map((j) => `${line(j)}，${addDays(today, 14 - j.waitingDays!)} 到期`)],
      [...now, ...soon].map(rec),
    );
  }
  if (module === "jobs") {
    const st = jobHandlers.job_stats({}) as { applied: number; thisWeek: number; ongoing: number; got: number; funnel: { name: string; have: number; done: number }[] };
    return q("统计", [`今天 ${today}。投出 ${st.applied} 份（有投递日期的），本周投了 ${st.thisWeek} 份，进行中 ${st.ongoing} 份，拿到 ${st.got} 份。`, "漏斗（只看至少勾了一步的投递）：", ...st.funnel.map((x) => `- ${x.name}：${x.done} / ${x.have} 份（勾上的 / 时间线上有这一步的）`)]);
  }
  if (module === "finance" && args.view === "assets") {
    const from = addDays(today, -182);
    const ts = assetsOut().totals.filter((t) => t.day >= from && t.day <= today);
    const full = ts.filter((t) => t.totalCny !== null);
    const head = [`总资产 = 每次更新余额那天各账户最近的余额按汇率折成人民币。${pSpan(from, today)}（${from} 至 ${today}）更新过 ${ts.length} 次。`];
    if (full.length) {
      const [a, b] = [full[0], full[full.length - 1]];
      head.push(`最早 ${a.day} ${pMoney(a.totalCny!)}，最近 ${b.day} ${pMoney(b.totalCny!)}，变化 ${b.totalCny! >= a.totalCny! ? "+" : ""}${pMoney(b.totalCny! - a.totalCny!)}。`);
    }
    return q(`总资产 ${pSpan(from, today)}（${ts.length} 次更新）`, [...head, "每次更新的总资产：", ...ts.map((t) => (t.totalCny === null ? `- ${t.day} 缺汇率，没算总数` : `- ${t.day} ${pMoney(t.totalCny)}`))]);
  }
  if (module === "finance") {
    const from = `${today.slice(0, 7)}-01`;
    const es = fin.entries.filter((e) => e.flow === "out" && e.day <= today && (e.endDay ?? e.day) >= from);
    const name = (e: FEntry) => kinds.find((k) => k.id === e.kindId)?.name ?? "未分类";
    return q(`${pSpan(from, today)}支出（${es.length} 笔）`, [`${pSpan(from, today)}（${from} 至 ${today}）支出 ${es.length} 笔。`, ...es.map((e) => `- ${e.day} 支出 ${name(e)} ${pMoney(e.amount, e.currency)}${e.note ? ` · ${e.note}` : ""}`)], es.map((e) => ({ module: "finance", kind: "entry", id: e.id, title: e.note ?? name(e), day: e.day })));
  }
  return q("模拟查询", [`（浏览器模拟没有实现${label}的这个查询：${JSON.stringify(args)}）`]);
};
const pAnswer = (qs: PQuery[]) => {
  const starts = qs.map((x) => x.result.split("请在回答开头写明「")[1]?.split("」")[0]).filter(Boolean).map((x) => `${x}。`);
  const quotes = qs.filter((x) => x.state === "ok").map((x) => `\n> ${x.result.split("\n").slice(0, 4).join("\n> ")}\n`);
  const body = quotes.length ? `这是开发版的**模拟回答**，下面是查到的内容的开头：\n${quotes.join("")}` : "这是开发版的**模拟回答**。";
  return `${starts.join("")}${starts.length ? "\n\n" : ""}${body}`;
};
const pStream = (msg: PMsg, thinking: boolean, prompt: string, emit: Emit, done: (state: string, error: string | null) => void, answer?: string) => {
  if (prompt.includes("失败测试")) return window.setTimeout(() => done("failed", "模拟的失败：模型回复意外中断。"), 300);
  const reply = answer ?? (prompt.includes("滚动测试") ? Array.from({ length: 60 }, (_, i) => `第 ${i + 1} 段模拟回复：这里有一些用于检查长内容滚动的文字。\n\n`).join("") : MOCK_REPLY);
  const parts: [boolean, string][] = [...(thinking ? ([[true, MOCK_REASONING]] as [boolean, string][]) : []), [false, reply]];
  let pi = 0, ci = 0, step = 0, started = 0;
  const tick = () => {
    if (pi >= parts.length) return done("completed", null);
    const [isR, text] = parts[pi];
    const chars = [...text];
    const piece = chars.slice(ci, ci + 3 + (step++ % 5)).join("");
    ci += [...piece].length;
    if (isR) {
      started ||= Date.now();
      msg.reasoning += piece;
      emit({ kind: "reasoning", text: piece });
    } else {
      if (started && msg.reasoningMs == null) msg.reasoningMs = Date.now() - started;
      msg.content += piece;
      emit({ kind: "content", text: piece });
    }
    if (ci >= chars.length) (pi++, (ci = 0));
    if (pky.run) pky.run.timer = window.setTimeout(tick, 20);
  };
  return window.setTimeout(tick, 20);
};

type PNode = PMsg & { parent: number | null; activeChild: number | null };
type PSession = {
  id: number; title: string; titleSource: "auto" | "user"; updatedAt: string; root: number | null; nodes: PNode[];
  projectId: number | null; pinned: boolean; archived: boolean; sortOrder: number | null;
  instructions: string; presetId: number | null;
};
const pNewSession = (title: string, projectId: number | null = null): PSession => ({
  id: nextId++, title, titleSource: "auto", updatedAt: now(), root: null, nodes: [], projectId, pinned: false, archived: false, sortOrder: null,
  instructions: "", presetId: null,
});
// ---- phase 4: templates, instructions, context (instructions.rs / templates.rs / context.rs) ----
const pCmp = (x: string, y: string) => (x < y ? -1 : x > y ? 1 : 0);
const pTemplates = () =>
  [...pky.templates]
    .sort((a, b) => (a.pinnedAt == null ? 1 : 0) - (b.pinnedAt == null ? 1 : 0) || pCmp(a.pinnedAt ?? "", b.pinnedAt ?? "") || pCmp(a.category, b.category) || pCmp(a.title, b.title) || a.id - b.id)
    .map(({ id, title, category, body, pinnedAt, lastUsedAt }) => ({ id, title, category, body, pinned: pinnedAt != null, lastUsedAt }));
const pLayers = (global: string, project: string, session: string, preset: string) =>
  (
    [
      ["global", "全局", global],
      ["project", "项目", project],
      ["session", "会话", session],
      ["preset", "预设", preset],
    ] as const
  )
    .map(([kind, label, c]) => ({ kind, label, content: c.trim() }))
    .filter((l) => l.content);
type PSetting = { content: string; presetId: number | null };
const pPreview = (sessionId: number | null, projectId: number | null, draft: PSetting | null) => {
  const sess = sessionId == null ? null : pFind(sessionId);
  const pid = sess ? sess.projectId : projectId;
  const project = pky.projects.find((x) => x.id === pid)?.instructions ?? "";
  const setting = sess ? { content: sess.instructions, presetId: sess.presetId } : { content: draft?.content ?? "", presetId: pky.presets.some((x) => x.id === draft?.presetId) ? draft!.presetId : null };
  const preset = pky.presets.find((x) => x.id === setting.presetId)?.content ?? "";
  return { layers: pLayers(pky.global, project, setting.content, preset), session: setting };
};
const MOCK_WINDOW = 8192;
const pEstimate = (t: string) => {
  let w = 0, o = 0;
  for (const ch of t) {
    const v = ch.codePointAt(0)!;
    if ((v >= 0x2e80 && v <= 0x9fff) || (v >= 0xac00 && v <= 0xd7af) || (v >= 0xf900 && v <= 0xfaff) || (v >= 0xfe30 && v <= 0xfe4f) || (v >= 0xff00 && v <= 0xffef) || (v >= 0x20000 && v <= 0x3ffff)) w++;
    else o++;
  }
  return w + Math.floor((o + 2) / 3) + 1;
};
const pReserve = (w: number) => Math.min(8192, Math.max(1024, Math.floor(w / 4)));
const pApplicable = (sid: number, ids: number[]) => {
  let best: { i: number; x: (typeof pky.summaries)[number] } | null = null;
  for (const x of [...pky.summaries].reverse()) {
    const i = ids.indexOf(x.through);
    if (x.sessionId === sid && i >= 0 && (!best || i > best.i)) best = { i, x };
  }
  return best;
};
/** context.rs request(): sizes of what goes, before fitting. */
const pRequest = (s: PSession, turns: PNode[], prompt: string) => {
  const system = pPreview(s.id, null, null).layers.map((l) => l.content).join("\n\n");
  const sum = pApplicable(s.id, turns.map((t) => t.id));
  const history = turns.slice(sum ? sum.i + 1 : 0).filter((t) => t.role === "user" || (t.state !== "streaming" && t.content.trim()));
  return {
    system: system ? pEstimate(system) : null,
    summary: sum ? pEstimate("以下是较早对话的摘要，仅供延续对话参考：\n\n" + sum.x.content) : null,
    history: history.map((t) => pEstimate(t.content)),
    prompt: pEstimate(prompt),
  };
};
const pSelect = (r: ReturnType<typeof pRequest>) => {
  const reserve = pReserve(MOCK_WINDOW);
  let used = 32 + r.prompt + 8 + (r.system != null ? r.system + 8 : 0) + (r.summary != null ? r.summary + 8 : 0);
  if (used + reserve > MOCK_WINDOW) return null;
  let kept = 0;
  for (const t of [...r.history].reverse()) {
    if (used + t + 8 + reserve > MOCK_WINDOW) break;
    used += t + 8;
    kept++;
  }
  return { dropped: r.history.length - kept };
};
const pFitError = (r: ReturnType<typeof pRequest>) =>
  pSelect(r)
    ? null
    : pSelect({ system: null, summary: null, history: [], prompt: r.prompt })
      ? `指令或摘要太长：约 ${(r.system ?? 0) + (r.summary ?? 0)} token，加上这条消息后放不进当前模型的 ${MOCK_WINDOW} token 上下文。请缩短指令，或切换上下文更大的模型。`
      : `这条消息太长：约 ${r.prompt} token，当前模型的上下文只有 ${MOCK_WINDOW} token，还要给回复留出空间。请缩短后再发送。`;
const pAncestors = (s: PSession, id: number | null) => {
  const out: PNode[] = [];
  let cur = s.nodes.find((n) => n.id === id);
  while (cur) {
    out.unshift(cur);
    const c: PNode = cur;
    cur = s.nodes.find((n) => n.id === c.parent);
  }
  return out;
};
const pPlan = (s: PSession) => {
  const shown = pPath(s);
  const done = (m: PNode) => m.state === "complete" && m.content.trim() !== "";
  const completed = shown.filter(done);
  const keep = completed.length > 6 ? 4 : completed.length > 2 ? 2 : 0;
  const boundary = completed.slice(0, completed.length - keep).reverse().find((m) => m.role === "assistant");
  if (!boundary) throw "还没有可以总结的较早回复，请先完成一轮对话";
  const prefix = shown.slice(0, shown.indexOf(boundary) + 1);
  const prev = pApplicable(s.id, prefix.map((m) => m.id));
  const source = prefix.slice(prev ? prev.i + 1 : 0).filter(done).map((m) => (m.role === "user" ? "用户：\n" : "助手：\n") + m.content).join("\n\n");
  return { through: boundary.id, source, previous: prev?.x.content ?? null };
};
/** library.rs: which list a session shows in. */
const pListOf = (s: PSession) =>
  s.archived ? "archived" : s.pinned ? "pinned" : s.projectId != null && pky.projects.some((p) => p.id === s.projectId) ? `project:${s.projectId}` : "recent";
const pRow = (s: PSession) => ({ id: s.id, title: s.title, projectId: s.projectId, pinned: s.pinned, archived: s.archived, updatedAt: s.updatedAt });
const pMembers = (list: string) => {
  const ms = pky.sessions.filter((x) => pListOf(x) === list).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt) || b.id - a.id);
  if (pky.manual.has(list)) ms.sort((a, b) => (a.sortOrder == null ? 0 : 1) - (b.sortOrder == null ? 0 : 1) || (a.sortOrder ?? 0) - (b.sortOrder ?? 0));
  return ms;
};
const pRegroup = (s: PSession, change: (x: PSession) => void) => {
  const before = pListOf(s);
  change(s);
  if (pListOf(s) !== before) s.sortOrder = null;
};
const pFind = (id: unknown) => {
  const s = pky.sessions.find((x) => x.id === id);
  if (!s) throw "找不到这个对话，可能已经被删除了";
  return s;
};
const pSnippet = (content: string, needle: string) => {
  const flat = content.split(/\s+/).join(" ");
  const at = Math.max(0, flat.toLowerCase().indexOf(needle));
  const start = Math.max(0, at - 28);
  const end = Math.min(flat.length, at + needle.length + 28);
  return (start > 0 ? "…" : "") + flat.slice(start, end) + (end < flat.length ? "…" : "");
};
const pDoc = (s: PSession) =>
  [`# ${s.title}`, ...pPath(s).map((m) => [`## ${m.role === "user" ? "我" : "小拾"}`, m.content].join("\n\n"))].join("\n\n") + "\n";
/** `&picky=ready`: a few finished conversations and a project to organise. */
const pSeed = () => {
  const done = (s: PSession, q: string, a: string, model = "Qwen3.5 9B") => {
    const path = pPath(s);
    const u = pAdd(s, "user", q, path[path.length - 1]?.id ?? null, null);
    const r = pAdd(s, "assistant", a, u.id, model);
    r.state = "complete";
    pShow(s, r);
  };
  const proj = { id: nextId++, name: "毕业论文", instructions: "用中文回答，引用注明出处。", sortOrder: 0 };
  pky.projects.push(proj);
  const a = pNewSession("向量数据库怎么选");
  done(a, "向量数据库怎么选", "可以先看看 **SQLite 的向量扩展**，数据量小的时候够用。");
  const b = pNewSession("周末去哪", proj.id);
  b.projectId = null;
  done(b, "周末去哪", "去公园走走，顺便想想向量检索的论文。");
  const c = pNewSession("论文提纲", proj.id);
  done(c, "论文提纲", "1. 引言\n2. 相关工作\n3. 方法");
  [a, b, c].forEach((x, i) => (x.updatedAt = new Date(Date.now() - (3 - i) * 3600000).toISOString()));
  pky.sessions.push(a, b, c);
  const tpl = (title: string, category: string, body: string, pinned: boolean) =>
    pky.templates.push({ id: nextId++, title, category, body, pinnedAt: pinned ? new Date(Date.now() - (10 - pky.templates.length) * 1000).toISOString() : null, lastUsedAt: null });
  tpl("润色中文", "写作", "请润色下面这段中文：保持原意，让语气更自然，少用套话。\n\n{{原文}}", true);
  tpl("解释代码", "代码", "说明这段代码做了什么，并指出潜在问题。\n\n```\n{{代码}}\n```", true);
  tpl("总结长文", "阅读", "用要点总结下面的资料，再列出三个值得追问的问题。\n\n{{资料}}", true);
  tpl("对比两个方案", "思考", "比较 {{方案 A}} 和 {{方案 B}}，用表格列出优缺点，最后给出建议。", true);
  tpl("改写成正式邮件", "写作", "把下面的内容改写成一封正式的邮件，收件人是 {{收件人}}。\n\n{{内容}}", false);
  pky.presets.push({ id: nextId++, name: "老师", content: "像耐心的老师一样一步一步解释，多举例子。" }, { id: nextId++, name: "翻译", content: "把我发的内容翻译成英文，只输出译文。" });
  pky.global = "回复用中文，先给结论再解释。";
  pky.memories.push(
    { id: nextId++, content: "我住在新加坡，平时说中文和英文。", projectId: null, sharedProjectIds: [], enabled: true, localOnly: false, source: "manual", sourceSessionId: null, notedOn: "2026-09-20", createdAt: "2026-09-20T02:00:00.000Z", updatedAt: "2026-09-20T02:00:00.000Z" },
    { id: nextId++, content: "论文引用用 APA 格式。", projectId: proj.id, sharedProjectIds: [], enabled: true, localOnly: false, source: "command", sourceSessionId: null, notedOn: "2026-10-02", createdAt: "2026-10-02T02:00:00.000Z", updatedAt: "2026-10-02T02:00:00.000Z" },
    { id: nextId++, content: "月薪的具体数字只给本地模型看。", projectId: null, sharedProjectIds: [], enabled: true, localOnly: true, source: "manual", sourceSessionId: null, notedOn: "2026-10-05", createdAt: "2026-10-05T02:00:00.000Z", updatedAt: "2026-10-08T02:00:00.000Z" },
  );
  const report = pickyHandlers.picky_import_file({ path: "/mock/开题报告.pdf" }) as { id: number };
  const fid = pickyHandlers.picky_project_file_add({ projectId: proj.id, artifactId: report.id }) as number;
  pky.files.find((f) => f.id === fid)!.indexed = true;
  pky.built.add(report.id);
  const old = pNewSession("新对话");
  old.updatedAt = new Date(Date.now() - 40 * 86400000).toISOString();
  pky.sessions.push(old);
};

const pTitle = (prompt: string) => {
  const line = prompt.split("\n").map((l) => l.trim()).find(Boolean) ?? "新对话";
  return [...line].length > 40 ? [...line].slice(0, 40).join("") + "…" : line;
};
const pPath = (s: PSession) => {
  const out: PNode[] = [];
  const byId = new Map(s.nodes.map((n) => [n.id, n]));
  let cur = byId.get(s.root ?? -1) ?? [...s.nodes].reverse().find((n) => n.parent === null);
  while (cur && !out.includes(cur)) {
    out.push(cur);
    const c: PNode = cur;
    cur = byId.get(c.activeChild ?? -1) ?? [...s.nodes].reverse().find((n) => n.parent === c.id);
  }
  return out;
};
const pSiblings = (s: PSession, m: PNode) => s.nodes.filter((n) => n.parent === m.parent && n.role === m.role);
const pShow = (s: PSession, m: PNode) => {
  let cur: PNode | undefined = m;
  while (cur) {
    const c: PNode = cur;
    const p = s.nodes.find((n) => n.id === c.parent);
    if (p) p.activeChild = c.id;
    else s.root = c.id;
    cur = p;
  }
};
const pAdd = (s: PSession, role: "user" | "assistant", content: string, parent: number | null, model: string | null): PNode => {
  const n: PNode = { id: nextId++, role, content, reasoning: "", reasoningMs: null, state: role === "user" ? "complete" : "streaming", createdAt: now(), model, error: null, parent, activeChild: null };
  s.nodes.push(n);
  s.updatedAt = now();
  return n;
};
const pTarget = () => {
  if (pky.run) throw "正在生成回复，等它写完或先停止。";
  if (pky.selection.startsWith("cloud:")) {
    const m = pky.cloud.find((x) => `cloud:${x.id}` === pky.selection)!;
    if (!pky.keys.has(m.service)) throw "尚未保存这个云端接口的 API Key。";
    return { modelName: m.name, thinking: false };
  }
  if (!pky.active || pky.readiness !== "ready" || pky.op) throw pky.op || pky.readiness === "loading" ? "模型还在加载，等它就绪再发送。" : "本地模型还没有运行。";
  return { modelName: pky.active.name, thinking: pky.active.thinking };
};
const pRun = (s: PSession, user: PNode, msg: PNode, thinking: boolean, prompt: string, a: Args, fitError: string | null = null, material: { label: string; content: string }[] = []) => {
  const ch = a.onEvent as { onmessage: (e: unknown) => void };
  const emit: Emit = (e) => ch.onmessage(e);
  const runId = nextId++;
  const done = (state: string, error: string | null) => {
    if (!pky.run || pky.run.id !== runId) return;
    clearTimeout(pky.run.timer);
    msg.state = state === "completed" ? "complete" : state;
    msg.error = error;
    pky.run = null;
    emit({ kind: "done", state, error, reasoningMs: msg.reasoningMs });
  };
  pky.run = { id: runId, session: s.id, kind: "reply", msg, timer: 0, done };
  const size = material.reduce((n, m) => n + pEstimate(m.content), 0);
  if (fitError) pky.run.timer = window.setTimeout(() => done("failed", fitError), 300);
  else if (size > 5000) {
    // reading in segments: progress, notes, then the reply (thinking off)
    const notes: { label: string; content: string }[] = [];
    pky.notes.set(msg.id, notes);
    let i = 0;
    const step = () => {
      if (!pky.run || pky.run.id !== runId) return;
      if (i >= material.length) {
        pky.run.reading = null;
        emit({ kind: "reading", current: 0, total: 0, label: "" });
        pky.run.timer = pStream(msg, false, prompt, emit, done);
        return;
      }
      const m = material[i++];
      pky.run.reading = { current: i, total: material.length, label: `正在阅读 ${m.label}` };
      emit({ kind: "reading", ...pky.run.reading });
      const note = { label: m.label, content: `模拟要点：这一段约 ${[...m.content].length} 字。` };
      notes.push(note);
      emit({ kind: "note", ...note });
      pky.run.timer = window.setTimeout(step, 400);
    };
    pky.run.timer = window.setTimeout(step, 400);
  } else if (pMockCalls(prompt).length) {
    // lookups first, one by one, then the answer from them
    const calls = pMockCalls(prompt);
    const qs: PQuery[] = [];
    pky.queries.set(msg.id, qs);
    const next = () => {
      if (!pky.run || pky.run.id !== runId) return;
      const call = calls[qs.length];
      if (!call) return void (pky.run.timer = pStream(msg, thinking, prompt, emit, done, pAnswer(qs)));
      const q = pLookup(call[0], call[1]);
      qs.push(q);
      emit({ kind: "query", ...q });
      pky.run.timer = window.setTimeout(next, 350);
    };
    pky.run.timer = window.setTimeout(next, 450);
  } else pky.run.timer = pStream(msg, thinking, prompt, emit, done);
  return { sessionId: s.id, runId, userMessageId: user.id, messageId: msg.id };
};

const pickyHandlers: Record<string, (a: Args) => unknown> = {
  picky_info: () => ({ mode: "mock", dbPath: "/mock/Pomelo OS Dev/picky.db", filesDir: "/mock/Pomelo OS Dev/picky-files", schemaVersion: 6, error: null }),
  picky_storage: () => {
    const files = pky.artifacts
      .filter((x) => !x.deleted)
      .map((x) => ({
        ...pInfo(x),
        bytes: x.byteCount,
        messages: [...pky.used.values()].filter((u) => u.attachments.includes(x.id) || u.files.some((f) => f.artifact === x.id)).length,
        projectFiles: pky.files.filter((f) => f.versions.some((v) => v.artifact === x.id)).length,
      }))
      .sort((p, q) => q.bytes - p.bytes || p.id - q.id);
    const filesBytes = pky.artifacts.filter((x) => !x.deleted).reduce((n, x) => n + x.byteCount, 0) + pky.strays.reduce((n, x) => n + x.bytes, 0);
    return { dbBytes: 1200000 + pky.sessions.length * 4000, filesBytes, files, strays: pky.strays, emptySessions: pky.sessions.filter(pOldEmpty).length };
  },
  picky_storage_delete_file: (a) => {
    if (pky.run) throw "正在生成回复，等它写完再删。";
    const x = pArt(a.id as number);
    x.deleted = true;
    pky.built.delete(x.id);
  },
  picky_storage_delete_stray: (a) => {
    pky.strays = pky.strays.filter((x) => x.name !== a.name);
  },
  picky_storage_clean_empty: () => {
    const n = pky.sessions.filter(pOldEmpty).length;
    pky.sessions = pky.sessions.filter((x) => !pOldEmpty(x));
    return n;
  },
  picky_status: () => pStatus(),
  picky_select: (a) => {
    if (pky.run) throw "正在生成回复，等它写完再切换。";
    const c = a.choice as string;
    if (c.startsWith("local:")) {
      pky.selection = "local";
      if (pky.active?.key !== c.slice(6)) pActivate(c.slice(6));
    } else pky.selection = c;
    return null;
  },
  picky_model_stop: () => {
    if (pky.run) throw "正在生成回复，等它写完再停止模型。";
    pky.op = { kind: "stopping", model: pky.active };
    setTimeout(() => ((pky.active = null), (pky.readiness = "idle"), (pky.op = null)), 500);
    return null;
  },
  picky_refresh: () => null,
  picky_embedding: (a) => {
    pky.emb.working = true;
    setTimeout(() => ((pky.emb.working = false), (pky.emb.state = a.action === "start" ? "running" : "stopped")), 600);
    return null;
  },
  picky_issue_action: (a) => {
    const act = a.action as { kind: string; key?: string };
    if (act.kind === "retryStart") pActivate(act.key!);
    else if (act.kind.startsWith("open")) throw "模拟模式下没有这个文件";
    return null;
  },
  picky_issue_dismiss: () => ((pky.issue = null), null),
  picky_cloud_model_save: (a) => {
    const m = { ...(a.model as Record<string, unknown>) } as { id: string; name: string; apiModel: string; service: string; contextWindow: number; maxOutputTokens: number };
    m.name = m.name.trim();
    m.apiModel = m.apiModel.trim();
    if (!m.name || !m.apiModel || /\s/.test(m.apiModel) || m.contextWindow < 2048 || m.maxOutputTokens < 16 || m.maxOutputTokens >= m.contextWindow) throw "请检查模型名称、接口模型名和令牌上限。";
    if (!m.id) m.id = `m-${Math.random().toString(16).slice(2, 14)}`;
    const i = pky.cloud.findIndex((x) => x.id === m.id);
    if (i >= 0) pky.cloud[i] = m;
    else pky.cloud.push(m);
    return m;
  },
  picky_cloud_model_delete: (a) => {
    pky.cloud.splice(pky.cloud.findIndex((x) => x.id === a.id), 1);
    if (pky.selection === `cloud:${a.id}`) pky.selection = "local";
    return null;
  },
  picky_cloud_key_save: (a) => {
    const k = String(a.key).trim();
    if (!k || /[^\x21-\x7e]/.test(k)) throw "API Key 为空或格式无效。";
    pky.keys.add(a.service as string);
    return null;
  },
  picky_cloud_key_remove: (a) => (pky.keys.delete(a.service as string), null),
  picky_sessions: () => [...pky.sessions].sort((x, y) => y.updatedAt.localeCompare(x.updatedAt) || y.id - x.id).map(({ id, title, updatedAt }) => ({ id, title, updatedAt })),
  picky_messages: (a) => {
    const s = pky.sessions.find((x) => x.id === a.sessionId);
    if (!s) throw "找不到这个对话，可能已经被删除了";
    return pPath(s).map((m) => {
      const sib = pSiblings(s, m);
      const { parent: _p, activeChild: _c, ...rest } = m;
      return { ...rest, versionIndex: sib.indexOf(m) + 1, versionCount: sib.length, files: m.role === "user" ? pMessageFiles(m) : [], notes: pky.notes.get(m.id) ?? [], memories: pky.runMemories.get(m.id) ?? [], queries: pky.queries.get(m.id) ?? [] };
    });
  },
  picky_send: (a) => {
    const attachments = (a.attachments as number[] | undefined) ?? [];
    const prompt = String(a.text).trim() || (attachments.length ? "请总结这些附件。" : "");
    if (!prompt) throw "问题不能为空";
    attachments.forEach(pArt);
    const { modelName, thinking } = pTarget();
    let s = pky.sessions.find((x) => x.id === a.sessionId);
    if (!s) {
      s = pNewSession(pTitle(prompt), (a.projectId as number | null) ?? null);
      const d = a.instructions as PSetting | null;
      if (d) Object.assign(s, { instructions: d.content.trim(), presetId: pky.presets.some((x) => x.id === d.presetId) ? d.presetId : null });
      if (a.memoryOff) pky.memoryOffSessions.add(s.id);
      pky.sessions.push(s);
    }
    if (s.archived) pRegroup(s, (x) => (x.archived = false));
    const path = pPath(s);
    const fit = pFitError(pRequest(s, path, prompt));
    const leaf = path[path.length - 1] ?? null;
    const user = pAdd(s, "user", prompt, leaf?.id ?? null, null);
    const used: PUsed = {
      attachments,
      files: ((a.files as number[] | undefined) ?? []).map((id) => {
        const f = pky.files.find((x) => x.id === id)!;
        return { fileId: id, version: f.currentVersion, artifact: pCurrent(f)!.id };
      }),
    };
    pky.used.set(user.id, used);
    const msg = pAdd(s, "assistant", "", user.id, modelName);
    pky.runMemories.set(msg.id, (a.memories as never[]) ?? []);
    pShow(s, msg);
    return pRun(s, user, msg, thinking, prompt, a, fit, pMaterial(s, path, used));
  },
  picky_edit: (a) => {
    const text = String(a.text).trim();
    if (!text) throw "问题不能为空";
    const { modelName, thinking } = pTarget();
    const s = pky.sessions.find((x) => x.nodes.some((n) => n.id === a.messageId))!;
    const src = s.nodes.find((n) => n.id === a.messageId)!;
    const fit = pFitError(pRequest(s, pAncestors(s, src.parent), text));
    const user = pAdd(s, "user", text, src.parent, null);
    const used = pky.used.get(src.id) ?? { attachments: [], files: [] };
    pky.used.set(user.id, used);
    const msg = pAdd(s, "assistant", "", user.id, modelName);
    pky.runMemories.set(msg.id, (a.memories as never[]) ?? []);
    pShow(s, msg);
    if (src.parent === null && s.titleSource === "auto") s.title = pTitle(text);
    return pRun(s, user, msg, thinking, text, a, fit, pMaterial(s, pAncestors(s, src.parent), used));
  },
  picky_regenerate: (a) => {
    const { modelName, thinking } = pTarget();
    const s = pky.sessions.find((x) => x.nodes.some((n) => n.id === a.messageId))!;
    const src = s.nodes.find((n) => n.id === a.messageId)!;
    const q = s.nodes.find((n) => n.id === src.parent)!;
    const fit = pFitError(pRequest(s, pAncestors(s, q.parent), q.content));
    const msg = pAdd(s, "assistant", "", q.id, modelName);
    pky.runMemories.set(msg.id, (a.memories as never[]) ?? []);
    pShow(s, msg);
    return pRun(s, q, msg, thinking, q.content, a, fit, pMaterial(s, pAncestors(s, q.parent), pky.used.get(q.id) ?? { attachments: [], files: [] }));
  },
  picky_switch_version: (a) => {
    if (pky.run) throw "正在生成，等它写完再切换版本。";
    const s = pky.sessions.find((x) => x.nodes.some((n) => n.id === a.messageId))!;
    const m = s.nodes.find((n) => n.id === a.messageId)!;
    const sib = pSiblings(s, m);
    const t = sib[sib.indexOf(m) + (a.delta as number)];
    if (t) pShow(s, t);
    return (t ?? m).id;
  },
  picky_library: () => ({
    pinned: pMembers("pinned").map(pRow),
    projects: [...pky.projects]
      .sort((a, b) => a.sortOrder - b.sortOrder || a.id - b.id)
      .map((p) => ({ ...p, includeGlobalMemories: p.includeGlobalMemories ?? true, sessions: pMembers(`project:${p.id}`).map(pRow), files: pky.files.filter((f) => f.projectId === p.id).length })),
    recent: pMembers("recent").map(pRow),
    archived: pMembers("archived").map(pRow),
  }),
  picky_rename: (a) => {
    const s = pFind(a.id);
    const t = String(a.title).trim();
    if (t) Object.assign(s, { title: t.slice(0, 120), titleSource: "user" });
    else Object.assign(s, { title: pTitle(pPath(s).find((m) => m.role === "user")?.content ?? ""), titleSource: "auto" });
    return null;
  },
  picky_pin: (a) => ((a.ids as number[]).forEach((id) => pRegroup(pFind(id), (x) => (x.pinned = !!a.pinned))), null),
  picky_archive: (a) => ((a.ids as number[]).forEach((id) => pRegroup(pFind(id), (x) => (x.archived = !!a.archived))), null),
  picky_move: (a) => ((a.ids as number[]).forEach((id) => pRegroup(pFind(id), (x) => (x.projectId = (a.projectId as number | null) ?? null))), null),
  picky_delete: (a) => {
    if (pky.run && (a.ids as number[]).includes(pky.run.session)) throw "这个对话正在写回复，等它写完再删除。";
    pky.sessions = pky.sessions.filter((x) => !(a.ids as number[]).includes(x.id));
    return null;
  },
  picky_project_create: (a) => {
    const id = nextId++;
    pky.projects.push({ id, name: String(a.name).trim().slice(0, 60) || "未命名项目", instructions: String(a.instructions).trim(), sortOrder: Math.min(0, ...pky.projects.map((p) => p.sortOrder)) - 1, includeGlobalMemories: a.includeGlobalMemories !== false });
    return id;
  },
  picky_project_update: (a) => {
    const p = pky.projects.find((x) => x.id === a.id)!;
    Object.assign(p, { name: String(a.name).trim().slice(0, 60) || "未命名项目", instructions: String(a.instructions).trim(), includeGlobalMemories: a.includeGlobalMemories !== false });
    return null;
  },
  picky_project_delete: (a) => {
    pky.sessions.forEach((s) => s.projectId === a.id && pRegroup(s, (x) => (x.projectId = null)));
    pky.memories.forEach((m) => {
      m.sharedProjectIds = m.sharedProjectIds.filter((x) => x !== a.id);
      if (m.projectId === a.id) Object.assign(m, { projectId: null, enabled: false, sharedProjectIds: [], updatedAt: now() });
    });
    pky.projects = pky.projects.filter((p) => p.id !== a.id);
    return null;
  },
  picky_projects_reorder: (a) => ((a.ids as number[]).forEach((id, i) => (pky.projects.find((p) => p.id === id)!.sortOrder = i)), null),
  picky_drop: (a) => {
    const s = pFind(a.id);
    const list = a.list as string;
    const members = pMembers(list).map((x) => x.id);
    const target = (a.before ?? a.after) as number | null;
    if (target != null && !members.includes(target)) throw "对话列表已经变了，请重新拖一次";
    if (target === s.id) return false;
    const order = members.filter((x) => x !== s.id);
    const at = a.before != null ? order.indexOf(a.before as number) : a.after != null ? order.indexOf(a.after as number) + 1 : 0;
    order.splice(Math.max(0, at), 0, s.id);
    if (pListOf(s) === list && order.join() === members.join()) return false;
    pRegroup(s, (x) => {
      if (list === "pinned") Object.assign(x, { pinned: true, archived: false });
      else if (list === "recent") Object.assign(x, { pinned: false, archived: false, projectId: null });
      else if (list === "archived") x.archived = true;
      else Object.assign(x, { pinned: false, archived: false, projectId: Number(list.slice(8)) });
    });
    order.forEach((id, i) => (pFind(id).sortOrder = i));
    pky.manual.add(list);
    return true;
  },
  picky_search: (a) => {
    const words = String(a.q).toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    const has = (t: string) => words.every((w) => t.toLowerCase().includes(w));
    const out: unknown[] = [];
    for (const s of [...pky.sessions].sort((x, y) => y.updatedAt.localeCompare(x.updatedAt))) {
      if (s.archived && !a.archived) continue;
      const msgs = s.nodes
        .filter((m) => m.state !== "streaming" && has(m.content) && (!a.model || m.model === a.model || (m.role === "user" && s.nodes.some((r) => r.parent === m.id && r.model === a.model))) && (!a.since || m.createdAt >= (a.since as string)))
        .sort((x, y) => y.createdAt.localeCompare(x.createdAt));
      const m = msgs[0];
      if (!m && !has(s.title)) continue;
      out.push({ sessionId: s.id, title: s.title, messageId: m?.id ?? null, snippet: m ? pSnippet(m.content, words[0]) : null, at: m?.createdAt ?? s.updatedAt, model: m?.model ?? null, archived: s.archived });
    }
    return out;
  },
  picky_search_models: () => [...new Set(pky.sessions.flatMap((s) => s.nodes.map((n) => n.model).filter(Boolean)))],
  picky_reveal: (a) => {
    const s = pky.sessions.find((x) => x.nodes.some((n) => n.id === a.messageId))!;
    pShow(s, s.nodes.find((n) => n.id === a.messageId)!);
    return s.id;
  },
  picky_copy_conversation: (a) => (void navigator.clipboard?.writeText(pDoc(pFind(a.id))).catch(() => {}), null),
  picky_export_markdown: () => null,
  picky_export_package: (a) => a.path,
  picky_ui_get: (a) => pky.ui.get(a.key as string) ?? null,
  picky_ui_set: (a) => (pky.ui.set(a.key as string, a.value), null),
  copy_text: (a) => (void navigator.clipboard?.writeText(String(a.text)).catch(() => {}), null),
  picky_templates: () => pTemplates(),
  picky_template_save: (a) => {
    const t = a.template as { title: string; category: string; body: string };
    const title = t.title.split(/\s+/).filter(Boolean).join(" ") || "未命名模板";
    const fields = { title, category: t.category.split(/\s+/).filter(Boolean).join(" "), body: t.body.trimEnd() };
    if (a.id == null) {
      const id = nextId++;
      pky.templates.push({ id, ...fields, pinnedAt: null, lastUsedAt: null });
      return id;
    }
    const x = pky.templates.find((y) => y.id === a.id);
    if (!x) throw "找不到这个模板，可能已经被删除了";
    Object.assign(x, fields);
    return x.id;
  },
  picky_template_pin: (a) => {
    const x = pky.templates.find((y) => y.id === a.id)!;
    if (a.pinned && !x.pinnedAt) x.pinnedAt = now();
    if (!a.pinned) x.pinnedAt = null;
    return null;
  },
  picky_template_delete: (a) => ((pky.templates = pky.templates.filter((y) => y.id !== a.id)), null),
  picky_template_used: (a) => {
    const x = pky.templates.find((y) => y.id === a.id);
    if (x) x.lastUsedAt = now();
    return null;
  },
  picky_instructions: (a) => pPreview((a.sessionId as number | null) ?? null, (a.projectId as number | null) ?? null, (a.draft as PSetting | null) ?? null),
  picky_session_instructions_save: (a) => {
    const st = a.setting as PSetting;
    Object.assign(pFind(a.sessionId), { instructions: st.content.trim(), presetId: st.presetId });
    return null;
  },
  picky_global_instructions: () => pky.global,
  picky_global_instructions_save: (a) => ((pky.global = String(a.content).trim()), null),
  picky_presets: () => pky.presets,
  picky_preset_save: (a) => {
    const name = String(a.name).split(/\s+/).filter(Boolean).join(" ");
    if (!name) throw "预设要有名称";
    if (a.id == null) {
      const id = nextId++;
      pky.presets.push({ id, name, content: String(a.content).trim() });
      return id;
    }
    Object.assign(pky.presets.find((x) => x.id === a.id)!, { name, content: String(a.content).trim() });
    return a.id;
  },
  picky_preset_delete: (a) => {
    pky.presets = pky.presets.filter((x) => x.id !== a.id);
    pky.sessions.forEach((x) => x.presetId === a.id && (x.presetId = null));
    return null;
  },
  picky_context: (a) => {
    if (pky.run) return null;
    try {
      pTarget();
    } catch {
      return null;
    }
    const sess = pFind(a.sessionId);
    const shown = pPath(sess);
    const r = pRequest(sess, shown, String(a.prompt).trim());
    const used = 32 + r.prompt + 8 + (r.system != null ? r.system + 8 : 0) + (r.summary != null ? r.summary + 8 : 0) + r.history.reduce((n, t) => n + t + 8, 0);
    const sel = pSelect(r);
    const percent = Math.round((used / Math.max(1, MOCK_WINDOW - pReserve(MOCK_WINDOW))) * 100);
    const dropped = sel ? sel.dropped : r.history.length;
    return { percent, nearLimit: percent >= 85 || dropped > 0 || !sel, dropped, fits: !!sel, summarizable: shown.some((m) => m.role === "assistant" && m.state === "complete" && m.content.trim()) };
  },
  picky_summarize: (a) => {
    pTarget();
    const sess = pFind(a.sessionId);
    const plan = pPlan(sess);
    const ch = a.onEvent as { onmessage: (e: unknown) => void };
    const runId = nextId++;
    const done = (state: string, error: string | null) => {
      if (!pky.run || pky.run.id !== runId) return;
      clearTimeout(pky.run.timer);
      pky.run = null;
      if (state === "completed") {
        const start = plan.source.replace(/\s/g, "").split("：").slice(1).join("：").slice(0, 12);
        pky.summaries.push({ id: nextId++, sessionId: sess.id, through: plan.through, content: `模拟摘要：较早的对话约 ${[...plan.source].length} 字，从「${start}」聊起。` });
      }
      ch.onmessage({ kind: "done", state, error });
    };
    pky.run = { id: runId, session: sess.id, kind: "summary", msg: null, timer: window.setTimeout(() => done("completed", null), 1500), done };
    return runId;
  },
  picky_import_file: (a) => {
    const path = String(a.path);
    const name = path.split("/").pop() ?? "文件";
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    const format = ext === "pdf" ? "PDF" : ext === "md" || ext === "markdown" ? "Markdown" : ext === "txt" || ext === "text" ? "文本" : null;
    if (!format) throw "只支持文本、Markdown 和 PDF 文件";
    const same = pky.artifacts.find((x) => x.path === path && !x.deleted);
    if (same) return pInfo(same);
    const pages = format === "PDF" ? 12 : 2;
    const segments = Array.from({ length: pages }, (_, i) => `${name} 第 ${i + 1} 部分。` + "这里是用来模拟的正文，讲向量检索、资料整理和论文写作。".repeat(format === "PDF" ? 30 : 4));
    const art: PArtifact = { id: nextId++, name, format, pageCount: format === "PDF" ? pages : null, byteCount: segments.join("").length * 3, deleted: false, path, segments };
    pky.artifacts.push(art);
    return pInfo(art);
  },
  picky_attachment_library: () =>
    [...pky.artifacts]
      .reverse()
      .filter((x) => !x.deleted && ([...pky.used.values()].some((u) => u.attachments.includes(x.id)) || !pky.files.some((f) => f.versions.some((v) => v.artifact === x.id))))
      .map(pInfo),
  picky_open_file: () => null,
  picky_project_files: (a) => pky.files.filter((f) => f.projectId === a.projectId).map(pFileView),
  picky_project_file_add: (a) => {
    const art = pArt(a.artifactId as number);
    const dup = pky.files.find((f) => f.projectId === a.projectId && f.versions.some((v) => v.artifact === art.id));
    if (dup) throw `项目里已有这份资料：${pCurrent(dup)?.name}`;
    const id = nextId++;
    pky.files.push({ id, projectId: a.projectId as number, currentVersion: 1, indexed: false, versions: [{ version: 1, artifact: art.id, addedAt: now() }] });
    return id;
  },
  picky_project_file_replace: (a) => {
    const f = pky.files.find((x) => x.id === a.fileId)!;
    const art = pArt(a.artifactId as number);
    if (pCurrent(f)?.id === art.id) throw "选的文件和现在的版本一样";
    const next = Math.max(...f.versions.map((v) => v.version)) + 1;
    f.versions.push({ version: next, artifact: art.id, addedAt: now() });
    f.currentVersion = next;
    pky.jobs.delete(f.id);
    if (f.indexed) pBuild(f);
    return null;
  },
  picky_project_file_remove: (a) => {
    pky.files = pky.files.filter((x) => x.id !== a.fileId);
    pky.jobs.delete(a.fileId as number);
    return null;
  },
  picky_project_file_index: (a) => {
    const f = pky.files.find((x) => x.id === a.fileId)!;
    f.indexed = !!a.indexed;
    pky.jobs.delete(f.id);
    if (f.indexed) pBuild(f);
    else pky.built.delete(pCurrent(f)?.id ?? -1);
    return null;
  },
  picky_index_rebuild: (a) => (pBuild(pky.files.find((x) => x.id === a.fileId)!), null),
  picky_index_stop: (a) => (pky.jobs.delete(a.fileId as number), null),
  picky_retrieval_plan: (a) => {
    const proj = a.sessionId != null ? pFind(a.sessionId).projectId : ((a.projectId as number | null) ?? null);
    const fs_ = pky.files.filter((f) => f.projectId === proj && f.indexed && !(a.excludeFiles as number[]).includes(f.id));
    const ready = fs_.filter((f) => pky.built.has(pCurrent(f)?.id ?? -1));
    return { readyNames: ready.map((f) => pCurrent(f)!.name), pending: fs_.length - ready.length };
  },
  picky_retrieve: async (a) => {
    const plan = pickyHandlers.picky_retrieval_plan(a) as { readyNames: string[] };
    if (!plan.readyNames.length) return [];
    await new Promise((r) => setTimeout(r, 600));
    if (String(a.question).includes("向量失败测试")) throw "模拟：8081 向量服务没有就绪。";
    const proj = a.sessionId != null ? pFind(a.sessionId).projectId : ((a.projectId as number | null) ?? null);
    return pky.files
      .filter((f) => f.projectId === proj && f.indexed && pky.built.has(pCurrent(f)?.id ?? -1) && !(a.excludeFiles as number[]).includes(f.id))
      .slice(0, 2)
      .map((f) => {
        const art = pCurrent(f)!;
        return { fileId: f.id, artifactId: art.id, segmentOrdinal: 0, label: `${art.name} · 第 1 段`, content: art.segments[0].slice(0, 80) };
      });
  },
  picky_memories: () => [...pky.memories].reverse(),
  picky_memory_save: (a) => {
    const m = a.memory as { content: string; projectId: number | null; sharedProjectIds: number[]; enabled: boolean; localOnly: boolean };
    const content = pMemoryValid(m.content);
    const shared = m.projectId == null ? [] : [...new Set(m.sharedProjectIds.filter((x) => x !== m.projectId))];
    if (a.id == null) {
      const id = nextId++;
      pky.memories.push({ id, content, projectId: m.projectId, sharedProjectIds: shared, enabled: m.enabled, localOnly: m.localOnly, source: "manual", sourceSessionId: null, notedOn: pToday(), createdAt: now(), updatedAt: now() });
      return id;
    }
    const x = pky.memories.find((y) => y.id === a.id)!;
    if (x.content !== content) x.notedOn = pToday();
    Object.assign(x, { content, projectId: m.projectId, sharedProjectIds: shared, enabled: m.enabled, localOnly: m.localOnly, updatedAt: now() });
    return x.id;
  },
  picky_remember: (a) => {
    const contents = (a.entries as string[]).map(pMemoryValid);
    for (const content of contents)
      pky.memories.push({ id: nextId++, content, projectId: (a.projectId as number | null) ?? null, sharedProjectIds: [], enabled: true, localOnly: !!a.localOnly, source: "command", sourceSessionId: (a.sessionId as number | null) ?? null, notedOn: pToday(), createdAt: now(), updatedAt: now() });
    return null;
  },
  picky_memory_flag: (a) => {
    const x = pky.memories.find((y) => y.id === a.id)!;
    if (a.flag === "enabled") x.enabled = !!a.on;
    else x.localOnly = !!a.on;
    x.updatedAt = now();
    return null;
  },
  picky_memory_delete: (a) => ((pky.memories = pky.memories.filter((y) => y.id !== a.id)), null),
  picky_memory_enabled: () => pky.memoryOn,
  picky_memory_enabled_set: (a) => ((pky.memoryOn = !!a.on), null),
  picky_data_cloud: () => pDataCloud(),
  picky_data_cloud_set: (a) => {
    if (!P_MODULES.some((m) => m[0] === a.module)) throw `没有这个模块：${a.module}`;
    pky.dataCloud.set(a.module as string, !!a.allowed);
    return null;
  },
  picky_conversation_memory: (a) => (a.off ? pky.memoryOffSessions.add(a.sessionId as number) : pky.memoryOffSessions.delete(a.sessionId as number), null),
  picky_memory_plan: (a) => {
    const { candidates: _c, ...plan } = pMemoryPlan((a.sessionId as number | null) ?? null, (a.projectId as number | null) ?? null, !!a.draftOff, !!a.nextOff);
    return plan;
  },
  picky_choose_memories: async (a) => {
    const plan = pMemoryPlan((a.sessionId as number | null) ?? null, (a.projectId as number | null) ?? null, !!a.draftOff, !!a.nextOff);
    if (plan.off || !plan.count) return [];
    await new Promise((r) => setTimeout(r, 500));
    const q = String(a.question);
    if (q.includes("向量失败测试")) throw "模拟：8081 向量服务没有就绪。";
    // shared characters stand in for similarity
    const score = (t: string) => [...new Set([...t])].filter((ch) => /\p{L}/u.test(ch) && q.includes(ch)).length;
    return plan.candidates
      .map((m) => ({ m, s: score(m.content) }))
      .filter((x) => x.s >= 2)
      .sort((x, y) => y.s - x.s)
      .slice(0, 6)
      .map(({ m }) => ({ memoryId: m.id, content: m.content, notedOn: m.notedOn }));
  },
  picky_stop: (a) => {
    const r = pky.run;
    if (r && r.id === a.runId) r.done("cancelled", null);
    return null;
  },
};

const handlers: Record<string, (a: Args) => unknown> = {
  ...pickyHandlers,
  home_overview: (a) => homeMock((a.hidden as string[]) ?? []),
  search_all: (a) => [
    ...(searchAllMock(String(a.q)) as unknown[]),
    ...(pickyHandlers.picky_search({ q: a.q, model: null, since: null, archived: false }) as { sessionId: number; title: string; messageId: number | null; snippet: string | null; at: string }[])
      .slice(0, 6)
      .map((h) => ({ module: "picky", kind: "chat", id: h.sessionId, title: h.title, detail: h.snippet, parent: h.messageId, day: h.at.slice(0, 10), amount: null, currency: null, flow: null })),
  ],
  app_info: () => ({
    appVersion: "0.1.0-mock",
    isDev: true,
    dataDir: "/mock/Pomelo OS Dev",
    dbPath: "/mock/Pomelo OS Dev/pomelo.db",
    schemaVersion: 21,
    startupError: null,
  }),
  reveal_data_dir: () => null,
  ...todoHandlers,
  ...finHandlers,
  ...jobHandlers,
  ...noteHandlers,
  ...vaultHandlers,
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
    { title: "示例书 A", author: "作者甲", country: "中国", status: "done", rating: 5, finishedYear: "2025" },
    { title: "示例书 B", author: "作者乙", country: "日本", status: "doing", rating: 3, kindId: kindId("books", "非虚构") },
    { title: "示例书 C", author: "作者甲", country: "中国", status: "todo", kindId: kindId("books", "漫画") },
    { title: "示例书 D", author: "作者丙 / 作者丁", country: "法国", note: "一条备注", kindId: null },
  ]),
  ...collection("films", { director: null, kindId: kindId("films", "电影") }, [
    { title: "示例电影", director: "导演甲", country: "美国", status: "done", rating: 5, finishedYear: "2026" },
    { title: "示例剧集", director: "导演乙", country: "韩国", status: "doing", kindId: kindId("films", "剧集") },
    { title: "示例动漫", director: "导演丙", country: "日本", kindId: kindId("films", "动漫") },
  ]),

  drawing_courses: () => [...courses].sort((a, b) => a.sortOrder - b.sortOrder).map(withCounts),
  drawing_courses_reorder: (a) => reorderPart(courses, a.ids as number[]),
  drawing_lessons_reorder: (a) => reorderPart(lessons.filter((l) => l.courseId === a.courseId) as unknown as { id: number; sortOrder: number }[], a.ids as number[]),
  packing_templates_reorder: (a) => reorderPart(tpls, a.ids as number[]),
  packing_items_reorder: (a) => reorderPart((a.list === "template" ? tplItems : tripItems).filter((t) => t.parentId === a.parentId), a.ids as number[]),
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
    const l = applyPatch(addLesson(a.courseId as number, { title: "x", status: "todo" }), a.patch as Args, "课名");
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

  packing_templates: () => [...tpls].sort((a, b) => a.sortOrder - b.sortOrder).map(tplOut),
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
  packing_trip_create: (a) => {
    const from = (a.fromTripId as number | null) ?? null;
    const t = createTripRow(a.patch as Args, from === null ? ((a.templateId as number | null) ?? null) : null);
    if (from !== null) tripItems.filter((i) => i.parentId === from).forEach((i) => addPItem(tripItems, t.id, { kindId: i.kindId, name: i.name, note: i.note, packed: false }));
    return tripOut(t);
  },
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

  "plugin:dialog|open": (a) => {
    const o = (a.options ?? {}) as { multiple?: boolean; filters?: { extensions: string[] }[] };
    if (!o.filters?.some((f) => f.extensions.includes("pdf"))) return "/mock/pomelo-import.json";
    pPickCount += 1;
    return o.multiple ? [`/mock/论文初稿${pPickCount > 1 ? pPickCount : ""}.pdf`, `/mock/读书笔记${pPickCount > 1 ? pPickCount : ""}.md`] : `/mock/提纲-第${pPickCount}版.md`;
  },
  "plugin:dialog|save": () => "/mock/pomelo-export.json",
  export_to: (a) => ({
    path: a.path,
    modules: [
      { key: "books", label: "摸鱼读读", incoming: handlers["books__count"]({}) },
      { key: "films", label: "好奇看看", incoming: handlers["films__count"]({}) },
      { key: "drawing", label: "随意画画", incoming: courses.length },
      { key: "packing", label: "出门走走", incoming: tpls.length + tripRows.length },
      { key: "picky", label: "小拾", incoming: pky.projects.length + pky.presets.length + pky.templates.length + pky.memories.length },
    ],
  }),
  backups_list: () => [...backups].sort((x, y) => y.createdAt.localeCompare(x.createdAt)),
  backup_now: () => addBackup("manual"),
  backup_daily: () => (backups.some((b) => b.label === "daily" && b.fileName.includes(stamp(new Date()).slice(0, 8))) ? null : addBackup("daily")),
  backup_restore: (a) => {
    if (!backups.some((b) => b.fileName === a.fileName)) throw "找不到这份备份，可能已经被删除了";
    return { safety: addBackup("before-restore") };
  },
  reveal_backups_dir: () => null,
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

/** `?mock&notes=prototype`: the converted Flomo notes (prototype/notes-import.json, local only) instead of the samples. */
async function loadPrototypeNotes() {
  if (!new URLSearchParams(location.search).has("notes")) return;
  const r = await fetch("/prototype/notes-import.json").catch(() => null);
  if (!r?.ok) return console.warn("[mock] prototype/notes-import.json not found (pnpm import:flomo)");
  const file = (await r.json()) as { modules: { notes: { notes: { content: string; pinned?: boolean; createdAt: string }[] } } };
  nt.notes = file.modules.notes.notes.map((n) => ({ id: nextId++, content: n.content, pinned: !!n.pinned, refs: [], createdAt: n.createdAt, updatedAt: n.createdAt }));
}

export async function installMockIpc() {
  if (new URLSearchParams(location.search).get("picky") === "ready") pSeed();
  await loadPrototypeNotes();
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
