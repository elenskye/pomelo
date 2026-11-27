// DevJanitor's 清理 and 环境 (1.1.5): both sorted into classes, each class a little sticker with its icon (janitor.css).
// 清理: a side list of the classes (一键清理's, then the listed ones; what's picked in each shows beside it) and the
// class picked on the right: each category a card with a tick for all of it; rows tick by their box, the path is
// shortened (~, the middle folded) and Finder opens from the folder icon. .DS_Store and Office's lock files have a
// button of their own. One bar on top: 清理选中的 (for good, one confirm), 移到废纸篓 (a protected one asks again),
// 清空废纸篓. 环境: the classes as chips that jump to their section; a section's cards share two columns when wide.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { AppWindow, Archive, Bot, Braces, CircleQuestionMark, Code, Container, Copy, Database, Disc3, Download, Dustpan, FolderOpen, Ghost, Hammer, Mail, Package, Smartphone, Sparkles, Weight, Wrench, type IconType } from "@/shared/icons";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, EmptyState, IconButton, Picked } from "@/shared/ui";
import { JANITOR_KEY, ago, bytes, janitorApi, type JanitorEnv, type JanitorEnvItem, type JanitorGroup, type JanitorItem, type JanitorOutcome, type JanitorOverview } from "./janitorApi";

type Tone = "mint" | "sky" | "pink" | "orange" | "grape";
type Look = { name: string; icon: IconType; tone: Tone };

/** 清理's classes (Rust rules.rs `Category::class`, `GROUPS`), in the side list's order. */
const CLEAN: [string, Look][] = [
  ["deps", { name: "项目的依赖", icon: Package, tone: "mint" }],
  ["build", { name: "编译产物", icon: Hammer, tone: "orange" }],
  ["download", { name: "下载缓存", icon: Download, tone: "sky" }],
  ["app", { name: "App 的缓存", icon: AppWindow, tone: "grape" }],
  ["litter", { name: "零碎的小文件", icon: Dustpan, tone: "pink" }],
  ["agent", { name: "Agent 加的", icon: Sparkles, tone: "orange" }],
  ["big", { name: "大文件", icon: Weight, tone: "orange" }],
  ["dup", { name: "重复的文件", icon: Copy, tone: "sky" }],
  ["installers", { name: "安装包和压缩包", icon: Disc3, tone: "mint" }],
  ["left", { name: "App 留下的", icon: Ghost, tone: "grape" }],
  ["xarchive", { name: "Xcode 的 Archives", icon: Archive, tone: "orange" }],
  ["mail", { name: "邮件的数据", icon: Mail, tone: "pink" }],
  ["docker", { name: "Docker", icon: Container, tone: "sky" }],
];
const CLEAN_LOOK = new Map(CLEAN);

/** 环境's classes (Rust env.rs `Env::class`). */
const ENV: [string, Look][] = [
  ["lang", { name: "编程语言", icon: Braces, tone: "mint" }],
  ["pkg", { name: "包管理", icon: Package, tone: "orange" }],
  ["vm", { name: "容器和虚拟机", icon: Container, tone: "sky" }],
  ["app", { name: "做 App 和游戏", icon: Smartphone, tone: "pink" }],
  ["editor", { name: "编辑器和 IDE", icon: Code, tone: "grape" }],
  ["ai", { name: "AI 工具和模型", icon: Bot, tone: "orange" }],
  ["db", { name: "数据库", icon: Database, tone: "sky" }],
  ["tools", { name: "其他工具", icon: Wrench, tone: "mint" }],
  ["unknown", { name: "认不出来的", icon: CircleQuestionMark, tone: "grape" }],
];

/** The litter kinds that clean on a button of their own. */
const LITTER = new Set(["ds_store", "office_temp"]);

/** `count`: how many are picked in that class, a little number on the sticker's corner. */
function Badge(props: { look: Look; small?: boolean; tilt?: number; count?: number }) {
  const Icon = props.look.icon;
  return (
    <span aria-hidden className={`jn-badge jn-${props.look.tone}${props.small ? " jn-small" : ""}`} style={props.tilt != null ? ({ "--jn-tilt": `${props.tilt}deg` } as CSSProperties) : undefined}>
      <Icon size={props.small ? 15 : 18} />
      {!!props.count && <span className="jn-count tabular-nums">{props.count}</span>}
    </span>
  );
}

/** "/Users/me/Documents/GitHub/a/b/c/d/target" → "~/Documents/GitHub/…/c/d/target". */
export function shortPath(p: string, home: string): string {
  const s = p === home ? "~" : p.startsWith(`${home}/`) ? `~${p.slice(home.length)}` : p;
  const parts = s.split("/");
  return parts.length > 7 ? [...parts.slice(0, 3), "…", ...parts.slice(-3)].join("/") : s;
}

function Reveal(props: { path: string }) {
  const toast = useToast();
  return (
    <span className="shrink-0 opacity-0 group-hover:opacity-100 focus-within:opacity-100">
      <IconButton label="在访达中显示" onClick={() => void janitorApi.reveal(props.path).catch((e) => toast(errorText(e), "error"))}>
        <FolderOpen size={14} />
      </IconButton>
    </span>
  );
}

function ItemRow(props: { item: JanitorItem; home: string; on: boolean; toggle: () => void }) {
  const it = props.item;
  const busy = !!it.inUse;
  const where = it.project ?? it.paths[0];
  return (
    <li className={`group flex items-start gap-1 py-1.5 pr-3 text-sm ${busy ? "text-muted-foreground" : ""}`}>
      <label className={`flex shrink-0 self-stretch px-3 pt-1 ${busy ? "" : "cursor-pointer"}`}>
        <input type="checkbox" checked={props.on && !busy} disabled={busy} onChange={props.toggle} aria-label={it.title} className="size-3.5 accent-[var(--primary)]" />
      </label>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
          <span className="min-w-0 truncate">{it.title}</span>
          {it.tag && <span className="shrink-0 text-xs text-muted-foreground">{it.tag}</span>}
          {it.inUse && <span className="shrink-0 rounded-full bg-muted px-1.5 text-xs">在用 · {it.inUse}</span>}
          {it.protected && <span className="shrink-0 rounded-full bg-accent px-1.5 text-xs text-primary">受保护 · {it.protected}</span>}
        </div>
        <div className="flex min-w-0 items-baseline gap-x-2 text-xs text-muted-foreground">
          <span className="min-w-0 truncate font-mono" title={it.paths.join("\n")}>
            {shortPath(where, props.home)}
            {it.paths.length > 1 ? ` 等 ${it.paths.length} 处` : ""}
          </span>
          {it.newest > 0 && <span className="shrink-0">· {ago(it.newest)}</span>}
        </div>
        {it.note && <div className="text-xs text-muted-foreground">{it.note}</div>}
      </div>
      <Reveal path={where} />
      <span className="w-20 shrink-0 pt-px text-right tabular-nums">{bytes(it.size)}</span>
    </li>
  );
}

/** A tick for a whole category: on when all of it that can be ticked is, a dash when some is. */
function AllBox(props: { items: JanitorItem[]; on: (it: JanitorItem) => boolean; set: (items: JanitorItem[], v: boolean) => void; label: string }) {
  const ref = useRef<HTMLInputElement>(null);
  const can = props.items.filter((i) => !i.inUse);
  const n = can.filter(props.on).length;
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = n > 0 && n < can.length;
  });
  return (
    <label className="flex shrink-0 cursor-pointer self-center px-3">
      <input ref={ref} type="checkbox" checked={can.length > 0 && n === can.length} disabled={!can.length} onChange={() => props.set(can, n < can.length)} aria-label={props.label} className="size-3.5 accent-[var(--primary)]" />
    </label>
  );
}

function CategoryCard(props: { g: JanitorGroup; home: string; on: (it: JanitorItem) => boolean; toggle: (it: JanitorItem) => void; set: (items: JanitorItem[], v: boolean) => void }) {
  const { g } = props;
  const items = useMemo(() => [...g.items].sort((a, b) => b.size - a.size), [g.items]);
  return (
    <section className="mb-5 rounded-card border bg-card">
      <div className="flex items-start gap-1 border-b py-2.5 pr-4">
        <AllBox items={g.items} on={props.on} set={props.set} label={`${g.name} 全选`} />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-baseline gap-2">
            <span className="min-w-0 truncate font-bold">{g.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{g.items.length} 项</span>
            <span className="ml-auto shrink-0 tabular-nums">{bytes(g.size)}</span>
          </div>
          {g.rebuild && <p className="text-xs text-muted-foreground">{[g.rebuild, g.takes, g.network ? "要联网" : "不用联网"].filter(Boolean).join(" · ")}</p>}
        </div>
      </div>
      <ul className="divide-y">
        {items.map((it) => <ItemRow key={it.id} item={it} home={props.home} on={props.on(it)} toggle={() => props.toggle(it)} />)}
      </ul>
    </section>
  );
}

/** .DS_Store and Office's lock files: how many, how big, and a button of their own. */
function LitterCard(props: { g: JanitorGroup; busy: boolean; clean: (it: JanitorItem) => void }) {
  const it = props.g.items[0];
  if (!it) return null;
  return (
    <section className="mb-5 rounded-card border bg-card px-4 py-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="font-bold">{props.g.name}</span>
        <span className="text-sm text-muted-foreground tabular-nums">{it.tag} · {bytes(it.size)}</span>
        {it.inUse && <span className="shrink-0 rounded-full bg-muted px-1.5 text-xs">在用 · {it.inUse}</span>}
        <Button className="ml-auto" disabled={props.busy || !!it.inUse} onClick={() => props.clean(it)}>清理这 {it.tag}</Button>
      </div>
      {props.g.rebuild && <p className="mt-1 text-xs text-muted-foreground">{props.g.rebuild}</p>}
    </section>
  );
}

function DupCard(props: { g: JanitorGroup; home: string; picked: Set<number>; toggle: (it: JanitorItem) => void }) {
  const sets = useMemo(() => {
    const m = new Map<number, JanitorItem[]>();
    for (const it of props.g.items) m.set(it.dup ?? 0, [...(m.get(it.dup ?? 0) ?? []), it]);
    return [...m.values()].filter((v) => v.length > 1);
  }, [props.g.items]);
  return (
    <>
      {sets.map((set, i) => (
        <section key={i} className="mb-4 rounded-card border bg-card">
          <p className="border-b px-4 py-2 text-xs text-muted-foreground">{set[0].tag} · 每份 {bytes(set[0].size)}</p>
          <ul className="divide-y">{set.map((it) => <ItemRow key={it.id} item={it} home={props.home} on={props.picked.has(it.id)} toggle={() => props.toggle(it)} />)}</ul>
        </section>
      ))}
    </>
  );
}

function ListedCard(props: { g: JanitorGroup; home: string; picked: Set<number>; toggle: (it: JanitorItem) => void; titled: boolean }) {
  const items = useMemo(() => [...props.g.items].sort((a, b) => b.size - a.size), [props.g.items]);
  return (
    <section className="mb-5 rounded-card border bg-card">
      {props.titled && (
        <div className="flex items-baseline gap-2 border-b px-4 py-2.5">
          <span className="font-bold">{props.g.name}</span>
          <span className="ml-auto tabular-nums">{bytes(props.g.size)}</span>
        </div>
      )}
      <ul className="divide-y">{items.map((it) => <ItemRow key={it.id} item={it} home={props.home} on={props.picked.has(it.id)} toggle={() => props.toggle(it)} />)}</ul>
    </section>
  );
}

function outcomeText(o: JanitorOutcome, verb: string) {
  const head = !o.done ? `没有${verb}` : verb === "移走" ? `移到废纸篓 ${o.done} 项（${bytes(o.freed)}）` : `${verb}了 ${o.done} 项，腾出 ${bytes(o.freed)}`;
  return o.failed.length ? `${head}；${o.failed.length} 项没成：${o.failed.slice(0, 3).map(([t, why]) => `${t}（${why}）`).join("、")}` : head;
}

type Klass = { id: string; look: Look; listed: boolean; groups: JanitorGroup[]; size: number };

export function CleanTab(props: { ov: JanitorOverview }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const { categories, groups, summary, home } = props.ov;
  const at = summary?.scannedAt ?? 0;
  const [over, setOver] = useState<Map<number, boolean>>(new Map());
  const [listed, setListed] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [pick, setPick] = useState<string | null>(null);
  useEffect(() => {
    setOver(new Map());
    setListed(new Set());
  }, [at]);
  const on = (it: JanitorItem) => !it.inUse && (over.get(it.id) ?? it.checked);
  const toggle = (it: JanitorItem) => setOver((m) => new Map(m).set(it.id, !on(it)));
  const setAll = (items: JanitorItem[], v: boolean) => setOver((m) => { const n = new Map(m); for (const i of items) n.set(i.id, v); return n; });
  const toggleListed = (it: JanitorItem) => setListed((s) => (s.has(it.id) ? new Set([...s].filter((x) => x !== it.id)) : new Set(s).add(it.id)));
  const batch = categories.filter((g) => !LITTER.has(g.id));
  const chosen = batch.flatMap((g) => g.items).filter(on);
  const chosenSize = chosen.reduce((s, i) => s + i.size, 0);
  const listedItems = groups.flatMap((g) => g.items).filter((i) => listed.has(i.id));
  const listedSize = listedItems.reduce((s, i) => s + i.size, 0);
  const docker = summary?.docker ?? null;
  const classes = useMemo(() => {
    const out: Klass[] = [];
    for (const [id, look] of CLEAN) {
      const gs = [...categories, ...groups].filter((g) => g.class === id);
      if (gs.length || (id === "docker" && docker)) out.push({ id, look, listed: !categories.some((g) => g.class === id), groups: gs, size: id === "docker" ? (docker?.size ?? 0) : gs.reduce((a, g) => a + g.size, 0) });
    }
    return out;
  }, [categories, groups, docker]);
  const refresh = () => qc.invalidateQueries({ queryKey: JANITOR_KEY });
  const run = async (label: string, f: () => Promise<JanitorOutcome>, verb: string, text?: (o: JanitorOutcome) => string) => {
    setBusy(label);
    try {
      const o = await f();
      toast(text && !o.failed.length ? text(o) : outcomeText(o, verb), o.failed.length ? "error" : "info");
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(null);
      await refresh();
    }
  };
  const clean = async () => {
    if (!chosen.length) return;
    if (!(await confirm({ title: `清理选中的 ${chosen.length} 项？`, message: `腾出约 ${bytes(chosenSize)}。直接删除，不进废纸篓。`, confirmLabel: "清理", danger: true }))) return;
    await run("clean", () => janitorApi.clean(chosen.map((i) => i.id)), "清理");
  };
  const cleanLitter = async (g: JanitorGroup, it: JanitorItem) => {
    if (!(await confirm({ title: `清理 ${it.tag} ${g.name}？`, message: `一共 ${bytes(it.size)}。直接删除，不进废纸篓。`, confirmLabel: "清理", danger: true }))) return;
    await run("litter", () => janitorApi.clean([it.id]), "清理", (o) => `清理了 ${it.tag} ${g.name}，腾出 ${bytes(o.freed)}`);
  };
  const toTrash = async () => {
    if (!listedItems.length) return;
    if (!(await confirm({ title: `把 ${listedItems.length} 项移到废纸篓？`, message: `一共 ${bytes(listedSize)}，在废纸篓里还能放回去。`, confirmLabel: "移到废纸篓" }))) return;
    const guarded = listedItems.filter((i) => i.protected);
    let ok = false;
    if (guarded.length) {
      ok = await confirm({
        title: `其中 ${guarded.length} 项在受保护的地方`,
        message: guarded.slice(0, 5).map((i) => `${i.title}（${i.protected}）`).join("、") + (guarded.length > 5 ? " 等" : "") + "。也移到废纸篓吗？",
        confirmLabel: "也移走",
        danger: true,
      });
    }
    const ids = listedItems.filter((i) => ok || !i.protected).map((i) => i.id);
    if (!ids.length) return;
    await run("trash", () => janitorApi.trash(ids, ok), "移走");
  };
  const emptyTrash = async () => {
    if (!summary) return;
    if (!(await confirm({ title: "清空废纸篓？", message: `${summary.trash.count} 项、${bytes(summary.trash.size)}，清空后找不回来。`, confirmLabel: "清空", danger: true }))) return;
    await run("empty", janitorApi.emptyTrash, "清空");
  };
  if (!summary) return <EmptyState title={props.ov.scan.running ? "正在扫描磁盘…" : "还没扫描"} />;
  const current = classes.find((c) => c.id === pick) ?? classes[0];
  const picked = (k: Klass) => (k.listed ? k.groups.flatMap((g) => g.items).filter((i) => listed.has(i.id)).length : k.groups.filter((g) => !LITTER.has(g.id)).flatMap((g) => g.items).filter(on).length);
  const sideRow = (k: Klass, i: number) => {
    const n = picked(k);
    const here = k.id === current?.id;
    return (
      <button key={k.id} type="button" aria-current={here} onClick={() => setPick(k.id)} className={`side-row w-full gap-2.5 px-2.5 py-1.5 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${here ? "on" : ""}`}>
        {here && <Picked seed={1500 + i} />}
        <Badge look={k.look} small tilt={i % 2 ? 5 : -5} count={n} />
        <span className="min-w-0 flex-1 truncate" title={n ? `选了 ${n} 项` : undefined}>{k.look.name}</span>
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{bytes(k.size)}</span>
      </button>
    );
  };
  const oneClick = classes.filter((k) => !k.listed);
  const lists = classes.filter((k) => k.listed);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {confirmDialog}
      <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-card border bg-card px-4 py-3">
        <span className="text-base font-bold">一键清理</span>
        <span className="text-sm text-muted-foreground tabular-nums">选了 {chosen.length} 项 · {bytes(chosenSize)}</span>
        <Button variant="primary" disabled={!chosen.length || !!busy} onClick={() => void clean()}>{busy === "clean" ? "正在清理…" : "清理选中的"}</Button>
        {listedItems.length > 0 && (
          <>
            <span className="ml-2 text-sm text-muted-foreground tabular-nums">只列出来的选了 {listedItems.length} 项 · {bytes(listedSize)}</span>
            <Button disabled={!!busy} onClick={() => void toTrash()}>{busy === "trash" ? "正在移走…" : "移到废纸篓"}</Button>
          </>
        )}
        <Button className="ml-auto" disabled={!summary.trash.size || !!busy} onClick={() => void emptyTrash()}>{busy === "empty" ? "正在清空…" : `清空废纸篓（${bytes(summary.trash.size)}）`}</Button>
      </div>
      {!classes.length ? (
        <EmptyState title="没有能清的" />
      ) : (
        <div className="flex min-h-0 flex-1">
          <nav aria-label="清理的分类" className="side-list flex w-60 shrink-0 flex-col pr-4">
            <div className="-m-1 min-h-0 flex-1 space-y-0.5 overflow-y-auto p-1">
              {oneClick.length > 0 && <p className="px-2.5 pt-1 pb-1 text-xs text-muted-foreground">一键清理</p>}
              {oneClick.map((k, i) => sideRow(k, i))}
              {oneClick.length > 0 && lists.length > 0 && <div className="side-rule" />}
              {lists.length > 0 && <p className="px-2.5 pb-1 text-xs text-muted-foreground">只列出来</p>}
              {lists.map((k, i) => sideRow(k, i + oneClick.length))}
            </div>
          </nav>
          {current && (
            <div className="selectable -m-1 min-h-0 min-w-0 flex-1 overflow-y-auto p-1 pl-6">
              <div className="mb-4 flex items-center gap-3 px-1">
                <Badge look={current.look} />
                <h2 className="text-lg font-bold">{current.look.name}</h2>
                <span className="text-sm text-muted-foreground tabular-nums">{bytes(current.size)}</span>
              </div>
              {current.id === "docker" && docker ? (
                <section className="mb-5 rounded-card border bg-card">
                  {docker.parts?.length ? (
                    <ul className="divide-y">
                      {docker.parts.map((p) => (
                        <li key={p.name} className="flex items-baseline gap-2.5 px-4 py-2 text-sm">
                          <span className="min-w-0 flex-1">{p.name}</span>
                          <span className="text-xs text-muted-foreground tabular-nums">能回收 {bytes(p.reclaimable)}</span>
                          <span className="w-20 text-right tabular-nums">{bytes(p.size)}</span>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <p className="px-4 py-2 text-sm text-muted-foreground">Docker 没开，只知道一共多大</p>
                  )}
                </section>
              ) : (
                current.groups.map((g) =>
                  LITTER.has(g.id) ? (
                    <LitterCard key={g.id} g={g} busy={!!busy} clean={(it) => void cleanLitter(g, it)} />
                  ) : !current.listed ? (
                    <CategoryCard key={g.id} g={g} home={home} on={on} toggle={toggle} set={setAll} />
                  ) : g.id === "duplicates" ? (
                    <DupCard key={g.id} g={g} home={home} picked={listed} toggle={toggleListed} />
                  ) : (
                    <ListedCard key={g.id} g={g} home={home} picked={listed} toggle={toggleListed} titled={current.groups.length > 1} />
                  ),
                )
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ---- 环境 ----

function EnvRow(props: { it: JanitorEnvItem; home: string }) {
  const it = props.it;
  const extra = [it.packages != null ? `${it.packages} ${it.unit ?? "个包"}` : null, it.top.length ? `最大：${it.top.map(([n, s]) => `${n} ${bytes(s)}`).join("、")}` : null].filter(Boolean).join(" · ");
  return (
    <li className={`group flex items-start gap-2 py-2 pr-4 text-sm ${it.part ? "pl-9" : "pl-4"}`}>
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
          <span className={`min-w-0 truncate ${it.part ? "text-muted-foreground" : ""}`}>{it.title}</span>
          {it.version && !it.title.includes(it.version) && <span className="shrink-0 text-xs text-muted-foreground">{it.version}</span>}
          {it.default && <span className="shrink-0 rounded-full bg-accent px-1.5 text-xs text-primary">默认</span>}
          {it.tag && <span className="shrink-0 text-xs text-muted-foreground">{it.tag}</span>}
          {it.protected && <span className="shrink-0 rounded-full bg-accent px-1.5 text-xs text-primary">受保护 · {it.protected}</span>}
          {it.admin && <span className="shrink-0 text-xs text-muted-foreground">删要管理员密码</span>}
        </div>
        {(it.by || it.path) && (
          <div className="flex min-w-0 items-baseline gap-x-2 text-xs text-muted-foreground">
            {it.by && <span className="shrink-0">{it.by}</span>}
            {it.path && <span className="min-w-0 truncate font-mono" title={it.path}>{shortPath(it.path, props.home)}</span>}
          </div>
        )}
        {extra && <div className="text-xs text-muted-foreground">{extra}</div>}
      </div>
      {it.path && <Reveal path={it.path} />}
      <span className="shrink-0 pt-px tabular-nums">{bytes(it.size)}</span>
    </li>
  );
}

/** `plain`: no header (a card of one row, or the only card of its section, says it already). */
function EnvCard(props: { e: JanitorEnv; home: string; plain: boolean }) {
  const { e } = props;
  return (
    <section className="mb-4 rounded-card border bg-card">
      {!props.plain && <div className="flex flex-wrap items-baseline gap-x-2 border-b px-4 py-2.5">
        <span className="font-bold">{e.name}</span>
        {e.note && <span className="min-w-0 truncate text-xs text-muted-foreground">{e.note}</span>}
        <span className="ml-auto shrink-0 tabular-nums">{bytes(e.size)}</span>
      </div>}
      <ul className="divide-y">{e.items.map((it, k) => <EnvRow key={`${it.path ?? it.title}:${k}`} it={it} home={props.home} />)}</ul>
    </section>
  );
}

/** Cards into two columns, each to the shorter one (by rows), so neither runs much longer. */
function twoColumns(envs: JanitorEnv[]): [JanitorEnv[], JanitorEnv[]] {
  const cols: [JanitorEnv[], JanitorEnv[]] = [[], []];
  const h = [0, 0];
  for (const e of envs) {
    const k = h[0] <= h[1] ? 0 : 1;
    cols[k].push(e);
    h[k] += 2 + e.items.length;
  }
  return cols;
}

export function EnvTab(props: { ov: JanitorOverview }) {
  const at = props.ov.summary?.scannedAt ?? 0;
  const envs = useQuery({ queryKey: [...JANITOR_KEY, "envs", at], queryFn: janitorApi.envs, enabled: !!props.ov.summary });
  const sections = useMemo(
    () =>
      ENV.map(([id, look]) => {
        const list = (envs.data ?? []).filter((e) => (e.class || "tools") === id).sort((a, b) => b.size - a.size);
        return { id, look, list, size: list.reduce((a, e) => a + e.size, 0) };
      }).filter((s) => s.list.length),
    [envs.data],
  );
  if (!props.ov.summary) return <EmptyState title={props.ov.scan.running ? "正在扫描磁盘…" : "还没扫描"} />;
  if (!envs.data) return null;
  if (!envs.data.length) return <EmptyState title={props.ov.scan.running ? "正在认开发环境…" : "还没认出开发环境"} />;
  const jump = (id: string) => document.getElementById(`jn-env-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  return (
    <div className="jn-envs">
      <nav aria-label="开发环境的分类" className="mb-6 flex flex-wrap gap-x-2 gap-y-2.5">
        {sections.map((s, i) => (
          <button key={s.id} type="button" onClick={() => jump(s.id)} className="flex items-center gap-2 rounded-full py-1 pr-3 pl-1.5 text-sm outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring">
            <Badge look={s.look} small tilt={i % 2 ? 5 : -5} />
            <span>{s.look.name}</span>
            <span className="text-xs text-muted-foreground tabular-nums">{bytes(s.size)}</span>
          </button>
        ))}
      </nav>
      {sections.map((s) => (
        <section key={s.id} id={`jn-env-${s.id}`} className="mb-6 scroll-mt-2">
          <div className="mb-3 flex items-center gap-3 px-1">
            <Badge look={s.look} />
            <h2 className="text-lg font-bold">{s.look.name}</h2>
            <span className="text-sm text-muted-foreground tabular-nums">{s.list.length} 个 · {bytes(s.size)}</span>
          </div>
          <div className="jn-cols">
            {twoColumns(s.list).map((col, k) => (
              <div key={k} className="jn-col">{col.map((e) => <EnvCard key={e.id} e={e} home={props.ov.home} plain={!e.note && (e.items.length === 1 || s.list.length === 1)} />)}</div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

/** A category's class look (settings lists them with their sticker). */
export function ClassBadge(props: { id: string }) {
  const look = CLEAN_LOOK.get(props.id);
  return look ? <Badge look={look} small /> : null;
}
