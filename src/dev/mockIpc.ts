// DEV-ONLY in-memory backend so the UI can be exercised in a plain browser: `pnpm dev` → http://localhost:1420/?mock
// Mirrors the Rust command surface loosely (not the source of truth — Rust rules/tests are).
// Keep in sync when adding commands. Never imported in production builds (guarded in main.tsx).
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { nextOnOrAfter } from "@/modules/finance/money";
import { excerpt, tagSpans } from "@/modules/notes/NoteBody";
import { addDays, parseYmd, toYmd } from "@/shared/dates";

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

// ---- backups (file names mirror backup.rs; restore only records the safety copy) ----
type Backup = { fileName: string; label: string; createdAt: string; size: number };
const stamp = (d: Date) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
};
const mkBackup = (label: string, d = new Date()): Backup => {
  const s = stamp(d);
  return {
    fileName: `pomelo-${s}-${label}.db`,
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
];
const addBackup = (label: string) => {
  const b = mkBackup(label);
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

// ---- 钱来钱往 (finance): mirrors finance.rs loosely (currency per account, posting 固定支出, totals) ----
type Cur = "CNY" | "SGD" | "HKD" | "USD";
type FAccount = { id: number; name: string; kind: string; currency: Cur; closed: boolean; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
type FEntry = { id: number; flow: "out" | "in"; amount: number; currency: Cur; day: string; kindId: number | null; accountId: number | null; fixedId: number | null; note: string | null; createdAt: string; updatedAt: string };
type FFixed = { id: number; name: string; flow: "out" | "in"; amount: number; currency: Cur; kindId: number | null; accountId: number | null; rule: string; nextDay: string; stopped: boolean; note: string | null; sortOrder: number; createdAt: string; updatedAt: string };
let fin = {
  accounts: [] as FAccount[],
  balances: [] as { accountId: number; month: string; amount: number }[],
  rates: [] as { month: string; currency: Cur; rate: number }[],
  holdings: [] as { id: number; accountId: number; month: string; name: string; quantity: number | null; value: number }[],
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
const accountOut = (a: FAccount) => {
  const last = fin.balances.filter((b) => b.accountId === a.id).sort((x, y) => y.month.localeCompare(x.month))[0];
  return { ...a, lastMonth: last?.month ?? null, lastAmount: last?.amount ?? null };
};
const checkMoney = (flow: string, currency: Cur, kindId: number | null, accountId: number | null) => {
  if (kindId !== null && kinds.find((k) => k.id === kindId)?.module !== (flow === "in" ? "fin_in" : "fin_out")) throw "选择的分类不存在，或者不是这一类（支出 / 收入）的";
  if (accountId !== null) {
    const a = finAccount(accountId);
    if (a.currency !== currency) throw `「${a.name}」是 ${a.currency} 账户，币种要一致`;
  }
};
const checkEntry = (e: FEntry) => {
  if (!(e.amount > 0)) throw "金额要大于 0";
  if (!/^\d{4}-\d{2}-\d{2}$/.test(e.day)) throw "日期不对";
  checkMoney(e.flow, e.currency, e.kindId, e.accountId);
};
const newEntry = (p: Partial<FEntry>): FEntry => ({
  id: nextId++, flow: "out", amount: 0, currency: "CNY", day: mockToday(), kindId: null, accountId: null, fixedId: null, note: null, createdAt: now(), updatedAt: now(), ...p,
});
const postDue = (today: string) => {
  let n = 0;
  for (const f of fin.fixed.filter((x) => !x.stopped && x.nextDay <= today)) {
    for (let k = 0; k < 240 && f.nextDay <= today; k++) {
      fin.entries.push(newEntry({ flow: f.flow, amount: f.amount, currency: f.currency, day: f.nextDay, kindId: f.kindId, accountId: f.accountId, fixedId: f.id, note: f.name }));
      f.nextDay = nextOnOrAfter(f.rule, addDays(f.nextDay, 1));
      n++;
    }
  }
  return n;
};
const assetsOut = () => {
  const balances = [...fin.balances].sort((a, b) => a.month.localeCompare(b.month) || a.accountId - b.accountId);
  const rates = [...fin.rates].sort((a, b) => a.month.localeCompare(b.month) || a.currency.localeCompare(b.currency));
  const totals: { month: string; totalCny: number | null; missing: Cur[] }[] = [];
  for (const b of balances) {
    if (totals[totals.length - 1]?.month !== b.month) totals.push({ month: b.month, totalCny: 0, missing: [] });
    const t = totals[totals.length - 1];
    const c = finAccount(b.accountId).currency;
    const r = c === "CNY" ? 1 : rates.find((x) => x.month === b.month && x.currency === c)?.rate;
    if (r === undefined) {
      if (!t.missing.includes(c)) t.missing.push(c);
    } else if (t.totalCny !== null) t.totalCny += Math.round(b.amount * r);
  }
  for (const t of totals) if (t.missing.length) t.totalCny = null;
  return { balances, rates, totals };
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
  const thisM = mockToday().slice(0, 7);
  const ym = (k: number) => toYmd(new Date(Number(thisM.slice(0, 4)), Number(thisM.slice(5)) - 1 + k, 1)).slice(0, 7);
  for (let k = -5; k <= -1; k++) {
    const i = k + 5;
    fin.balances.push(
      { accountId: cmb.id, month: ym(k), amount: 2_850_000 + i * 120_000 },
      { accountId: dbs.id, month: ym(k), amount: 1_240_000 - i * 45_000 + (i % 2) * 90_000 },
      { accountId: ali.id, month: ym(k), amount: 180_000 + i * 15_000 },
      { accountId: ib.id, month: ym(k), amount: 420_000 + i * 18_000 },
    );
    fin.rates.push({ month: ym(k), currency: "SGD", rate: 5.31 + i * 0.02 }, { month: ym(k), currency: "USD", rate: 7.18 - i * 0.01 });
  }
  fin.holdings.push(
    { id: nextId++, accountId: ib.id, month: ym(-1), name: "VOO", quantity: 4, value: 252_000 },
    { id: nextId++, accountId: ib.id, month: ym(-1), name: "AAPL", quantity: 6, value: 140_000 },
    { id: nextId++, accountId: ib.id, month: ym(-1), name: "现金", quantity: null, value: 100_000 },
  );
  const fx = (name: string, amount: number, currency: Cur, kind: number, account: number | null, rule: string) =>
    fin.fixed.push({ id: nextId++, name, flow: "out", amount, currency, kindId: kind, accountId: account, rule, nextDay: nextOnOrAfter(rule, `${ym(-3)}-01`), stopped: false, note: null, sortOrder: fin.fixed.length + 1, createdAt: now(), updatedAt: now() });
  fx("房租", 150_000, "SGD", fk("住房"), dbs.id, "monthly:1");
  fx("话费", 3_000, "CNY", fk("通讯"), cmb.id, "monthly:20");
  fx("iCloud", 299, "USD", fk("订阅"), null, "monthly:5");
  postDue(mockToday());
  for (let k = -3; k <= 0; k++) {
    const day = (d: number) => `${ym(k)}-${String(d).padStart(2, "0")}`;
    const add = (d: number, p: Partial<FEntry>) => day(d) <= mockToday() && fin.entries.push(newEntry({ day: day(d), ...p }));
    add(1, { flow: "in", amount: 200_000, currency: "SGD", kindId: ik("津贴"), accountId: dbs.id, note: "实习津贴" });
    [7, 14, 21, 28].forEach((d, i) => add(d, { amount: 9_000 + i * 2_300 + (k + 3) * 700, currency: "SGD", kindId: fk("餐饮"), accountId: dbs.id, note: "本周小额" }));
    [7, 21].forEach((d) => add(d, { amount: 4_200 + (k + 3) * 300, currency: "SGD", kindId: fk("交通"), accountId: dbs.id, note: "本周小额" }));
    add(12, { amount: 38_900 - (k + 3) * 5_000, currency: "SGD", kindId: fk("购物"), accountId: dbs.id, note: "显示器支架" });
    add(16, { amount: 45_000, currency: "CNY", kindId: fk("餐饮"), accountId: ali.id, note: "请客" });
    if (k === -2) add(3, { flow: "in", amount: 300_000, currency: "CNY", kindId: ik("家里"), accountId: cmb.id });
    if (k === -1) add(9, { amount: 128_000, currency: "SGD", kindId: fk("旅行"), accountId: dbs.id, note: "机票" });
  }
  fin.budgets.push({ kindId: fk("餐饮"), currency: "SGD", amount: 60_000 }, { kindId: fk("交通"), currency: "SGD", amount: 15_000 }, { kindId: fk("购物"), currency: "SGD", amount: 30_000 });
}
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
    const used = fin.balances.some((b) => b.accountId === acc.id) || fin.entries.some((e) => e.accountId === acc.id);
    if (p.currency && p.currency !== acc.currency && used) throw "这个账户已经有记录了，不能改币种；可以新建一个账户";
    Object.assign(acc, p, { name: (p.name ?? acc.name).trim(), updatedAt: now() });
    return accountOut(acc);
  }),
  fin_account_delete: undoable("删除账户", (a) => {
    fin.accounts = fin.accounts.filter((x) => x.id !== a.id);
    fin.balances = fin.balances.filter((b) => b.accountId !== a.id);
    fin.holdings = fin.holdings.filter((h) => h.accountId !== a.id);
    for (const e of [...fin.entries, ...fin.fixed]) if (e.accountId === a.id) e.accountId = null;
    return null;
  }),
  fin_accounts_reorder: undoable("调整顺序", (a) => reorderPart(fin.accounts, a.ids as number[])),
  fin_assets: () => assetsOut(),
  fin_month_save: undoable("更新余额", (a) => {
    const month = String(a.month);
    for (const b of a.balances as { accountId: number; amount: number | null }[]) {
      fin.balances = fin.balances.filter((x) => !(x.accountId === b.accountId && x.month === month));
      if (b.amount !== null) fin.balances.push({ accountId: b.accountId, month, amount: b.amount });
    }
    for (const r of a.rates as { currency: Cur; rate: number | null }[]) {
      if (r.rate !== null && !(r.rate > 0)) throw "汇率要大于 0";
      fin.rates = fin.rates.filter((x) => !(x.month === month && x.currency === r.currency));
      if (r.rate !== null) fin.rates.push({ month, currency: r.currency, rate: r.rate });
    }
    return null;
  }),
  fin_holdings: () => [...fin.holdings].sort((a, b) => a.accountId - b.accountId || a.month.localeCompare(b.month)),
  fin_holdings_save: undoable("更新持仓", (a) => {
    if (finAccount(a.accountId as number).kind !== "broker") throw "只有券商账户能记持仓";
    fin.holdings = fin.holdings.filter((h) => !(h.accountId === a.accountId && h.month === a.month));
    for (const r of a.rows as { name: string; quantity: number | null; value: number }[])
      fin.holdings.push({ id: nextId++, accountId: a.accountId as number, month: String(a.month), name: r.name.trim(), quantity: r.quantity, value: r.value });
    return null;
  }),
  fin_entries: (a) => fin.entries.filter((e) => e.day.startsWith(String(a.month))).sort((x, y) => y.day.localeCompare(x.day) || y.id - x.id),
  fin_entry_create: undoable("记一笔", (a) => {
    const e = newEntry(a.patch as Partial<FEntry>);
    checkEntry(e);
    fin.entries.push(e);
    return e;
  }),
  fin_entries_create: undoable("记一周小额", (a) => {
    const es = (a.patches as Partial<FEntry>[]).map(newEntry);
    es.forEach(checkEntry);
    fin.entries.push(...es);
    return es.length;
  }),
  fin_entry_update: undoable("修改记录", (a) => {
    const e = fin.entries.find((x) => x.id === a.id);
    if (!e) throw "找不到这个记录，可能已经被删除了";
    const next = { ...e, ...(a.patch as Partial<FEntry>), updatedAt: now() };
    checkEntry(next);
    Object.assign(e, next);
    return e;
  }),
  fin_entry_delete: undoable("删除记录", (a) => void (fin.entries = fin.entries.filter((e) => e.id !== a.id))),
  fin_fixed: () => [...fin.fixed].sort((a, b) => a.sortOrder - b.sortOrder),
  fin_fixed_create: undoable("添加固定支出", (a) => {
    const p = a.patch as Partial<FFixed>;
    const name = (p.name ?? "").trim();
    if (!name) throw "名称不能为空";
    if (fin.fixed.some((f) => f.name === name)) throw `已经有叫「${name}」的固定支出了`;
    const f: FFixed = { id: nextId++, flow: "out", amount: 0, currency: "CNY", kindId: null, accountId: null, rule: "monthly:1", stopped: false, note: null, sortOrder: fin.fixed.length + 1, createdAt: now(), updatedAt: now(), ...p, name, nextDay: p.nextDay ?? nextOnOrAfter(p.rule ?? "monthly:1", mockToday()) } as FFixed;
    if (!(f.amount > 0)) throw "金额要大于 0";
    checkMoney(f.flow, f.currency, f.kindId, f.accountId);
    fin.fixed.push(f);
    postDue(mockToday());
    return f;
  }),
  fin_fixed_update: undoable("修改固定支出", (a) => {
    const f = fin.fixed.find((x) => x.id === a.id)!;
    const next = { ...f, ...(a.patch as Partial<FFixed>), updatedAt: now() };
    if (!(next.amount > 0)) throw "金额要大于 0";
    checkMoney(next.flow, next.currency, next.kindId, next.accountId);
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
      const k = `${e.day.slice(0, 7)}|${e.currency}|${e.flow}|${e.kindId}`;
      const t = m.get(k) ?? { month: e.day.slice(0, 7), currency: e.currency, flow: e.flow, kindId: e.kindId, amount: 0, count: 0 };
      t.amount += e.amount;
      t.count += 1;
      m.set(k, t);
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

// ---- 投石问路 (jobs): mirrors jobs.rs loosely (stages, stamps, reached, follow-up, linked task) ----
type JStage = "wish" | "applied" | "active" | "got" | "closed";
type JStatus = { id: number; name: string; stage: JStage; sortOrder: number };
type JRow = {
  id: number; company: string; position: string | null; location: string | null; link: string | null; kindId: number | null;
  resume: string | null; coverLetter: string | null; deadline: string | null; salary: string | null; note: string | null;
  statusId: number; appliedOn: string | null; stepOn: string | null; followedOn: string | null; reachedId: number | null; taskId: number | null;
  createdAt: string; updatedAt: string;
};
let jb = { statuses: [] as JStatus[], jobs: [] as JRow[] };
(
  [["想投", "wish"], ["已投递", "applied"], ["网申测评 / 笔试", "active"], ["面试", "active"], ["Offer", "got"], ["已接受", "got"], ["被拒", "closed"]] as const
).forEach(([name, stage], i) => jb.statuses.push({ id: nextId++, name, stage, sortOrder: i + 1 }));
["官网", "内推", "Boss 直聘", "牛客", "LinkedIn", "学校", "其他"].forEach((n) => addKind("job_channel", n));
itemsUsing.job_channel = () => jb.jobs as never;
const jStatus = (id: number) => jb.statuses.find((x) => x.id === id)!;
const jStatusNamed = (n: string) => jb.statuses.find((x) => x.name === n)!;
const jobOut = (j: JRow) => {
  const stage = jStatus(j.statusId).stage;
  const since = [j.appliedOn, j.followedOn].filter(Boolean).sort().pop() ?? null;
  const waitingDays = stage === "applied" && since ? Math.round((parseYmd(mockToday()).getTime() - parseYmd(since).getTime()) / 86400000) : null;
  return { ...j, stage, waitingDays, followUp: waitingDays !== null && waitingDays >= 14 };
};
const jobTask = (j: JRow) => {
  const st = jStatus(j.statusId);
  const label = j.position ? `${j.company} · ${j.position}` : j.company;
  const want = st.stage === "wish" && j.deadline ? [`投递 ${label}`, j.deadline] : st.stage === "active" && j.stepOn ? [`${st.name} ${label}`, j.stepOn] : null;
  const task = tTasks.find((t) => t.id === j.taskId);
  if (want && task) {
    if (task.title !== want[0] || task.dueDate !== want[1]) Object.assign(task, { title: want[0], dueDate: want[1], startDate: null, done: false, doneAt: null });
  } else if (want) {
    const linked = jb.jobs.map((x) => tTasks.find((t) => t.id === x.taskId)).find(Boolean);
    const group = linked?.groupId ?? tGroups.find((g) => g.name === "💼 求职")?.id ?? addTGroup("💼 求职").id;
    j.taskId = addTTask({ groupId: group, title: want[0], dueDate: want[1] }).id;
  } else if (task && !task.done) markDone(task);
};
const jobSave = (j: JRow, p: Partial<JRow>, today: string) => {
  const statusChanged = p.statusId !== undefined && p.statusId !== j.statusId;
  if (statusChanged && !("stepOn" in p)) j.stepOn = null;
  Object.assign(j, p, { updatedAt: now() });
  if (!j.company?.trim()) throw "公司不能为空";
  j.company = j.company.trim();
  const st = jStatus(j.statusId);
  if (["applied", "active", "got"].includes(st.stage) && !j.appliedOn) j.appliedOn = today;
  const reached = j.reachedId ? jStatus(j.reachedId) : undefined;
  if (st.stage !== "closed" && (!reached || reached.sortOrder < st.sortOrder)) j.reachedId = st.id;
  jobTask(j);
};
{
  const ch = (n: string) => kindId("job_channel", n);
  const add = (p: Partial<JRow>) => {
    const j: JRow = { id: nextId++, company: "", position: null, location: null, link: null, kindId: null, resume: null, coverLetter: null, deadline: null, salary: null, note: null, statusId: jStatusNamed("想投").id, appliedOn: null, stepOn: null, followedOn: null, reachedId: null, taskId: null, createdAt: now(), updatedAt: now() };
    jb.jobs.push(j);
    const { statusId, ...rest } = p;
    jobSave(j, { ...rest, statusId: statusId ?? j.statusId }, mockToday());
    if (p.reachedId) j.reachedId = p.reachedId;
    return j;
  };
  add({ company: "KeyReply", position: "软件开发实习", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/template-it-cv-v1.docx", statusId: jStatusNamed("被拒").id, reachedId: jStatusNamed("已投递").id, note: "CV 阶段被拒" });
  add({ company: "中国银行新加坡分行", position: "Application Development Assistant（实习）", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/template-it-cv-v1.docx", statusId: jStatusNamed("已接受").id, stepOn: "2026-08-13", reachedId: jStatusNamed("已接受").id, note: "2026-09-07 入职，金融科技部" });
  add({ company: "NUS-ISS 学院代投", position: "实习（学院转投）", location: "新加坡", kindId: ch("学校"), resume: "~/Documents/Seeker/cv/en/seek-it-iss-cv-v3.docx", statusId: jStatusNamed("已投递").id, appliedOn: dayOffset(-30) });
  add({ company: "腾讯", position: "后台开发", location: "深圳", kindId: ch("官网"), statusId: jStatusNamed("面试").id, appliedOn: dayOffset(-9), stepOn: dayOffset(2), link: "https://join.qq.com" });
  add({ company: "字节跳动", position: "后端开发", location: "北京", kindId: ch("内推"), statusId: jStatusNamed("已投递").id, appliedOn: dayOffset(-16) });
  add({ company: "美团", position: "Java 开发", location: "北京", kindId: ch("牛客"), statusId: jStatusNamed("网申测评 / 笔试").id, appliedOn: dayOffset(-5), stepOn: dayOffset(1) });
  add({ company: "阿里巴巴", position: "Java 开发", location: "杭州", kindId: ch("官网"), deadline: dayOffset(10), salary: "25k × 16" });
  add({ company: "招商银行", position: "信息技术岗", location: "深圳", kindId: ch("官网"), statusId: jStatusNamed("被拒").id, appliedOn: dayOffset(-20), reachedId: jStatusNamed("网申测评 / 笔试").id });
}
const jobHandlers: Record<string, (a: Args) => unknown> = {
  job_statuses: () => [...jb.statuses].sort((a, b) => a.sortOrder - b.sortOrder).map((st) => ({ ...st, count: jb.jobs.filter((j) => j.statusId === st.id).length })),
  job_status_create: undoable("添加状态", (a) => {
    const name = String(a.name).trim();
    if (!name) throw "状态名不能为空";
    if (jb.statuses.some((x) => x.name === name)) throw `已经有「${name}」这个状态了`;
    const st = { id: nextId++, name, stage: a.stage as JStage, sortOrder: jb.statuses.length + 1 };
    jb.statuses.push(st);
    return { ...st, count: 0 };
  }),
  job_status_update: undoable("修改状态", (a) => {
    const st = jStatus(a.id as number);
    if (a.name != null) st.name = String(a.name).trim();
    if (a.stage != null) st.stage = a.stage as JStage;
    jb.jobs.filter((j) => j.statusId === st.id).forEach(jobTask);
    return { ...st, count: jb.jobs.filter((j) => j.statusId === st.id).length };
  }),
  job_status_delete: undoable("删除状态", (a) => {
    const n = jb.jobs.filter((j) => j.statusId === a.id).length;
    if (n) throw `还有 ${n} 条投递是「${jStatus(a.id as number).name}」，先把它们改成别的状态`;
    jb.statuses = jb.statuses.filter((x) => x.id !== a.id);
    for (const j of jb.jobs) if (j.reachedId === a.id) j.reachedId = null;
    return null;
  }),
  job_statuses_reorder: undoable("调整顺序", (a) => reorderPart(jb.statuses, a.ids as number[])),
  jobs_list: () =>
    [...jb.jobs]
      .sort((x, y) => jStatus(x.statusId).sortOrder - jStatus(y.statusId).sortOrder || (y.appliedOn ?? y.createdAt).localeCompare(x.appliedOn ?? x.createdAt))
      .map(jobOut),
  job_create: undoable("添加投递", (a) => {
    const p = a.patch as Partial<JRow>;
    const j: JRow = { id: nextId++, company: "", position: null, location: null, link: null, kindId: null, resume: null, coverLetter: null, deadline: null, salary: null, note: null, statusId: (jb.statuses.find((x) => x.stage === "wish") ?? jb.statuses[0]).id, appliedOn: null, stepOn: null, followedOn: null, reachedId: null, taskId: null, createdAt: now(), updatedAt: now() };
    jobSave(j, p, mockToday());
    jb.jobs.push(j);
    return jobOut(j);
  }),
  job_update: undoable("修改投递", (a) => {
    const j = jb.jobs.find((x) => x.id === a.id);
    if (!j) throw "找不到这个投递，可能已经被删除了";
    const copy = { ...j };
    jobSave(copy, a.patch as Partial<JRow>, mockToday());
    Object.assign(j, copy);
    return jobOut(j);
  }),
  job_follow_up: undoable("已跟进", (a) => {
    const j = jb.jobs.find((x) => x.id === a.id)!;
    j.followedOn = mockToday();
    return jobOut(j);
  }),
  job_delete: undoable("删除投递", (a) => {
    const j = jb.jobs.find((x) => x.id === a.id)!;
    const t = tTasks.find((x) => x.id === j.taskId);
    if (t && !t.done) tTasks = tTasks.filter((x) => x.id !== t.id);
    jb.jobs = jb.jobs.filter((x) => x.id !== a.id);
    return null;
  }),
};

// ---- 念念不忘 (notes): mirrors notes.rs loosely (inline tags incl. sub-tags, refs both ways, rename) ----
type NRow = { id: number; content: string; pinned: boolean; refs: number[]; createdAt: string; updatedAt: string };
let nt = { notes: [] as NRow[] };
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
      tags: [...under].sort().map(([tag, ids]) => ({ tag, count: ids.size })),
    };
  },
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

const handlers: Record<string, (a: Args) => unknown> = {
  app_info: () => ({
    appVersion: "0.1.0-mock",
    isDev: true,
    dataDir: "/mock/Pomelo OS Dev",
    dbPath: "/mock/Pomelo OS Dev/pomelo.db",
    schemaVersion: 15,
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
  "plugin:dialog|save": () => "/mock/pomelo-export.json",
  export_to: (a) => ({
    path: a.path,
    modules: [
      { key: "books", label: "摸鱼读读", incoming: handlers["books__count"]({}) },
      { key: "films", label: "好奇看看", incoming: handlers["films__count"]({}) },
      { key: "drawing", label: "随意画画", incoming: courses.length },
      { key: "packing", label: "出门走走", incoming: tpls.length + tripRows.length },
    ],
  }),
  backups_list: () => [...backups].sort((x, y) => y.fileName.localeCompare(x.fileName)),
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
