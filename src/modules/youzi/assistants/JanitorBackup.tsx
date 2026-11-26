// DevJanitor 备份 (1.1.0 Phase 1; Rust janitor/backup/): a Mac folder → a folder on the external drive, only what
// changed. The bar says which drive and whether it's in; the run bar picks the tasks to back up (those with
// something to copy are ticked) and shows the progress; each task folds open to what the comparison found
// (new, changed — the drive's copy goes to 旧版 —, renamed or moved, held, only on the drive). Renames are done
// on the drive only when picked here. The page asks every few seconds, so plugging the drive in shows up.
// 柚子 (Phase 2; backup/look.rs): after a drive is plugged in, DevJanitor's Agent looks at what the comparison
// and the clues say; its reply and cards (JanitorCards.tsx) show here and in Agent. 柚子看一眼 asks again.
// PikPak (Phase 3; backup/pikpak.rs): the whole drive mirrored there in the background while it's in; its card
// says how far, stops and carries on; logging in and the folder / limit are dialogs.
// 不备份: drive folders kept out (no clues or cards about them, not synced to PikPak), picked from the drive's top folders.
import { useState, type FormEvent } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, HardDrive, Pencil, Plus, Settings2, Trash2 } from "@/shared/icons";
import { Section } from "@/settings/layout";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { Button, EmptyState, Field, IconButton, ProgressBar, TextInput } from "@/shared/ui";
import { pad } from "@/shared/dates";
import { subNav } from "@/app/subpage";
import { MarkdownView } from "../MarkdownView";
import { JanitorCards } from "./JanitorCards";
import { BACKUP_KEY, backupApi, bytes, type BackupDiff, type BackupLook, type BackupOverview, type BackupRecord, type BackupTask, type PikPakView } from "./janitorApi";

export function useBackup() {
  return useQuery({
    queryKey: BACKUP_KEY,
    queryFn: backupApi.overview,
    refetchOnMount: "always",
    refetchInterval: (q) => (q.state.data?.run.running ? 700 : q.state.data?.comparing || q.state.data?.look?.state === "running" || q.state.data?.pikpak.running ? 1000 : 4000),
  });
}

/** 「2026-11-25T12:03:00Z」 (or unix seconds) → 11-25 20:03 (local). */
function when(stamp: string | number | null): string {
  if (!stamp) return "";
  const d = new Date(typeof stamp === "number" ? stamp * 1000 : stamp);
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

const home = (ov: BackupOverview, p: string) => (p.startsWith(ov.home + "/") ? "~" + p.slice(ov.home.length) : p);
const toCopy = (d: BackupDiff | null) => (d ? { files: d.new.count + d.changed.count, bytes: d.new.bytes + d.changed.bytes } : { files: 0, bytes: 0 });

/** The top bar on 备份: the drive, and comparing again. */
export function BackupBar() {
  const qc = useQueryClient();
  const toast = useToast();
  const q = useBackup();
  const ov = q.data;
  if (!ov) return null;
  const d = ov.drive;
  const pick = (uuid: string | null) => backupApi.drive(uuid).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"));
  return (
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
        <Button disabled={ov.comparing || ov.run.running} onClick={() => void backupApi.compare().then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"))}>
          {ov.comparing ? "正在对比…" : "重新对比"}
        </Button>
      )}
      {d?.connected && (
        <Button disabled={ov.comparing || ov.run.running || ov.look?.state === "running"} onClick={() => void backupApi.look().then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"))}>
          柚子看一眼
        </Button>
      )}
    </div>
  );
}

export function BackupTab() {
  const q = useBackup();
  if (q.isError) return <EmptyState title="备份暂时打不开" hint={errorText(q.error)} action={<Button onClick={() => void q.refetch()}>重试</Button>} />;
  if (!q.data) return <EmptyState title="正在读取…" />;
  const ov = q.data;
  if (!ov.drive) return <PickDrive ov={ov} />;
  return <BackupBody ov={ov} />;
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

function BackupBody(props: { ov: BackupOverview }) {
  const { ov } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [over, setOver] = useState<Map<number, boolean>>(new Map());
  const [editing, setEditing] = useState<BackupTask | "new" | null>(null);
  const set = (o: BackupOverview) => qc.setQueryData(BACKUP_KEY, o);
  const on = (t: BackupTask) => over.get(t.id) ?? toCopy(t.diff).files > 0;
  const picked = ov.tasks.filter((t) => on(t) && !t.problem && t.diff);
  const total = picked.reduce((a, t) => ({ files: a.files + toCopy(t.diff).files, bytes: a.bytes + toCopy(t.diff).bytes }), { files: 0, bytes: 0 });
  const run = ov.run;
  const ready = !!ov.drive?.connected && !ov.comparing && !run.running;
  const start = () => backupApi.start(picked.map((t) => t.id)).then((o) => { set(o); setOver(new Map()); }, (e) => toast(errorText(e), "error"));
  return (
    <div>
      <div className="mb-4 space-y-2 rounded-card border bg-card px-4 py-3">
        <div className="flex flex-wrap items-center gap-3">
          <span className="text-base font-bold">备份</span>
          {run.running ? (
            <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground tabular-nums">
              {run.filesDone} / {run.filesTotal} 个 · {bytes(run.bytesDone)} / {bytes(run.bytesTotal)} · {run.task}
            </span>
          ) : (
            <span className="text-sm text-muted-foreground tabular-nums">
              {ov.tasks.some((t) => toCopy(t.diff).files > 0) ? `选了 ${picked.length} 个任务 · 要复制 ${total.files} 个文件、${bytes(total.bytes)}` : ov.comparing ? "正在对比…" : ov.tasks.length && ov.drive?.connected ? "都和硬盘上一样" : ""}
            </span>
          )}
          {run.running ? (
            <Button className="ml-auto" onClick={() => void backupApi.stop()}>停下</Button>
          ) : (
            <Button variant="primary" className="ml-auto" disabled={!ready || !picked.length} onClick={() => void start()}>开始备份</Button>
          )}
        </div>
        {run.running && (
          <>
            <ProgressBar done={run.bytesDone} total={Math.max(run.bytesTotal, 1)} />
            {run.current && <p className="truncate text-xs text-muted-foreground">{run.current}</p>}
          </>
        )}
        {!run.running && run.ended && <p className="text-sm text-muted-foreground">{run.ended}</p>}
      </div>
      <PikPakCard ov={ov} />
      {ov.look && <LookCard look={ov.look} />}
      <Section title="任务" actions={<Button disabled={run.running} onClick={() => setEditing("new")}><Plus size={14} />任务</Button>}>
        {ov.tasks.length ? (
          <ul className="divide-y">
            {ov.tasks.map((t) => (
              <TaskRow key={t.id} ov={ov} t={t} on={on(t)} toggle={() => setOver((m) => new Map(m).set(t.id, !on(t)))} edit={() => setEditing(t)} />
            ))}
          </ul>
        ) : (
          <p className="px-4 py-3 text-sm text-muted-foreground">还没有任务</p>
        )}
      </Section>
      {ov.drive && <Skipped ov={ov} />}
      {ov.runs.length > 0 && (
        <Section title="记录">
          <ul className="divide-y">{ov.runs.map((r) => <RecordRow key={r.id} r={r} />)}</ul>
        </Section>
      )}
      {editing && <TaskDialog ov={ov} task={editing === "new" ? null : editing} close={() => setEditing(null)} />}
    </div>
  );
}

function Skipped(props: { ov: BackupOverview }) {
  const { ov } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const busy = ov.run.running;
  const taken = (f: string) => ov.skipped.includes(f) || ov.tasks.some((t) => t.target === f || t.target.startsWith(`${f}/`));
  const free = (ov.drive?.folders ?? []).filter((f) => !taken(f));
  const out = (f: string) => backupApi.skip(f, false).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"));
  return (
    <Section title="不备份" actions={<Button disabled={busy || !free.length} onClick={() => setAdding(true)}><Plus size={14} />文件夹</Button>}>
      {ov.skipped.length ? (
        <ul className="divide-y">
          {ov.skipped.map((f) => (
            <li key={f} className="flex items-center gap-2 px-4 py-2">
              <span className="selectable min-w-0 flex-1 truncate font-mono text-sm">{f}</span>
              <IconButton label={`备份 ${f}`} tip="移出" disabled={busy} onClick={() => void out(f)}><Trash2 size={14} /></IconButton>
            </li>
          ))}
        </ul>
      ) : (
        <p className="px-4 py-3 text-sm text-muted-foreground">没有</p>
      )}
      {adding && <SkipDialog folders={free} close={() => setAdding(false)} />}
    </Section>
  );
}

function SkipDialog(props: { folders: string[]; close: () => void }) {
  const qc = useQueryClient();
  const [folder, setFolder] = useState(props.folders[0] ?? "");
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      qc.setQueryData(BACKUP_KEY, await backupApi.skip(folder, true));
      props.close();
    } catch (err) {
      setError(errorText(err));
    }
  };
  return (
    <Modal title="不备份" error={error} onClose={props.close} onSubmit={submit} submitLabel="不备份" submitDisabled={!folder}>
      <Field label="移动硬盘上的文件夹">
        <Dropdown<string> block ariaLabel="移动硬盘上的文件夹" value={folder} options={props.folders.map((f) => ({ value: f, label: f }))} onChange={setFolder} />
      </Field>
    </Modal>
  );
}

function TaskRow(props: { ov: BackupOverview; t: BackupTask; on: boolean; toggle: () => void; edit: () => void }) {
  const { ov, t } = props;
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const [open, setOpen] = useState(false);
  const d = t.diff;
  const c = toCopy(d);
  const del = async () => {
    if (!(await confirm({ title: "删掉这个任务？", message: "只删任务，电脑上和移动硬盘上的文件都不动。", confirmLabel: "删除", danger: true }))) return;
    backupApi.taskDelete(t.id).then((o) => qc.setQueryData(BACKUP_KEY, o), (e) => toast(errorText(e), "error"));
  };
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
      {confirmDialog}
      <div className="flex items-start gap-2.5">
        <input type="checkbox" checked={props.on && !t.problem && !!d} disabled={!!t.problem || !d || ov.run.running} onChange={props.toggle} aria-label={t.target} className="mt-1 size-3.5 shrink-0 accent-[var(--primary)]" />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
            <span className="min-w-0 truncate font-mono text-xs">{home(ov, t.source)}</span>
            <span className="text-muted-foreground">→</span>
            <span className="min-w-0 truncate">{t.target}</span>
          </div>
          <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 text-xs text-muted-foreground">
            {t.problem ? <span className="text-destructive">{t.problem}</span> : d ? <span>{counts.length ? counts.join(" · ") : "和硬盘上一样"}</span> : <span>{ov.drive?.connected ? "正在对比…" : "插上移动硬盘才能对比"}</span>}
            {t.last && <span>· 上次 {when(t.last)}</span>}
          </div>
          {d && counts.length > 0 && (
            <button type="button" onClick={() => setOpen((o) => !o)} className="mt-1 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
              <ChevronRight size={12} className={`transition-transform ${open ? "rotate-90" : ""}`} />
              {open ? "收起" : "看看有什么"}
            </button>
          )}
        </div>
        {c.files > 0 && <span className="shrink-0 pt-px tabular-nums">{bytes(c.bytes)}</span>}
        <IconButton label={`编辑 ${t.target}`} tip="编辑" disabled={ov.run.running} onClick={props.edit}><Pencil size={14} /></IconButton>
        <IconButton label={`删除 ${t.target}`} tip="删除" disabled={ov.run.running} onClick={() => void del()}><Trash2 size={14} /></IconButton>
      </div>
      {open && d && <DiffView ov={ov} t={t} d={d} />}
    </li>
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
      <Rows title="改过的" note="硬盘上那份挪进「旧版」" count={d.changed.count} size={d.changed.bytes} rows={d.changed.rows.map(row)} />
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
      <div className="selectable max-w-[720px] px-1">
        {l.text && <MarkdownView source={l.text} streaming={running} />}
        {running && !l.text && <p className="text-sm text-muted-foreground">正在看…</p>}
        {l.state === "failed" && <p className="text-sm text-destructive">没看成：{l.error ?? "不知道为什么"}</p>}
        {l.cards.length > 0 && <JanitorCards cards={l.cards} streaming={running} />}
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

/** PikPak: where the mirror stands, 停下 / 接着传 / 照样同步, the login and its settings. */
function PikPakCard(props: { ov: BackupOverview }) {
  const p = props.ov.pikpak;
  const qc = useQueryClient();
  const toast = useToast();
  const [dialog, setDialog] = useState<"login" | "settings" | null>(null);
  const set = (o: BackupOverview) => qc.setQueryData(BACKUP_KEY, o);
  const fail = (e: unknown) => toast(errorText(e), "error");
  const go = (force: boolean) => void backupApi.pikpakGo(force).then(set, fail);
  const stop = () => void backupApi.pikpakStop().then(() => qc.invalidateQueries({ queryKey: BACKUP_KEY }), fail);
  const status = !p.installed
    ? "没装 rclone：在终端里跑 brew install rclone"
    : !p.account
      ? "没登录"
      : p.running
        ? p.checking
          ? "正在对比…"
          : `${p.files} / ${p.totalFiles} 个 · ${bytes(p.bytes)} / ${bytes(p.totalBytes)}${p.speed ? ` · ${bytes(p.speed)}/s` : ""}${p.eta ? ` · 还要 ${left(p.eta)}` : ""}`
        : (p.ended ?? p.waiting ?? "");
  const facts = p.account ? [p.account, `PikPak 上的「${p.folder}」`, limitText(p.limit), p.used != null && p.total ? `用了 ${bytes(p.used)} / ${bytes(p.total)}` : ""].filter(Boolean) : [];
  return (
    <div className="mb-4 space-y-2 rounded-card border bg-card px-4 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-base font-bold">PikPak</span>
        <span className={`min-w-0 flex-1 truncate text-sm tabular-nums ${p.held ? "text-destructive" : "text-muted-foreground"}`}>{status}</span>
        {p.installed && !p.account && <Button variant="primary" className="ml-auto" onClick={() => setDialog("login")}>登录</Button>}
        {p.account && p.running && <Button className="ml-auto" onClick={stop}>停下</Button>}
        {p.account && !p.running && p.held && <Button variant="primary" className="ml-auto" onClick={() => go(true)}>照样同步</Button>}
        {p.account && !p.running && !p.held && p.waiting === "停下了" && <Button className="ml-auto" onClick={() => go(false)}>接着传</Button>}
        {p.account && <IconButton label="PikPak 的设置" tip="设置" onClick={() => setDialog("settings")}><Settings2 size={14} /></IconButton>}
      </div>
      {p.running && !p.checking && p.totalBytes > 0 && <ProgressBar done={p.bytes} total={p.totalBytes} />}
      {p.running && p.current.length > 0 && <p className="truncate text-xs text-muted-foreground">{p.current.join("、")}</p>}
      {facts.length > 0 && <p className="selectable truncate text-xs text-muted-foreground">{facts.join(" · ")}</p>}
      {dialog === "login" && <PikPakLogin close={() => setDialog(null)} />}
      {dialog === "settings" && <PikPakSettings p={p} close={() => setDialog(null)} />}
    </div>
  );
}

function PikPakLogin(props: { close: () => void }) {
  const qc = useQueryClient();
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      qc.setQueryData(BACKUP_KEY, await backupApi.pikpakLogin(account, password));
      props.close();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="登录 PikPak" error={error} onClose={props.close} onSubmit={submit} submitLabel={busy ? "正在登录…" : "登录"} submitDisabled={busy || !account.trim() || !password}>
      <Field label="账号">
        <TextInput aria-label="PikPak 的账号" value={account} maxLength={200} onChange={(e) => setAccount(e.target.value)} />
      </Field>
      <Field label="密码">
        <TextInput type="password" aria-label="PikPak 的密码" value={password} maxLength={200} onChange={(e) => setPassword(e.target.value)} />
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
    if (!(await confirm({ title: "退出 PikPak？", message: "同步停下，PikPak 上的文件都不动。", confirmLabel: "退出", danger: true }))) return;
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

function RecordRow(props: { r: BackupRecord }) {
  const { r } = props;
  const [open, setOpen] = useState(false);
  const more = r.errors.length > 0 || !!r.note;
  return (
    <li className="px-4 py-2 text-sm">
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-2">
        <span className="tabular-nums">{when(r.startedAt)}</span>
        <span className={r.status === "failed" ? "text-destructive" : "text-muted-foreground"}>{(r.dest === "pikpak" ? PIKPAK_STATUS : STATUS)[r.status]}</span>
        <span className="min-w-0 truncate text-muted-foreground">{r.dest === "pikpak" ? "PikPak" : r.tasks.map((t) => t.target).join("、")}</span>
        <span className="ml-auto shrink-0 text-muted-foreground tabular-nums">{r.dest === "pikpak" ? "传了" : "复制"} {r.files} 个 · {bytes(r.bytes)}</span>
        {more && (
          <button type="button" onClick={() => setOpen((o) => !o)} className="shrink-0 text-xs text-muted-foreground hover:text-foreground">
            {r.errors.length ? `${r.errors.length} 个没成` : "详情"}
          </button>
        )}
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

function TaskDialog(props: { ov: BackupOverview; task: BackupTask | null; close: () => void }) {
  const { ov, task } = props;
  const qc = useQueryClient();
  const drive = ov.drive;
  const [source, setSource] = useState(task?.source ?? "");
  const [target, setTarget] = useState(task?.target ?? "");
  const [excludes, setExcludes] = useState((task?.excludes ?? []).join("、"));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pickSource = async () => {
    const p = await openDialog({ directory: true, multiple: false, title: "电脑上的文件夹" });
    if (typeof p !== "string") return;
    setSource(p);
    const name = p.split("/").pop() ?? "";
    if (!target && name) setTarget(name);
  };
  const pickTarget = async () => {
    const p = await openDialog({ directory: true, multiple: false, title: "移动硬盘上的文件夹", defaultPath: drive?.path ?? undefined });
    if (typeof p !== "string") return;
    setTarget(drive?.path && p.startsWith(drive.path + "/") ? p.slice(drive.path.length + 1) : p);
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const o = await backupApi.taskSave({ id: task?.id ?? null, source, target, excludes: excludes.split(/[、,，\n]/).map((s) => s.trim()).filter(Boolean) });
      qc.setQueryData(BACKUP_KEY, o);
      props.close();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal wide title={task ? "编辑任务" : "新任务"} error={error} onClose={props.close} onSubmit={submit} submitLabel="保存" submitDisabled={busy || !source || !target.trim()}>
      <Field label="电脑上的文件夹">
        <div className="flex items-center gap-2">
          <p className="selectable min-w-0 flex-1 truncate font-mono text-xs" title={source}>{source ? home(ov, source) : "—"}</p>
          <Button onClick={() => void pickSource()}>选择…</Button>
        </div>
      </Field>
      <Field label="移动硬盘上的文件夹">
        <div className="flex items-center gap-2">
          <TextInput aria-label="移动硬盘上的文件夹" value={target} maxLength={200} onChange={(e) => setTarget(e.target.value)} />
          {drive?.connected && <Button onClick={() => void pickTarget()}>选择…</Button>}
        </div>
      </Field>
      <Field label="另外不备份的名字">
        <TextInput aria-label="另外不备份的名字" value={excludes} maxLength={600} onChange={(e) => setExcludes(e.target.value)} />
      </Field>
    </Modal>
  );
}
