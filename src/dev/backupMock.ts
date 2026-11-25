// DevJanitor 备份 in the browser mock (`?mock#/janitor/backup`): the fake drive and three tasks shaped like the
// fake Mac's (Rust janitor/fake.rs). 开始备份 runs for three seconds, then the copied groups empty and a record
// is added; 在硬盘上跟着改 takes the moves off; 重新对比 says 正在对比 for a second. `&backup=none`: no drive picked.
import type { BackupDiff, BackupOverview, BackupRecord, BackupTask, BackupTaskInput } from "@/modules/youzi/assistants/janitorApi";

type Args = Record<string, unknown>;

const DRIVE = { uuid: "FAKE-0000-0000-0001", name: "小柚的硬盘", fs: "Mac OS Extended (Journaled)", total: 2_000_000_000_000, free: 1_186_000_000_000 };
const row = (path: string, size: number) => ({ path, size, mtime: 1_788_000_000 });
const group = <T extends { size: number }>(rows: T[]) => ({ count: rows.length, bytes: rows.reduce((a, r) => a + r.size, 0), rows });
const diff = (p: Partial<Record<"new" | "changed" | "recent" | "archived", { path: string; size: number; mtime: number }[]>> & { moved?: { from: string; to: string; size: number }[]; held?: { path: string; size: number; why: string }[]; same?: number }): BackupDiff => ({
  new: group(p.new ?? []),
  changed: group(p.changed ?? []),
  moved: group(p.moved ?? []),
  held: group(p.held ?? []),
  recent: group(p.recent ?? []),
  archived: group(p.archived ?? []),
  same: p.same ?? 0,
  sameBytes: (p.same ?? 0) * 120_000,
  denied: [],
});

const on = () => new URLSearchParams(location.search).get("backup") !== "none";
let picked = on();
let tasks: BackupTask[] = [
  { id: 1, source: "/Users/me/Downloads/资料", target: "资料", excludes: [], last: "2026-09-27T09:00:00.000Z", problem: null, diff: diff({ new: [row("2609 实习/合同.pdf", 50_000)], changed: [row("2609 实习/日程.xlsx", 15_000)], archived: [row("2302 证书.zip", 300_000), row("2507 课程/大作业.zip", 900_000)], same: 3 }) },
  {
    id: 2,
    source: "/Users/me/Downloads/照片",
    target: "照片",
    excludes: [],
    last: "2026-09-27T09:00:00.000Z",
    problem: null,
    diff: diff({ new: [row("截图/截屏 1.png", 60_000), row("截图/截屏 2.png", 61_000)], moved: [{ from: "其它/IMG_0101.jpg", to: "其他/IMG_0101.jpg", size: 120_000 }, { from: "其它/IMG_0102.jpg", to: "其他/IMG_0102.jpg", size: 130_000 }], archived: [row("素材/纹理.zip", 400_000)], same: 3 }),
  },
  { id: 3, source: "/Users/me/Downloads/小柚", target: "回忆/小柚", excludes: [], last: null, problem: null, diff: diff({ new: [row("2026/c.jpg", 160_000)], held: [{ path: "2026/d.jpg", size: 170_000, why: "硬盘上的更新" }], same: 2 }) },
];
let nextId = 4;
let runs: BackupRecord[] = [];
let comparingUntil = 0;
let run: { until: number; ids: number[]; files: number; bytes: number } | null = null;
let ended: string | null = null;

/** A finished run: what it copied leaves the comparison. */
function settle() {
  if (!run || Date.now() < run.until) return;
  const r = run;
  run = null;
  tasks = tasks.map((t) => (r.ids.includes(t.id) && t.diff ? { ...t, last: new Date().toISOString(), diff: { ...t.diff, new: group([]), changed: group([]), same: t.diff.same + t.diff.new.count + t.diff.changed.count } } : t));
  runs = [{ id: runs.length + 1, drive: DRIVE.name, status: "done", tasks: tasks.filter((t) => r.ids.includes(t.id)).map((t) => ({ id: t.id, source: t.source, target: t.target, files: 0, bytes: 0, updated: 0, done: true })), files: r.files, bytes: r.bytes, errors: [], note: null, startedAt: new Date(r.until - 3000).toISOString(), finishedAt: new Date().toISOString() }, ...runs];
  ended = `备份好了：复制了 ${r.files} 个文件`;
}

function view(): BackupOverview {
  settle();
  const now = Date.now();
  const running = !!run;
  const frac = run ? Math.min(1, 1 - (run.until - now) / 3000) : 0;
  return {
    mode: "sandbox",
    home: "/Users/me",
    drive: picked ? { uuid: DRIVE.uuid, name: DRIVE.name, connected: true, fs: DRIVE.fs, total: DRIVE.total, free: DRIVE.free, path: "/Volumes/小柚的硬盘", folders: ["影音", "照片", "回忆", "回忆_2606", "资料"] } : null,
    volumes: [{ uuid: DRIVE.uuid, name: DRIVE.name, fs: DRIVE.fs, total: DRIVE.total, free: DRIVE.free }],
    tasks: picked ? tasks : [],
    comparing: now < comparingUntil,
    comparedAt: Math.floor(now / 1000),
    run: { running, filesTotal: run?.files ?? 0, filesDone: Math.floor((run?.files ?? 0) * frac), bytesTotal: run?.bytes ?? 0, bytesDone: Math.floor((run?.bytes ?? 0) * frac), task: running ? "照片" : "", current: running ? "截图/截屏 1.png" : "", ended: running ? null : ended },
    runs,
  };
}

export const backupHandlers: Record<string, (a: Args) => unknown> = {
  janitor_backup_overview: () => view(),
  janitor_backup_compare: () => {
    comparingUntil = Date.now() + 1000;
    return view();
  },
  janitor_backup_drive: (a) => {
    picked = a.uuid != null;
    return view();
  },
  janitor_backup_task_save: (a) => {
    const t = a.task as BackupTaskInput;
    if (!t.source.trim() || !t.target.trim()) throw "要选电脑上的文件夹和移动硬盘上的文件夹";
    if (t.target.split("/")[0] === "旧版") throw "「旧版」是放旧版的地方";
    if (/(^|\/)vault(\.|\/|$)/i.test(t.source) || /(^|\/)vault(\.|\/|$)/i.test(t.target)) throw "这个地方不能备份";
    const source = t.source.replace(/^\/Users\/[^/]+\/Library\/Application Support\/Pomelo OS Dev\/janitor-sandbox/, "");
    if (t.id != null) tasks = tasks.map((x) => (x.id === t.id ? { ...x, source, target: t.target.trim(), excludes: t.excludes } : x));
    else tasks = [...tasks, { id: nextId++, source, target: t.target.trim(), excludes: t.excludes, last: null, problem: null, diff: diff({ new: [row("设计/海报.psd", 1_000_000), row("笔记/读书.md", 8_000)] }) }];
    return view();
  },
  janitor_backup_task_delete: (a) => {
    tasks = tasks.filter((t) => t.id !== a.id);
    return view();
  },
  janitor_backup_start: (a) => {
    if (run) throw "正在备份";
    const ids = a.ids as number[];
    const chosen = tasks.filter((t) => ids.includes(t.id) && t.diff);
    run = { until: Date.now() + 3000, ids, files: chosen.reduce((s, t) => s + (t.diff?.new.count ?? 0) + (t.diff?.changed.count ?? 0), 0), bytes: chosen.reduce((s, t) => s + (t.diff?.new.bytes ?? 0) + (t.diff?.changed.bytes ?? 0), 0) };
    ended = null;
    return view();
  },
  janitor_backup_stop: () => {
    if (run) {
      run = null;
      ended = "停下了：复制了 0 个文件（0 KB）";
    }
    return null;
  },
  janitor_backup_move: (a) => {
    const paths = a.paths as string[];
    let done = 0;
    tasks = tasks.map((t) => {
      if (t.id !== a.id || !t.diff) return t;
      const left = t.diff.moved.rows.filter((m) => !paths.includes(m.to));
      done = t.diff.moved.rows.length - left.length;
      return { ...t, diff: { ...t.diff, moved: group(left), same: t.diff.same + done } };
    });
    return { done, failed: [], overview: view() };
  },
};
