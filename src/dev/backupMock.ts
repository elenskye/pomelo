// DevJanitor 备份 in the browser mock (`?mock#/janitor/backup`): the fake drive and ~/Downloads shaped like the fake
// Mac's (Rust janitor/fake.rs). The 清单 ticks 资料, 照片, 小柚, 回忆 and 影音 (回忆_2606 and 作品 not): the three in
// ~/Downloads are tasks to the drive folder of their name; ticking 作品 adds one, unticking drops it.
// 开始备份 runs for three seconds, then the copied groups empty and a record is added; 在硬盘上跟着改 takes the moves
// off; 重新对比 says 正在对比 for a second. `&backup=none`: no drive picked.
// 柚子 has looked already (a card of renamed folders; janitorMock.ts `backupCards`); 柚子看一眼 looks again for a
// second and a half. A done move card puts 小柚 where its task looks and takes the moves off.
// PikPak: logged in and compared (`&pikpak=none`: not logged in): the ticked drive folders, three with something
// to do; 开始同步 runs five seconds and puts the folders in step; 停下 stops it; logging in or a backup ending compares
// again for a second. How a run or a sync ended pops up once (`ends`).
import type { BackupDiff, BackupListRow, BackupLook, BackupOverview, BackupRecord, BackupTask, CloudDiff, PikPakView } from "@/modules/youzi/assistants/janitorApi";
import { BACKUP_TEXT, backupCardHooks, backupCards, janitorCards } from "./janitorMock";

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
/** The 清单: the drive's top folders and ~/Downloads' (files, bytes as compared). */
const DRIVE_TOPS: Record<string, [number, number]> = { 影音: [1, 5_000_000], 照片: [7, 1_120_000], 回忆: [3, 2_320_000], 回忆_2606: [1, 2_000_000], 资料: [6, 1_306_000] };
const MAC_TOPS: Record<string, [number, number]> = { 作品: [4, 1_023_000], 小柚: [4, 2_480_000], 照片: [7, 871_000], 资料: [5, 125_000] };
let ticked = new Set(["资料", "照片", "小柚", "回忆", "影音"]);
const WORKS = diff({ new: [row("设计/海报.psd", 1_000_000), row("笔记/读书.md", 8_000), row("lab/train.ipynb", 12_000), row("web/index.html", 3_000)] });
let tasks: BackupTask[] = [
  { id: 1, source: "/Users/me/Downloads/资料", target: "资料", last: "2026-09-27T09:00:00.000Z", problem: null, diff: diff({ new: [row("2609 实习/合同.pdf", 50_000)], changed: [row("2609 实习/日程.xlsx", 15_000)], archived: [row("2302 证书.zip", 300_000), row("2507 课程/大作业.zip", 900_000)], same: 3 }) },
  {
    id: 2,
    source: "/Users/me/Downloads/照片",
    target: "照片",
    last: "2026-09-27T09:00:00.000Z",
    problem: null,
    diff: diff({ new: [row("截图/截屏 1.png", 60_000), row("截图/截屏 2.png", 61_000)], moved: [{ from: "其它/IMG_0101.jpg", to: "其他/IMG_0101.jpg", size: 120_000 }, { from: "其它/IMG_0102.jpg", to: "其他/IMG_0102.jpg", size: 130_000 }], archived: [row("素材/纹理.zip", 400_000)], same: 3 }),
  },
  { id: 3, source: "/Users/me/Downloads/小柚", target: "小柚", last: null, problem: null, diff: diff({ new: [row("2024/a.mov", 2_000_000), row("2024/b.jpg", 150_000), row("2026/c.jpg", 160_000), row("2026/d.jpg", 170_000)] }) },
];
let nextId = 4;
let runs: BackupRecord[] = [];
let comparingUntil = 0;
let run: { until: number; ids: number[]; files: number; bytes: number } | null = null;
let ended: string | null = null;
let ends = 0;
const pikParam = () => new URLSearchParams(location.search).get("pikpak");
const item = (path: string, size: number) => ({ path, size });
const cloud = (p: { new?: [string, number][]; changed?: [string, number][]; moved?: { from: string; to: string; size: number }[]; extra?: [string, number][]; same?: number }): CloudDiff => ({
  same: p.same ?? 0,
  sameBytes: (p.same ?? 0) * 150_000,
  new: group((p.new ?? []).map(([a, n]) => item(a, n))),
  changed: group((p.changed ?? []).map(([a, n]) => item(a, n))),
  moved: group(p.moved ?? []),
  extra: group((p.extra ?? []).map(([a, n]) => item(a, n))),
});
/** How each drive folder compares with PikPak (the ticked ones are listed). */
const CLOUD: Record<string, CloudDiff> = {
  资料: cloud({ new: [["2609 实习/合同.pdf", 50_000]], changed: [["2609 实习/日程.xlsx", 15_000]], same: 4 }),
  照片: cloud({ moved: [{ from: "其它/IMG_0101.jpg", to: "其他/IMG_0101.jpg", size: 120_000 }, { from: "其它/IMG_0102.jpg", to: "其他/IMG_0102.jpg", size: 130_000 }], same: 4 }),
  回忆: cloud({ extra: [["小柚/2024/旧的.mov", 1_200_000]], same: 3 }),
  影音: cloud({ new: [["电影/片.mkv", 5_000_000]] }),
  回忆_2606: cloud({ same: 1 }),
};
let pik = { account: pikParam() === "none" ? (null as string | null) : "xiaoyou@example.com", folder: "小柚的硬盘", limit: 5, ended: null as string | null, ends: 0, comparingUntil: 0, sync: null as { until: number; tops: string[]; files: number; bytes: number } | null };

function pikSettle() {
  const r = pik.sync;
  if (!r || Date.now() < r.until) return;
  pik.sync = null;
  for (const t of r.tops) CLOUD[t] = cloud({ same: CLOUD[t].same + CLOUD[t].new.count + CLOUD[t].changed.count + CLOUD[t].moved.count });
  pik.ended = `同步好了：传了 ${r.files} 个（${(r.bytes / 1_000_000).toFixed(0)} MB）`;
  pik.ends++;
  runs = [{ id: runs.length + 1, dest: "pikpak", drive: DRIVE.name, status: "done", tasks: r.tops.map((t) => ({ id: 0, source: "", target: t, files: 0, bytes: 0, updated: 0, done: true })), files: r.files, bytes: r.bytes, errors: [], note: null, startedAt: new Date(r.until - 5000).toISOString(), finishedAt: new Date().toISOString() }, ...runs];
}

function pikView(): PikPakView {
  pikSettle();
  const r = pik.sync;
  const frac = r ? Math.min(1, 1 - (r.until - Date.now()) / 5000) : 0;
  const checking = !!r && frac < 0.2;
  const comparing = Date.now() < pik.comparingUntil;
  const last = (t: string) => runs.find((x) => x.dest === "pikpak" && x.tasks.some((y) => y.target === t))?.finishedAt ?? null;
  const tops = Object.keys(DRIVE_TOPS).filter((t) => ticked.has(t)).sort();
  return {
    installed: true,
    account: pik.account,
    folder: pik.folder,
    limit: pik.limit,
    running: !!r,
    syncing: r?.tops ?? [],
    checking,
    bytes: r && !checking ? Math.floor(r.bytes * frac) : 0,
    totalBytes: r && !checking ? r.bytes : 0,
    files: r && !checking ? Math.floor(r.files * frac) : 0,
    totalFiles: r && !checking ? r.files : 0,
    deletes: 0,
    speed: r && !checking ? 1_800_000 : 0,
    eta: r && !checking ? Math.ceil((r.until - Date.now()) / 1000) : 0,
    current: r && !checking ? ["影音/电影/片.mkv"] : [],
    ended: pik.ended,
    ends: pik.ends,
    waiting: r ? null : !pik.account ? "没登录" : run ? "正在备份" : null,
    used: pik.account ? 812_000_000_000 : null,
    total: pik.account ? 6_000_000_000_000 : null,
    comparing,
    comparedAt: pik.account ? Math.floor(Date.now() / 1000) : null,
    missing: false,
    error: null,
    folders: picked ? tops.map((t) => ({ name: t, last: last(t), diff: pik.account && !comparing ? (CLOUD[t] ?? cloud({ same: 2 })) : null })) : [],
  };
}

/** 柚子's look: its (made-up) reply's id, when, until when it's still writing. */
let look: { msg: number; at: number; until: number } | null = null;
let lookMsg = 900_000;

function lookView(): BackupLook | null {
  if (!picked) return null;
  if (!look) {
    look = { msg: ++lookMsg, at: Math.floor(Date.now() / 1000) - 60, until: 0 };
    backupCards(look.msg);
  }
  const running = Date.now() < look.until;
  return { at: look.at, state: running ? "running" : "done", text: running ? "" : BACKUP_TEXT, error: null, cards: running ? [] : structuredClone(janitorCards.get(look.msg) ?? []), stale: false };
}

backupCardHooks.done = (c) => {
  if (c.kind !== "move") return;
  tasks = tasks.map((t) => (t.diff && t.diff.moved.count ? { ...t, diff: { ...t.diff, moved: group([]), same: t.diff.same + t.diff.moved.count } } : t));
  if (c.items.some((i) => i.done && i.to === "小柚")) {
    tasks = tasks.map((t) => (t.target === "小柚" ? { ...t, diff: diff({ new: [row("2026/c.jpg", 160_000)], held: [{ path: "2026/d.jpg", size: 170_000, why: "硬盘上的更新" }], same: 2 }) } : t));
    delete DRIVE_TOPS["回忆"];
    DRIVE_TOPS["小柚"] = [3, 2_320_000];
  }
};

function list(): BackupListRow[] {
  const names = [...new Set([...Object.keys(DRIVE_TOPS), ...Object.keys(MAC_TOPS)])].sort();
  return names.map((name) => {
    const size = DRIVE_TOPS[name] ?? MAC_TOPS[name];
    return { name, onDrive: name in DRIVE_TOPS, onMac: name in MAC_TOPS, picked: ticked.has(name), files: size[0], bytes: size[1] };
  });
}

/** A finished run: what it copied leaves the comparison. */
function settle() {
  if (!run || Date.now() < run.until) return;
  const r = run;
  run = null;
  tasks = tasks.map((t) => (r.ids.includes(t.id) && t.diff ? { ...t, last: new Date().toISOString(), diff: { ...t.diff, new: group([]), changed: group([]), same: t.diff.same + t.diff.new.count + t.diff.changed.count } } : t));
  runs = [{ id: runs.length + 1, dest: "drive", drive: DRIVE.name, status: "done", tasks: tasks.filter((t) => r.ids.includes(t.id)).map((t) => ({ id: t.id, source: t.source, target: t.target, files: 0, bytes: 0, updated: 0, done: true })), files: r.files, bytes: r.bytes, errors: [], note: null, startedAt: new Date(r.until - 3000).toISOString(), finishedAt: new Date().toISOString() }, ...runs];
  ended = r.files ? `备份好了：复制了 ${r.files} 个文件` : "备份好了：没有要复制的";
  ends++;
  pik.comparingUntil = Date.now() + 1000;
}

function view(): BackupOverview {
  settle();
  const now = Date.now();
  const running = !!run;
  const frac = run ? Math.min(1, 1 - (run.until - now) / 3000) : 0;
  return {
    mode: "sandbox",
    home: "/Users/me",
    drive: picked ? { uuid: DRIVE.uuid, name: DRIVE.name, connected: true, fs: DRIVE.fs, total: DRIVE.total, free: DRIVE.free, path: "/Volumes/小柚的硬盘" } : null,
    volumes: [{ uuid: DRIVE.uuid, name: DRIVE.name, fs: DRIVE.fs, total: DRIVE.total, free: DRIVE.free }],
    tasks: picked ? tasks : [],
    comparing: now < comparingUntil,
    comparedAt: Math.floor(now / 1000),
    run: { running, filesTotal: run?.files ?? 0, filesDone: Math.floor((run?.files ?? 0) * frac), bytesTotal: run?.bytes ?? 0, bytesDone: Math.floor((run?.bytes ?? 0) * frac), task: running ? "照片" : "", current: running ? "截图/截屏 1.png" : "", ended: running ? null : ended, ends },
    runs,
    look: lookView(),
    pikpak: pikView(),
    list: picked ? list() : [],
  };
}

export const backupHandlers: Record<string, (a: Args) => unknown> = {
  janitor_backup_overview: () => view(),
  janitor_pikpak_login: (a) => {
    if (!String(a.account).trim() || !a.password) throw "要写 PikPak 的账号和密码";
    if (a.password === "wrong") throw "没登录上：账号或密码不对";
    pik.account = String(a.account).trim();
    pik.folder = String(a.folder).trim() || pik.folder;
    pik.comparingUntil = Date.now() + 1500;
    return view();
  },
  janitor_pikpak_logout: () => {
    pik = { ...pik, account: null, sync: null };
    return view();
  },
  janitor_pikpak_set: (a) => {
    const f = String(a.folder).trim().replace(/^\/+|\/+$/g, "");
    if (!f || f.split("/").some((p) => p === ".." || p.startsWith("."))) throw "文件夹的名字不对";
    pik = { ...pik, folder: f, limit: Number(a.limit) };
    return view();
  },
  janitor_pikpak_stop: () => {
    if (pik.sync) {
      pik.ended = "停下了：传了 1 个（1 MB）";
      pik.ends++;
    }
    pik = { ...pik, sync: null };
    return null;
  },
  janitor_pikpak_start: (a) => {
    if (pik.sync) throw "正在同步";
    const tops = (a.tops as string[]).filter((t) => t in CLOUD);
    if (!tops.length) throw "先勾要同步的文件夹";
    pik.sync = { until: Date.now() + 5000, tops, files: tops.reduce((s, t) => s + CLOUD[t].new.count + CLOUD[t].changed.count, 0), bytes: tops.reduce((s, t) => s + CLOUD[t].new.bytes + CLOUD[t].changed.bytes, 0) };
    return view();
  },
  janitor_backup_look: () => {
    look = { msg: ++lookMsg, at: Math.floor(Date.now() / 1000), until: Date.now() + 1500 };
    backupCards(look.msg);
    return view();
  },
  janitor_backup_compare: () => {
    comparingUntil = Date.now() + 1000;
    if (pik.account) pik.comparingUntil = Date.now() + 1500;
    return view();
  },
  janitor_backup_drive: (a) => {
    picked = a.uuid != null;
    return view();
  },
  janitor_backup_pick: (a) => {
    const name = String(a.name);
    if (!(name in DRIVE_TOPS) && !(name in MAC_TOPS)) throw "硬盘上和 Downloads 里都没有这个文件夹";
    if (a.on) {
      ticked.add(name);
      if (name in MAC_TOPS && !tasks.some((t) => t.target === name)) tasks = [...tasks, { id: nextId++, source: `/Users/me/Downloads/${name}`, target: name, last: null, problem: null, diff: name === "作品" ? WORKS : diff({ same: MAC_TOPS[name][0] }) }];
    } else {
      ticked.delete(name);
      tasks = tasks.filter((t) => t.target !== name);
    }
    ticked = new Set(ticked);
    comparingUntil = Date.now() + 800;
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
      ends++;
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
