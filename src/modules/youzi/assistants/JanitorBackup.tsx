// DevJanitor 备份 (1.1.0 Phase 1; Rust janitor/backup/): ~/Downloads' folders → the external drive, only what
// changed; the drive's folders → PikPak. The bar switches 备份 / 清单 and says which drive and whether it's in.
// 清单 (1.1.2): the drive's top folders and ~/Downloads', ticked ones backed up (a folder ~/Downloads has goes to the
// drive folder of its name). 备份 (1.1.3): two lists side by side, each card titled inside its frame. Downloads: the
// ticked folders against the drive (those with something to copy are ticked to copy; each folds open to what the
// comparison found — new, changed (the drive's copy goes to ARCHIVE), renamed or moved, held, only on the drive);
// renames are done on the drive only when picked here. PikPak (backup/pikpak.rs): the ticked drive folders against
// PikPak (to upload, moved up there, into ARCHIVE), synced only when I press 开始同步; logging in sits at its top
// right. How a run or a sync ended pops up as a note, once. The page asks every few seconds, so plugging the drive
// in shows up.
// 柚子 (Phase 2; backup/look.rs): after a drive is plugged in, DevJanitor's Agent looks at what the comparison
// and the clues say; its reply and cards (JanitorCards.tsx) show here and in Agent. 柚子看一眼 asks again.
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, HardDrive, Settings2, Trash2 } from "@/shared/icons";
import { Section } from "@/settings/layout";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { Button, EmptyState, Field, IconButton, ProgressBar, Segmented, TextInput } from "@/shared/ui";
import { pad } from "@/shared/dates";
import { subNav } from "@/app/subpage";
import { MarkdownView } from "../MarkdownView";
import { JanitorCards } from "./JanitorCards";
import { BACKUP_KEY, backupApi, bytes, type BackupDiff, type BackupListRow, type BackupLook, type BackupOverview, type BackupRecord, type BackupTask, type CloudDiff, type CloudFolder, type PikPakView } from "./janitorApi";

/** 备份 (copying, 柚子, records) or 清单 (what's backed up). */
export type BackupView = "run" | "list";

export function useBackup() {
  return useQuery({
    queryKey: BACKUP_KEY,
    queryFn: backupApi.overview,
    refetchOnMount: "always",
    refetchInterval: (q) => {
      const d = q.state.data;
      return d?.run.running ? 700 : d?.comparing || d?.look?.state === "running" || d?.pikpak.running || d?.pikpak.comparing ? 1000 : 4000;
    },
  });
}

/** 「2026-11-25T12:03:00Z」 (or unix seconds) → 11-25 20:03 (local). */
function when(stamp: string | number | null): string {
  if (!stamp) return "";
  const d = new Date(typeof stamp === "number" ? stamp * 1000 : stamp);
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const toCopy = (d: BackupDiff | null) => (d ? { files: d.new.count + d.changed.count, bytes: d.new.bytes + d.changed.bytes } : { files: 0, bytes: 0 });
const toSync = (d: CloudDiff | null) => (d ? { files: d.new.count + d.changed.count, bytes: d.new.bytes + d.changed.bytes, other: d.moved.count + d.extra.count } : { files: 0, bytes: 0, other: 0 });

/** The top bar on 备份: 备份 / 清单, the drive, and comparing again. */
export function BackupBar(props: { view: BackupView; setView: (v: BackupView) => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const q = useBackup();
  const ov = q.data;
  if (!ov) return null;
  const d = ov.drive;
  const pick = (uuid: string | null) => backupApi.drive(uuid).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"));
  return (
    <>
    {d && <Segmented<BackupView> ariaLabel="备份或清单" value={props.view} options={[{ value: "run", label: "备份" }, { value: "list", label: "清单" }]} onChange={props.setView} />}
    <div className="ml-auto flex min-w-0 items-center gap-3 text-sm">
      {ov.mode === "sandbox" && <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">假 Mac</span>}
      {d && (
        <span className="flex min-w-0 items-center gap-1.5 text-muted-foreground tabular-nums">
          <HardDrive size={14} className="shrink-0" />
          <span className="min-w-0 truncate">{d.connected ? `${d.name} · 剩 ${bytes(d.free)} / ${bytes(d.total)}` : `${d.name} 没插上`}</span>
        </span>
      )}
      {d && ov.volumes.length > (d.connected ? 1 : 0) && (
        <Dropdown
          ariaLabel="选移动硬盘"
          plain
          value={d?.uuid ?? ""}
          options={ov.volumes.map((v) => ({ value: v.uuid, label: `${v.name} · ${v.fs} · ${bytes(v.total)}` }))}
          onChange={(u) => void pick(u)}
          renderTrigger={() => <span className="text-sm">换一块</span>}
        />
      )}
      {d?.connected && (
        <Button disabled={ov.comparing || ov.run.running || ov.pikpak.running} onClick={() => void backupApi.compare().then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"))}>
          {ov.comparing ? "正在对比…" : "重新对比"}
        </Button>
      )}
      {d?.connected && (
        <Button disabled={ov.comparing || ov.run.running || ov.look?.state === "running"} onClick={() => void backupApi.look().then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"))}>
          柚子看一眼
        </Button>
      )}
    </div>
    </>
  );
}

export function BackupTab(props: { view: BackupView }) {
  const q = useBackup();
  useEndNotes(q.data);
  if (q.isError) return <EmptyState title="备份暂时打不开" hint={errorText(q.error)} action={<Button onClick={() => void q.refetch()}>重试</Button>} />;
  if (!q.data) return <EmptyState title="正在读取…" />;
  const ov = q.data;
  if (!ov.drive) return <PickDrive ov={ov} />;
  return props.view === "list" ? <ListBody ov={ov} /> : <BackupBody ov={ov} />;
}

/** Runs and syncs seen ended (-1: not yet looked; those before the page first opened aren't popped). */
const seenEnds = { drive: -1, pikpak: -1 };
const fine = (t: string, ok: string[]) => ok.some((w) => t.startsWith(w)) && !t.includes("没成");

/** How a run or a sync ended, popped as a note once. */
function useEndNotes(ov: BackupOverview | undefined) {
  const toast = useToast();
  const [drive, cloud] = [ov?.run.ends, ov?.pikpak.ends];
  useEffect(() => {
    if (!ov) return;
    const ends: ["drive" | "pikpak", number, string | null, boolean][] = [
      ["drive", ov.run.ends, ov.run.ended, fine(ov.run.ended ?? "", ["备份好了", "停下了"])],
      ["pikpak", ov.pikpak.ends, ov.pikpak.ended && `PikPak ${ov.pikpak.ended}`, fine(ov.pikpak.ended ?? "", ["同步好了", "和硬盘上一样", "停下了"])],
    ];
    for (const [k, n, text, ok] of ends) {
      if (seenEnds[k] >= 0 && n > seenEnds[k] && text) toast(text, ok ? "info" : "error");
      seenEnds[k] = n;
    }
  }, [drive, cloud]);
}

function PickDrive(props: { ov: BackupOverview }) {
  const qc = useQueryClient();
  const toast = useToast();
  const vols = props.ov.volumes;
  if (!vols.length) return <EmptyState title="移动硬盘还没来" />;
  return (
    <EmptyState
      title="用哪块移动硬盘备份？"
      action={
        <Dropdown
          ariaLabel="选移动硬盘"
          value=""
          placeholder="选移动硬盘"
          options={vols.map((v) => ({ value: v.uuid, label: `${v.name} · ${v.fs} · ${bytes(v.total)}` }))}
          onChange={(u) => void backupApi.drive(u).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"))}
        />
      }
    />
  );
}

/** A list's head inside its card: the title, where it stands, its buttons at the right. */
function CardHead(props: { title: string; status: ReactNode; danger?: boolean; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3">
      <h2 className="text-lg font-bold text-foreground">{props.title}</h2>
      <span className={`min-w-0 flex-1 truncate text-sm tabular-nums ${props.danger ? "text-destructive" : "text-muted-foreground"}`}>{props.status}</span>
      {props.children && <div className="ml-auto flex shrink-0 items-center gap-1.5">{props.children}</div>}
    </div>
  );
}

function BackupBody(props: { ov: BackupOverview }) {
  const { ov } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const forget = async (r: BackupRecord) => {
    if (!(await confirm({ title: "删掉这条记录？", message: "只删这条记录，硬盘和 PikPak 上的文件不动。", confirmLabel: "删除", danger: true }))) return;
    backupApi.forget(r.id).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"));
  };
  return (
    <div>
      {confirmDialog}
      <div className="mb-6 grid items-start gap-4 [grid-template-columns:repeat(auto-fit,minmax(min(100%,420px),1fr))]">
        <DownloadsCard ov={ov} />
        <PikPakCard ov={ov} />
      </div>
      {ov.look && <LookCard look={ov.look} />}
      {ov.runs.length > 0 && (
        <Section title="记录">
          <ul className="divide-y">{ov.runs.map((r) => <RecordRow key={r.id} r={r} onForget={() => void forget(r)} />)}</ul>
        </Section>
      )}
    </div>
  );
}

/** Downloads → the drive: the ticked folders, what each would copy, 开始备份. */
function DownloadsCard(props: { ov: BackupOverview }) {
  const { ov } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [over, setOver] = useState<Map<number, boolean>>(new Map());
  const on = (t: BackupTask) => over.get(t.id) ?? toCopy(t.diff).files > 0;
  const picked = ov.tasks.filter((t) => on(t) && !t.problem && t.diff);
  const total = picked.reduce((a, t) => ({ files: a.files + toCopy(t.diff).files, bytes: a.bytes + toCopy(t.diff).bytes }), { files: 0, bytes: 0 });
  const run = ov.run;
  const ready = !!ov.drive?.connected && !ov.comparing && !run.running;
  const start = () => backupApi.start(picked.map((t) => t.id)).then((o) => { qc.setQueryData(BACKUP_KEY, o); setOver(new Map()); }, (e) => toast(errorText(e), "error"));
  const status = run.running
    ? `${run.filesDone} / ${run.filesTotal} 个 · ${bytes(run.bytesDone)} / ${bytes(run.bytesTotal)} · ${run.task}`
    : ov.tasks.some((t) => toCopy(t.diff).files > 0)
      ? `选了 ${picked.length} 个文件夹 · 要复制 ${total.files} 个、${bytes(total.bytes)}`
      : ov.comparing
        ? "正在对比…"
        : ov.tasks.length && ov.drive?.connected
          ? "都和硬盘上一样"
          : "";
  return (
    <section className="rounded-card border bg-card">
      <CardHead title="Downloads" status={status}>
        {run.running ? <Button onClick={() => void backupApi.stop()}>停下</Button> : <Button variant="primary" disabled={!ready || !picked.length} onClick={() => void start()}>开始备份</Button>}
      </CardHead>
      {run.running && (
        <div className="space-y-1 px-4 pb-3">
          <ProgressBar done={run.bytesDone} total={Math.max(run.bytesTotal, 1)} />
          {run.current && <p className="truncate text-xs text-muted-foreground">{run.current}</p>}
        </div>
      )}
      {ov.tasks.length ? (
        <ul className="divide-y border-t">
          {ov.tasks.map((t) => (
            <TaskRow key={t.id} ov={ov} t={t} on={on(t)} toggle={() => setOver((m) => new Map(m).set(t.id, !on(t)))} />
          ))}
        </ul>
      ) : (
        <p className="border-t px-4 py-3 text-sm text-muted-foreground">清单里没勾 Downloads 里的文件夹</p>
      )}
    </section>
  );
}

/** 清单: the drive's top folders and ~/Downloads', a tick each (ticked: backed up). */
function ListBody(props: { ov: BackupOverview }) {
  const { ov } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);
  if (!ov.drive?.connected) return <EmptyState title={`${ov.drive?.name ?? "移动硬盘"} 没插上`} />;
  const pick = (r: BackupListRow) => {
    setBusy(r.name);
    backupApi.pick(r.name, !r.picked).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error")).finally(() => setBusy(null));
  };
  const where = (r: BackupListRow) => (r.onDrive && r.onMac ? "硬盘和 Downloads" : r.onDrive ? "只在硬盘上" : "只在 Downloads 里");
  const ticked = ov.list.filter((r) => r.picked);
  return (
    <section className="rounded-card border bg-card">
      <CardHead title="清单" status={`勾了 ${ticked.length} / ${ov.list.length} 个${ticked.some((r) => r.bytes != null) ? ` · ${bytes(ticked.reduce((a, r) => a + (r.bytes ?? 0), 0))}` : ""}`} />
      <ul className="divide-y border-t">
        {ov.list.map((r) => (
          <li key={r.name}>
            <label className="flex cursor-pointer items-center gap-3 px-4 py-2.5 text-sm">
              <input type="checkbox" checked={r.picked} disabled={ov.run.running || ov.pikpak.running || busy != null} onChange={() => pick(r)} aria-label={r.name} className="size-3.5 shrink-0 accent-[var(--primary)]" />
              <span className={`min-w-0 truncate ${r.picked ? "" : "text-muted-foreground"}`}>{r.name}</span>
              <span className="shrink-0 text-xs text-muted-foreground">{where(r)}</span>
              <span className="ml-auto shrink-0 text-xs text-muted-foreground tabular-nums">{r.files != null && r.bytes != null ? `${r.files} 个 · ${bytes(r.bytes)}` : ""}</span>
            </label>
          </li>
        ))}
      </ul>
    </section>
  );
}

function TaskRow(props: { ov: BackupOverview; t: BackupTask; on: boolean; toggle: () => void }) {
  const { ov, t } = props;
  const [open, setOpen] = useState(false);
  const d = t.diff;
  const c = toCopy(d);
  const counts = d
    ? [
        [`新加 ${d.new.count}`, d.new.count],
        [`改过 ${d.changed.count}`, d.changed.count],
        [`改名或挪了 ${d.moved.count}`, d.moved.count],
        [`不覆盖 ${d.held.count}`, d.held.count],
        [`电脑上刚删的 ${d.recent.count}`, d.recent.count],
        [`只在硬盘上 ${d.archived.count}`, d.archived.count],
      ].filter(([, n]) => n).map(([s]) => s as string)
    : [];
  return (
    <li className="px-4 py-2.5 text-sm">
      <div className="flex items-start gap-2.5">
        <input type="checkbox" checked={props.on && !t.problem && !!d} disabled={!!t.problem || !d || ov.run.running} onChange={props.toggle} aria-label={t.target} className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]" />
        <div className="min-w-0 flex-1">
          <p className="min-w-0 truncate">{t.target}</p>
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground">
            {t.problem ? <span className="text-destructive">{t.problem}</span> : d ? <span>{counts.length ? counts.join(" · ") : "和硬盘上一样"}</span> : <span>{ov.drive?.connected ? "正在对比…" : "插上移动硬盘才能对比"}</span>}
            {t.last && <span>· 上次 {when(t.last)}</span>}
          </div>
          {d && counts.length > 0 && <Unfold open={open} toggle={() => setOpen((o) => !o)} />}
        </div>
        {c.files > 0 && <span className="shrink-0 pt-px tabular-nums">{bytes(c.bytes)}</span>}
      </div>
      {open && d && <DiffView ov={ov} t={t} d={d} />}
    </li>
  );
}

function Unfold(props: { open: boolean; toggle: () => void }) {
  return (
    <button type="button" onClick={props.toggle} className="mt-1 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
      <ChevronRight size={12} className={`transition-transform ${props.open ? "rotate-90" : ""}`} />
      {props.open ? "收起" : "看看有什么"}
    </button>
  );
}

function Rows(props: { title: string; note?: string; count: number; size: number; rows: { key: string; left: string; right: string }[] }) {
  if (!props.count) return null;
  return (
    <div>
      <p className="text-xs text-muted-foreground">
        {props.title} · {props.count} 个 · {bytes(props.size)}
        {props.note && ` · ${props.note}`}
      </p>
      <ul className="selectable mt-1 max-h-56 overflow-y-auto">
        {props.rows.map((r) => (
          <li key={r.key} className="flex items-baseline gap-2 py-0.5 text-xs">
            <span className="min-w-0 flex-1 truncate font-mono">{r.left}</span>
            <span className="shrink-0 text-muted-foreground tabular-nums">{r.right}</span>
          </li>
        ))}
        {props.count > props.rows.length && <li className="py-0.5 text-xs text-muted-foreground">还有 {props.count - props.rows.length} 个</li>}
      </ul>
    </div>
  );
}

function DiffView(props: { ov: BackupOverview; t: BackupTask; d: BackupDiff }) {
  const { t, d } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [skip, setSkip] = useState<Set<string>>(new Set());
  const moves = d.moved.rows.filter((m) => !skip.has(m.to));
  const row = (r: { path: string; size: number }) => ({ key: r.path, left: r.path, right: bytes(r.size) });
  const follow = async () => {
    if (!moves.length) return;
    if (!(await confirm({ title: `在移动硬盘上跟着改 ${moves.length} 个？`, message: "把硬盘上的这些文件挪到电脑上现在的位置和名字，不重新复制。", confirmLabel: "跟着改" }))) return;
    backupApi.move(t.id, moves.map((m) => m.to)).then(
      (r) => {
        qc.setQueryData(BACKUP_KEY, r.overview);
        toast(r.failed.length ? `改了 ${r.done} 个；${r.failed.length} 个没成：${r.failed.slice(0, 3).map(([p, why]) => `${p}（${why}）`).join("、")}` : `改了 ${r.done} 个`, r.failed.length ? "error" : "info");
      },
      (e) => toast(errorText(e), "error"),
    );
  };
  return (
    <div className="mt-2 ml-6 space-y-3 border-l pl-3">
      {confirmDialog}
      <Rows title="新加的" count={d.new.count} size={d.new.bytes} rows={d.new.rows.map(row)} />
      <Rows title="改过的" note="硬盘上那份挪进 ARCHIVE" count={d.changed.count} size={d.changed.bytes} rows={d.changed.rows.map(row)} />
      {d.moved.count > 0 && (
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs text-muted-foreground">改了名或挪了位置 · {d.moved.count} 个 · {bytes(d.moved.bytes)} · 不复制，等你决定</p>
            <Button className="ml-auto" disabled={!moves.length || props.ov.run.running} onClick={() => void follow()}>在硬盘上跟着改（{moves.length}）</Button>
          </div>
          <ul className="selectable mt-1 max-h-56 overflow-y-auto">
            {d.moved.rows.map((m) => (
              <li key={m.to} className="flex items-baseline gap-2 py-0.5 text-xs">
                <input type="checkbox" checked={!skip.has(m.to)} onChange={() => setSkip((s) => { const n = new Set(s); if (n.has(m.to)) n.delete(m.to); else n.add(m.to); return n; })} aria-label={m.to} className="size-3 shrink-0 accent-[var(--primary)]" />
                <span className="min-w-0 flex-1 truncate font-mono">
                  <span className="text-muted-foreground">{m.from}</span> → {m.to}
                </span>
                <span className="shrink-0 text-muted-foreground tabular-nums">{bytes(m.size)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      <Rows title="不覆盖" count={d.held.count} size={d.held.bytes} rows={d.held.rows.map((h) => ({ key: h.path, left: h.path, right: h.why }))} />
      <Rows title="电脑上刚删的" note="上次备份时电脑上还有，硬盘上留着" count={d.recent.count} size={d.recent.bytes} rows={d.recent.rows.map(row)} />
      <Rows title="只在硬盘上" note="留着" count={d.archived.count} size={d.archived.bytes} rows={d.archived.rows.map(row)} />
      {d.denied.length > 0 && <p className="text-xs text-destructive">读不了：{d.denied.join("、")}</p>}
    </div>
  );
}

/** 柚子's last look: its reply, then its cards (the same ones as in Agent). */
function LookCard(props: { look: BackupLook }) {
  const l = props.look;
  const running = l.state === "running";
  // on the paper like a reply in Agent (its cards are cards of their own)
  return (
    <section className="mb-6">
      <div className="mb-2 flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-1">
        <h2 className="text-sm text-muted-foreground">柚子看了一眼</h2>
        <div className="flex shrink-0 items-center gap-2">
          <span className="text-xs text-muted-foreground tabular-nums">{when(l.at)}{l.stale ? " · 之后有变化" : ""}</span>
          <Button onClick={() => subNav("janitor")?.("agent")}>在 Agent 里接着聊</Button>
        </div>
      </div>
      <div className="selectable px-1">
        {l.text && <MarkdownView source={l.text} streaming={running} />}
        {running && !l.text && <p className="text-sm text-muted-foreground">正在看…</p>}
        {l.state === "failed" && <p className="text-sm text-destructive">没看成：{l.error ?? "不知道为什么"}</p>}
        {l.cards.length > 0 && (
          <div className="grid items-start gap-x-4 [grid-template-columns:repeat(auto-fill,minmax(min(100%,400px),1fr))]">
            <JanitorCards cards={l.cards} streaming={running} fill />
          </div>
        )}
      </div>
    </section>
  );
}

/** 「2 小时 5 分」「12 分」「40 秒」. */
function left(s: number): string {
  if (s >= 3600) return `${Math.floor(s / 3600)} 小时${Math.floor((s % 3600) / 60) ? ` ${Math.floor((s % 3600) / 60)} 分` : ""}`;
  if (s >= 60) return `${Math.round(s / 60)} 分`;
  return `${s} 秒`;
}

const LIMITS = [0, 1, 2, 5, 10, 20, 50];
const limitText = (n: number) => (n ? `${n} MB/s` : "不限速");

/** PikPak: the ticked drive folders against PikPak, what each would do, 开始同步 / 停下; logging in and its settings at the top right. */
function PikPakCard(props: { ov: BackupOverview }) {
  const { ov } = props;
  const p = ov.pikpak;
  const qc = useQueryClient();
  const toast = useToast();
  const [dialog, setDialog] = useState<"login" | "settings" | null>(null);
  const [over, setOver] = useState<Map<string, boolean>>(new Map());
  const busy = (f: CloudFolder) => toSync(f.diff).files + toSync(f.diff).other > 0;
  const on = (f: CloudFolder) => over.get(f.name) ?? (busy(f) && !f.diff?.clash.count);
  const picked = p.folders.filter((f) => on(f) && f.diff);
  const total = picked.reduce((a, f) => ({ files: a.files + toSync(f.diff).files, bytes: a.bytes + toSync(f.diff).bytes }), { files: 0, bytes: 0 });
  const ready = !!p.account && !p.waiting && !p.comparing && !p.running && !p.error && !ov.run.running;
  const fail = (e: unknown) => toast(errorText(e), "error");
  const start = () => backupApi.pikpakStart(picked.map((f) => f.name)).then((o) => { qc.setQueryData(BACKUP_KEY, o); setOver(new Map()); }, fail);
  const stop = () => void backupApi.pikpakStop().then(() => qc.invalidateQueries({ queryKey: BACKUP_KEY }), fail);
  const status = !p.installed
    ? "没装 rclone：在终端里跑 brew install rclone"
    : !p.account
      ? "没登录"
      : p.running
        ? p.checking
          ? `正在对比 ${p.syncing.join("、")}…`
          : `${p.files} / ${p.totalFiles} 个 · ${bytes(p.bytes)} / ${bytes(p.totalBytes)}${p.speed ? ` · ${bytes(p.speed)}/s` : ""}${p.eta ? ` · 还要 ${left(p.eta)}` : ""}`
        : p.comparing
          ? "正在和 PikPak 对比…"
          : (p.error ?? p.waiting ?? (p.folders.some(busy) ? `选了 ${picked.length} 个文件夹 · 要传 ${total.files} 个、${bytes(total.bytes)}` : p.folders.length && p.comparedAt ? "都和硬盘上一样" : ""));
  const facts = p.account ? [p.account, `PikPak 上的「${p.folder}」${p.missing ? "（还没有，同步时新建）" : ""}`, limitText(p.limit), p.used != null && p.total ? `用了 ${bytes(p.used)} / ${bytes(p.total)}` : ""].filter(Boolean) : [];
  return (
    <section className="rounded-card border bg-card">
      <CardHead title="PikPak" status={status} danger={!!p.error && !p.running && !p.comparing}>
        {p.installed && !p.account && <Button variant="primary" onClick={() => setDialog("login")}>登录</Button>}
        {p.account && (p.running ? <Button onClick={stop}>停下</Button> : <Button variant="primary" disabled={!ready || !picked.length} onClick={() => void start()}>开始同步</Button>)}
        {p.account && <IconButton label="PikPak 的设置" tip="设置" disabled={p.running} onClick={() => setDialog("settings")}><Settings2 size={14} /></IconButton>}
      </CardHead>
      {(p.running || facts.length > 0) && (
        <div className="space-y-1 px-4 pb-3">
          {p.running && !p.checking && p.totalBytes > 0 && <ProgressBar done={p.bytes} total={p.totalBytes} />}
          {p.running && p.current.length > 0 && <p className="truncate text-xs text-muted-foreground">{p.current.join("、")}</p>}
          {facts.length > 0 && <p className="selectable truncate text-xs text-muted-foreground">{facts.join(" · ")}</p>}
        </div>
      )}
      {p.folders.length ? (
        <ul className="divide-y border-t">
          {p.folders.map((f) => (
            <CloudRow key={f.name} p={p} f={f} on={on(f)} locked={!ready} toggle={() => setOver((m) => new Map(m).set(f.name, !on(f)))} />
          ))}
        </ul>
      ) : (
        <p className="border-t px-4 py-3 text-sm text-muted-foreground">清单里没勾硬盘上的文件夹</p>
      )}
      {dialog === "login" && <PikPakLogin p={p} close={() => setDialog(null)} />}
      {dialog === "settings" && <PikPakSettings p={p} close={() => setDialog(null)} />}
    </section>
  );
}

function CloudRow(props: { p: PikPakView; f: CloudFolder; on: boolean; locked: boolean; toggle: () => void }) {
  const { p, f } = props;
  const [open, setOpen] = useState(false);
  const d = f.diff;
  const t = toSync(d);
  const counts = d
    ? [
        [`要传 ${t.files}`, t.files],
        [`在 PikPak 上挪 ${d.moved.count}`, d.moved.count],
        [`挪进 ARCHIVE ${d.extra.count}`, d.extra.count],
        [`PikPak 上会撞名 ${d.clash.count}`, d.clash.count],
      ].filter(([, n]) => n).map(([s]) => s as string)
    : [];
  const syncing = p.running && p.syncing.includes(f.name);
  const state = syncing ? "正在同步" : d ? (counts.length ? counts.join(" · ") : "和硬盘上一样") : !p.account ? "登录后对比" : p.comparing ? "正在对比…" : "还没对比";
  return (
    <li className="px-4 py-2.5 text-sm">
      <div className="flex items-start gap-2.5">
        <input type="checkbox" checked={props.on && !!d} disabled={!d || props.locked} onChange={props.toggle} aria-label={f.name} className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]" />
        <div className="min-w-0 flex-1">
          <p className="min-w-0 truncate">{f.name}</p>
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground">
            <span>{state}</span>
            {f.last && <span>· 上次 {when(f.last)}</span>}
          </div>
          {d && counts.length > 0 && <Unfold open={open} toggle={() => setOpen((o) => !o)} />}
        </div>
        {t.files > 0 && <span className="shrink-0 pt-px tabular-nums">{bytes(t.bytes)}</span>}
      </div>
      {open && d && (
        <div className="mt-2 ml-6 space-y-3 border-l pl-3">
          <Rows title="新加的" count={d.new.count} size={d.new.bytes} rows={d.new.rows.map((r) => ({ key: r.path, left: r.path, right: bytes(r.size) }))} />
          <Rows title="大小不一样的" note="PikPak 上那份挪进 ARCHIVE" count={d.changed.count} size={d.changed.bytes} rows={d.changed.rows.map((r) => ({ key: r.path, left: r.path, right: bytes(r.size) }))} />
          <Rows title="在 PikPak 上挪" note="不重新传" count={d.moved.count} size={d.moved.bytes} rows={d.moved.rows.map((m) => ({ key: m.to, left: `${m.from} → ${m.to}`, right: bytes(m.size) }))} />
          <Rows title="只在 PikPak 上" note="挪进 ARCHIVE" count={d.extra.count} size={d.extra.bytes} rows={d.extra.rows.map((r) => ({ key: r.path, left: r.path, right: bytes(r.size) }))} />
          <Rows title="传到 PikPak 上会撞名" note="改掉一个再同步" count={d.clash.count} size={d.clash.bytes} rows={d.clash.rows.map((r) => ({ key: r.path, left: r.path, right: bytes(r.size) }))} />
        </div>
      )}
    </li>
  );
}

function PikPakLogin(props: { p: PikPakView; close: () => void }) {
  const qc = useQueryClient();
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [folder, setFolder] = useState(props.p.folder);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      qc.setQueryData(BACKUP_KEY, await backupApi.pikpakLogin(account, password, folder));
      props.close();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="登录 PikPak" error={error} onClose={props.close} onSubmit={submit} submitLabel={busy ? "正在登录…" : "登录"} submitDisabled={busy || !account.trim() || !password || !folder.trim()}>
      <Field label="账号">
        <TextInput aria-label="PikPak 的账号" value={account} maxLength={200} onChange={(e) => setAccount(e.target.value)} />
      </Field>
      <Field label="密码">
        <TextInput type="password" aria-label="PikPak 的密码" value={password} maxLength={200} onChange={(e) => setPassword(e.target.value)} />
      </Field>
      <Field label="PikPak 上的文件夹">
        <TextInput aria-label="PikPak 上的文件夹" value={folder} maxLength={200} onChange={(e) => setFolder(e.target.value)} />
      </Field>
    </Modal>
  );
}

function PikPakSettings(props: { p: PikPakView; close: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [folder, setFolder] = useState(props.p.folder);
  const [limit, setLimit] = useState(props.p.limit);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      qc.setQueryData(BACKUP_KEY, await backupApi.pikpakSet(folder, limit));
      props.close();
    } catch (err) {
      setError(errorText(err));
    }
  };
  const logout = async () => {
    if (!(await confirm({ title: "退出 PikPak？", message: "PikPak 上的文件都不动。", confirmLabel: "退出", danger: true }))) return;
    backupApi.pikpakLogout().then((o) => { qc.setQueryData(BACKUP_KEY, o); props.close(); }, (err) => toast(errorText(err), "error"));
  };
  return (
    <Modal title="PikPak" error={error} onClose={props.close} onSubmit={submit} submitLabel="保存" submitDisabled={!folder.trim()}>
      {confirmDialog}
      <Field label="PikPak 上的文件夹">
        <TextInput aria-label="PikPak 上的文件夹" value={folder} maxLength={200} onChange={(e) => setFolder(e.target.value)} />
      </Field>
      <Field label="限速">
        <Dropdown<number> block ariaLabel="限速" value={limit} options={LIMITS.map((n) => ({ value: n, label: limitText(n) }))} onChange={setLimit} />
      </Field>
      <div className="flex items-center gap-2 pt-1 text-sm">
        <span className="min-w-0 flex-1 truncate text-muted-foreground">{props.p.account}</span>
        <Button onClick={() => void logout()}>退出登录</Button>
      </div>
    </Modal>
  );
}

const STATUS = { running: "还在备份", done: "备份好了", stopped: "停下了", failed: "没备成" } as const;
const PIKPAK_STATUS = { running: "还在传", done: "传好了", stopped: "停下了", failed: "没传完" } as const;

function RecordRow(props: { r: BackupRecord; onForget: () => void }) {
  const { r } = props;
  const [open, setOpen] = useState(false);
  const more = r.errors.length > 0 || !!r.note;
  return (
    <li className="group py-2 pr-2 pl-4 text-sm">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2">
        <span className="tabular-nums">{when(r.startedAt)}</span>
        <span className={r.status === "failed" ? "text-destructive" : "text-muted-foreground"}>{(r.dest === "pikpak" ? PIKPAK_STATUS : STATUS)[r.status]}</span>
        <span className="min-w-0 truncate text-muted-foreground">{r.dest === "pikpak" ? ["PikPak", ...r.tasks.map((t) => t.target)].join(" · ") : r.tasks.map((t) => t.target).join("、")}</span>
        <span className="ml-auto shrink-0 text-muted-foreground tabular-nums">{r.dest === "pikpak" ? "传了" : "复制"} {r.files} 个 · {bytes(r.bytes)}</span>
        {more && (
          <button type="button" onClick={() => setOpen((o) => !o)} className="shrink-0 text-xs text-muted-foreground hover:text-foreground">
            {r.errors.length ? `${r.errors.length} 个没成` : "详情"}
          </button>
        )}
        <span className={`shrink-0 ${r.status === "running" ? "invisible" : "opacity-0 group-hover:opacity-100 focus-within:opacity-100"}`}>
          <IconButton label="删掉这条记录" tip="删除" danger disabled={r.status === "running"} onClick={props.onForget}>
            <Trash2 size={14} />
          </IconButton>
        </span>
      </div>
      {open && (
        <ul className="selectable mt-1 max-h-48 space-y-0.5 overflow-y-auto text-xs text-muted-foreground">
          {r.note && <li>{r.note}</li>}
          {r.errors.map(([p, why], i) => (
            <li key={i} className="truncate font-mono">{p}：{why}</li>
          ))}
        </ul>
      )}
    </li>
  );
}
