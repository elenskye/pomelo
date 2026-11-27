// DevJanitor (助手, v0.8 阶段 6; Rust youzi/assistants/janitor/): 柚子's 电脑管家. 磁盘: memory as one bar,
// the disk as a sunburst (click a ring to go in, the crumbs to go back) beside the same folders as a list.
// 清理 and 环境 (sorted into classes, 1.1.5): JanitorLists.tsx. 备份 (1.1.0): JanitorBackup.tsx.
// Agent (阶段 8): DevJanitor's own conversation (not in 柚子's list) with its model and 思考 above it; its
// cards (JanitorCards.tsx) under the replies; 新对话 drops the old one.
// 设置: which categories 一键清理 holds (with their class's sticker) and their names (click to rename), how many
// days make a project 很久没动 (not undone); the Agent's rules (rename, 删: one undo step each); its SKILL (mine to
// rewrite, in its own framed box) and the rest it's told.
// Everything but the buttons is selectable (copy a path, a size, a card's command).
// Opening the page scans in the background unless the last scan is fresh; the last result shows meanwhile.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, FolderOpen, Plus, Trash2, X } from "@/shared/icons";
import { ClassBadge, CleanTab, EnvTab } from "./JanitorLists";
import { useSubNav, useSubPage } from "@/app/subpage";
import { Section } from "@/settings/layout";
import { StackBar, Sunburst, type SunNode } from "@/shared/charts";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Dropdown } from "@/shared/Dropdown";
import { Button, EmptyState, IconButton, Switch, TextInput } from "@/shared/ui";
import { youziApi, useYouziStatus, useTemplates, type ChatMessage } from "../api";
import { Composer, type ComposerHandle } from "../Composer";
import { openFile } from "../Files";
import { useModels } from "../Schedules";
import { LevelPicker, fromLevel, levelOf } from "../Thinking";
import { Transcript, type TranscriptHandle } from "../Transcript";
import { PrepNotices, activeIn, messagesKey, useLiveReply, usePrepare } from "../useChat";
import { JanitorCards } from "./JanitorCards";
import { BackupBar, BackupTab, type BackupView } from "./JanitorBackup";
import { JANITOR_KEY, bytes, dayOf, janitorApi, type JanitorOverview, type JanitorRing } from "./janitorApi";
import { MdField } from "@/shared/editor/MdField";
import { showDialog } from "@/shared/dialog";
import { pad } from "@/shared/dates";

type Tab = "disk" | "clean" | "backup" | "env" | "agent" | "settings";
const OVERVIEW = [...JANITOR_KEY, "overview"];

function useOverview() {
  return useQuery({
    queryKey: OVERVIEW,
    queryFn: janitorApi.overview,
    refetchOnMount: "always",
    refetchInterval: (q) => (q.state.data?.scan.running ? 1000 : false),
  });
}

export function JanitorPage() {
  const qc = useQueryClient();
  const [tab, setTab] = useSubPage<Tab>("janitor");
  const [backupView, setBackupView] = useState<BackupView>("run");
  useSubNav("janitor", (p) => setTab(p as Tab));
  const ov = useOverview();
  useEffect(() => {
    janitorApi.scan(false).then(() => qc.invalidateQueries({ queryKey: OVERVIEW })).catch(() => {});
  }, [qc]);
  if (ov.isError) return <EmptyState title="DevJanitor 暂时打不开" hint={errorText(ov.error)} action={<Button onClick={() => void ov.refetch()}>重试</Button>} />;
  if (!ov.data) return <EmptyState title="正在读取…" />;
  return (
    <div className="relative flex min-h-0 min-w-0 flex-1 flex-col px-6 pb-4">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b pb-3">
        {tab === "backup" ? <BackupBar view={backupView} setView={setBackupView} /> : <ScanBar ov={ov.data} />}
      </div>
      {tab === "agent" ? (
        <div className="flex min-h-0 flex-1 flex-col pt-2"><AgentTab /></div>
      ) : tab === "clean" ? (
        <div className="flex min-h-0 flex-1 flex-col pt-4"><CleanTab ov={ov.data} /></div>
      ) : (
        <div className="selectable -m-1 min-h-0 flex-1 overflow-y-auto p-1 pt-4">
          {tab === "disk" ? <DiskTab ov={ov.data} /> : tab === "backup" ? <BackupTab view={backupView} /> : tab === "env" ? <EnvTab ov={ov.data} /> : <SettingsTab />}
        </div>
      )}
    </div>
  );
}

function ScanBar(props: { ov: JanitorOverview }) {
  const qc = useQueryClient();
  const { scan, summary, mode } = props.ov;
  const rescan = () => janitorApi.scan(true).then(() => qc.invalidateQueries({ queryKey: OVERVIEW }));
  const when = summary ? new Date(summary.scannedAt * 1000) : null;
  return (
    <div className="ml-auto flex min-w-0 items-center gap-3 text-sm">
      {mode === "sandbox" && <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">假 Mac</span>}
      {scan.running ? (
        <>
          <span className="min-w-0 truncate text-muted-foreground tabular-nums">
            正在{scan.phase || "扫描"} · {scan.files.toLocaleString()} 个文件 · {bytes(scan.bytes)}
          </span>
          <Button onClick={() => void janitorApi.stop()}>停下</Button>
        </>
      ) : (
        <>
          {scan.error && <span className="min-w-0 truncate text-destructive">{scan.error}</span>}
          {when && summary && !scan.error && (
            <span className="min-w-0 truncate text-muted-foreground tabular-nums">
              扫描于 {dayOf(summary.scannedAt).slice(5)} {pad(when.getHours())}:{pad(when.getMinutes())} · 用了 {Math.max(1, Math.round(summary.took / 1000))} 秒
            </span>
          )}
          <Button onClick={() => void rescan()}>重新扫描</Button>
        </>
      )}
    </div>
  );
}

// ---- 磁盘 ----

const PRESSURE = { normal: "正常", warn: "偏高", critical: "很高" } as const;

function MemoryLine() {
  const mem = useQuery({ queryKey: [...JANITOR_KEY, "memory"], queryFn: janitorApi.memory, refetchInterval: 10_000, refetchOnMount: "always" });
  const m = mem.data;
  if (!m) return null;
  const tone = m.pressure === "critical" ? "text-destructive" : m.pressure === "warn" ? "text-[var(--status-doing-fg)]" : "text-[var(--status-done-fg)]";
  // the biggest programs, then the rest of what's used; the empty end is what's free
  const top = m.top.slice(0, 5);
  const shown = top.reduce((a, t) => a + t.bytes, 0);
  const segments = [
    ...top.map((t, i) => ({ key: t.name, label: t.name, value: t.bytes, color: `var(--chart-${i + 1})` })),
    ...(m.used > shown ? [{ key: "rest", label: "其他", value: m.used - shown, color: "var(--chart-other)" }] : []),
  ];
  return (
    <div className="space-y-2 rounded-card border bg-card px-4 py-3 text-sm">
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="font-bold">运行内存</span>
        <span className={tone}>压力{PRESSURE[m.pressure]}</span>
        <span className="ml-auto text-muted-foreground tabular-nums">已用 {bytes(m.used)} / {bytes(m.total)} · 空着 {bytes(Math.max(0, m.total - m.used))}</span>
      </div>
      <StackBar segments={segments} total={m.total} format={bytes} label="运行内存" />
    </div>
  );
}

const toSun = (r: JanitorRing): SunNode => ({ key: r.path, label: r.name, value: r.size, color: r.kind === "system" ? "var(--chart-other)" : undefined, children: r.children.map(toSun) });

function DiskTab(props: { ov: JanitorOverview }) {
  const toast = useToast();
  const { summary } = props.ov;
  const [path, setPath] = useState("/");
  const ring = useQuery({ queryKey: [...JANITOR_KEY, "ring", path, summary?.scannedAt ?? 0], queryFn: () => janitorApi.ring(path), enabled: !!summary });
  const view = ring.data;
  const reveal = (p: string) => janitorApi.reveal(p).catch((e) => toast(errorText(e), "error"));
  const colorOf = (i: number, kind: string) => (kind === "system" ? "var(--chart-other)" : i < 6 ? `var(--chart-${i + 1})` : "var(--chart-other)");
  if (!summary) return <div className="space-y-4"><MemoryLine /><EmptyState title={props.ov.scan.running ? "正在扫描磁盘…" : "还没扫描"} /></div>;
  const node = view?.node;
  const disk = summary.disk;
  return (
    <div className="space-y-4">
      <MemoryLine />
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-1 text-sm">
        {disk && (
          <>
            <span className="font-bold tabular-nums">{bytes(disk.total)}</span>
            <span className="text-muted-foreground tabular-nums">用了 {bytes(disk.total - disk.free)} · 还剩 {bytes(disk.free)}</span>
          </>
        )}
        {!summary.fullDiskAccess && (
          <span className="text-muted-foreground">
            没有完全磁盘访问权限，{summary.denied} 处没扫{" "}
            <button type="button" className="text-primary underline-offset-2 hover:underline" onClick={() => void janitorApi.openPrivacy()}>打开系统设置</button>
          </span>
        )}
      </div>
      {view && node && (
        <>
          <nav aria-label="位置" className="flex flex-wrap items-center gap-1 px-1 text-sm">
            {view.crumbs.map(([p, name]) => (
              <span key={p} className="flex items-center gap-1">
                <button type="button" className="rounded px-1 text-muted-foreground hover:text-foreground" onClick={() => setPath(p)}>{name}</button>
                <ChevronRight size={13} className="text-muted-foreground" />
              </span>
            ))}
            <span className="px-1 font-bold">{node.name}</span>
          </nav>
          <div className="flex flex-wrap items-start gap-8">
            <Sunburst
              root={toSun(node)}
              format={bytes}
              size={320}
              onPick={(n) => {
                const hit = findRing(node, n.key);
                if (hit && hit.kind === "dir" && hit.children.length) setPath(hit.path);
              }}
              center={
                <>
                  <span className="max-w-full truncate text-xs text-muted-foreground">{node.name}</span>
                  <span className="text-base font-bold tabular-nums">{bytes(node.size)}</span>
                </>
              }
            />
            <ul className="min-w-[280px] flex-1 divide-y rounded-card border bg-card">
              {node.children.map((k, i) => (
                <li key={k.path} className="group flex items-center gap-2.5 px-3 py-2 text-sm">
                  <span className="size-2.5 shrink-0 rounded-full" style={{ background: colorOf(i, k.kind) }} />
                  <button
                    type="button"
                    disabled={!(k.kind === "dir" && k.children.length)}
                    onClick={() => setPath(k.path)}
                    className="min-w-0 flex-1 truncate text-left enabled:hover:underline disabled:cursor-default"
                  >
                    {k.name}
                  </button>
                  {k.denied > 0 && <span className="shrink-0 rounded-full bg-muted px-1.5 text-xs text-muted-foreground">没权限 {k.denied}</span>}
                  {k.kind !== "system" && (
                    <button type="button" aria-label="在访达中显示" title="在访达中显示" onClick={() => void reveal(k.path)} className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100">
                      <FolderOpen size={14} />
                    </button>
                  )}
                  <span className="w-20 shrink-0 text-right tabular-nums">{bytes(k.size)}</span>
                </li>
              ))}
              {node.rest > 0 && (
                <li className="flex items-center gap-2.5 px-3 py-2 text-sm text-muted-foreground">
                  <span className="size-2.5 shrink-0 rounded-full border" />
                  <span className="min-w-0 flex-1 truncate">其余</span>
                  <span className="w-20 shrink-0 text-right tabular-nums">{bytes(node.rest)}</span>
                </li>
              )}
            </ul>
          </div>
        </>
      )}
    </div>
  );
}

function findRing(n: JanitorRing, path: string): JanitorRing | null {
  if (n.path === path) return n;
  for (const k of n.children) {
    const f = findRing(k, path);
    if (f) return f;
  }
  return null;
}

// ---- Agent ----

const AGENT_KEY = [...JANITOR_KEY, "agent"];

function AgentTab() {
  const agent = useQuery({ queryKey: AGENT_KEY, queryFn: janitorApi.agent });
  if (agent.isError) return <EmptyState title="Agent 暂时打不开" hint={errorText(agent.error)} action={<Button onClick={() => void agent.refetch()}>重试</Button>} />;
  if (!agent.data) return null;
  return <AgentChat key={agent.data.session} session={agent.data.session} choice={agent.data.choice} />;
}

function AgentChat(props: { session: number; choice: { model: string | null; thinking: boolean; effort?: string | null } }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const status = useYouziStatus().data;
  const templates = useTemplates().data ?? [];
  const transcript = useRef<TranscriptHandle>(null);
  const composer = useRef<ComposerHandle>(null);
  const current = props.session;
  const { live, busyHere, run, shownWith } = useLiveReply({ current, status, onSent: () => {}, onStart: () => transcript.current?.toBottom() });
  const activeHere = activeIn(status, current);
  // refetched on mount: a reply that ended (with its cards) while the page was closed
  const messages = useQuery({ queryKey: messagesKey(current), queryFn: () => youziApi.messages(current), refetchOnMount: "always", refetchInterval: activeHere && !live ? 1000 : false });
  const writing = busyHere || !!activeHere;
  const prep = usePrepare({ current, draftProject: null, draftOff: true, projectId: null, knowledge: null, knowledgeOn: false });
  const busy = writing || prep.preparing != null;
  const draftKey = `janitor-agent:${current}`;
  const { models, keep } = useModels(props.choice.model, props.choice.thinking);
  const setChoice = (c: { model: string | null; thinking: boolean; effort?: string | null }) => janitorApi.agentSet(c).then((v) => qc.setQueryData(AGENT_KEY, v), (e) => toast(errorText(e), "error"));
  const send = async (text: string) => {
    prep.clearFail();
    const got = await prep.prepareFor(text, [], false);
    if ("error" in got) { prep.failCard(got, () => composer.current?.send()); return false; }
    const extras = { attachments: [], files: [], passages: got.passages, memories: [] };
    return !!(await run("send", text, (on) => youziApi.send(current, null, null, false, null, null, text, extras, null, [], null, on), [], null));
  };
  const versionRun = async (m: ChatMessage, text: string | null) => {
    void run("version", text ?? "", (on) => (text != null ? youziApi.edit(m.id, text, [], [], on) : youziApi.regenerate(m.id, [], [], on)));
  };
  const stop = () => { const runId = live?.runId ?? activeHere?.run; if (runId != null) void youziApi.stop(runId).catch((e) => toast(errorText(e), "error")); };
  const switchVersion = (m: ChatMessage, delta: -1 | 1) => void youziApi.switchVersion(m.id, delta).then(() => qc.invalidateQueries({ queryKey: messagesKey(current) }), (e) => toast(errorText(e), "error"));
  const fresh = async () => {
    if ((messages.data ?? []).length && !(await confirm({ title: "开个新对话？", message: "现在这个对话会删掉。", confirmLabel: "新对话" }))) return;
    try {
      const v = await janitorApi.agentNew();
      qc.removeQueries({ queryKey: messagesKey(v.session) });
      qc.setQueryData(AGENT_KEY, v);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {confirmDialog}
      <div className="flex flex-wrap items-center gap-2 pb-2">
        <div className="w-64"><Dropdown<string | null> block ariaLabel="Agent · 模型" value={props.choice.model} options={models} onChange={(m) => void setChoice({ ...props.choice, model: m })} /></div>
        <LevelPicker label="Agent · 思考" model={props.choice.model} value={levelOf(props.choice.thinking, props.choice.effort ?? null)} onChange={(l) => { const v = fromLevel(l); void setChoice({ model: keep(props.choice.model, v.thinking), ...v }); }} />
        <Button className="ml-auto" disabled={busy} onClick={() => void fresh()}><Plus size={14} /> 新对话</Button>
      </div>
      <Transcript ref={transcript} sessionKey={`janitor-agent-${current}`} messages={shownWith(messages.data ?? [])} busy={busy} empty={<p className="text-base">想清哪里、拿不准哪个能删，问我。</p>} onEdit={(m, text) => void versionRun(m, text)} onRegenerate={(m) => void versionRun(m, null)} onSwitch={switchVersion} onOpenFile={(id) => openFile(id, toast)} onStop={stop}
        extra={(m, streaming) => (m.janitor?.length ? <JanitorCards cards={m.janitor} streaming={streaming || writing} /> : null)} />
      <div className="space-y-2 pt-2">
        <PrepNotices prep={prep} />
        <Composer ref={composer} draftKey={draftKey} busy={busy} templates={templates} onManageTemplates={() => toast("模板在柚子页管理")} onSend={send} onStop={stop} />
      </div>
    </div>
  );
}

// ---- 设置 ----

function SettingsTab() {
  const qc = useQueryClient();
  const toast = useToast();
  const view = useQuery({ queryKey: [...JANITOR_KEY, "settings"], queryFn: janitorApi.settings });
  const [days, setDays] = useState("");
  useEffect(() => {
    if (view.data) setDays(String(view.data.settings.staleDays));
  }, [view.data]);
  if (!view.data) return null;
  const s = view.data.settings;
  const save = async (next: typeof s) => {
    try {
      qc.setQueryData([...JANITOR_KEY, "settings"], await janitorApi.settingsSet(next));
      await qc.invalidateQueries({ queryKey: OVERVIEW });
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  return (
    <div className="mx-auto w-full max-w-2xl">
      <Section title="一键清理里放哪些">
        <ul className="divide-y">
          {view.data.categories.map((c) => (
            <li key={c.id} className="flex items-center gap-3 px-4 py-2.5 text-sm">
              <ClassBadge id={c.class} />
              <div className="min-w-0 flex-1">
                <NameEdit
                  name={s.names?.[c.id] ?? c.name}
                  hint={s.names?.[c.id] ? `原来叫「${c.name}」，清空就改回去` : undefined}
                  onSave={(n) => {
                    const names = { ...(s.names ?? {}) };
                    if (n && n !== c.name) names[c.id] = n;
                    else delete names[c.id];
                    void save({ ...s, names });
                  }}
                />
                <div className="text-xs text-muted-foreground">{c.rebuild}</div>
              </div>
              <Switch label={s.names?.[c.id] ?? c.name} checked={!s.off.includes(c.id)} onChange={(v) => void save({ ...s, off: v ? s.off.filter((o) => o !== c.id) : [...s.off, c.id] })} />
            </li>
          ))}
        </ul>
      </Section>
      {view.data.rules.length > 0 && (
        <Section title="Agent 加进一键清理的">
          <ul className="divide-y">
            {view.data.rules.map((r) => (
              <li key={r.id} className="flex items-center gap-3 px-4 py-2.5 text-sm">
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 items-baseline gap-2">
                    <NameEdit name={r.name} onSave={(n) => n && void janitorApi.ruleRename(r.id, n).then((v) => { qc.setQueryData([...JANITOR_KEY, "settings"], v); void qc.invalidateQueries({ queryKey: OVERVIEW }); }, (e) => toast(errorText(e), "error"))} />
                    {r.checked && <span className="shrink-0 text-xs text-muted-foreground">默认勾上</span>}
                  </div>
                  <div className="truncate font-mono text-xs text-muted-foreground">{r.pattern}</div>
                  {r.rebuild && <div className="text-xs text-muted-foreground">{r.rebuild}</div>}
                </div>
                <IconButton label={`删掉「${r.name}」`} tip="删除" onClick={() => void janitorApi.ruleDelete(r.id).then((v) => { qc.setQueryData([...JANITOR_KEY, "settings"], v); void qc.invalidateQueries({ queryKey: OVERVIEW }); }, (e) => toast(errorText(e), "error"))}><Trash2 size={14} /></IconButton>
              </li>
            ))}
          </ul>
        </Section>
      )}
      <Section title="很久没动的项目" help="项目里的文件这么多天都没改过，它的依赖和编译结果在一键清理里默认勾上。">
        <div className="flex items-center gap-2 px-4 py-2.5 text-sm">
          <span className="shrink-0 whitespace-nowrap">没动过</span>
          <div className="ml-auto w-20 shrink-0">
          <TextInput
            type="number"
            min={1}
            value={days}
            onChange={(e) => setDays(e.target.value)}
            onBlur={() => {
              const n = Math.round(Number(days));
              if (n >= 1 && n !== s.staleDays) void save({ ...s, staleDays: n });
              else setDays(String(s.staleDays));
            }}
            className="text-right"
            aria-label="天数"
          />
          </div>
          <span className="shrink-0 whitespace-nowrap">天</span>
        </div>
      </Section>
      <AgentPrompt />
    </div>
  );
}

/** A name that turns into a text box on click (Enter / leaving saves, Esc keeps the old one). */
function NameEdit(props: { name: string; hint?: string; onSave: (name: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [v, setV] = useState(props.name);
  if (!editing)
    return (
      <button type="button" title={props.hint ?? "改名"} onClick={() => (setV(props.name), setEditing(true))} className="max-w-full truncate rounded text-left hover:underline">
        {props.name}
      </button>
    );
  const done = (keep: boolean) => {
    setEditing(false);
    if (keep && v.trim() !== props.name) props.onSave(v.trim());
  };
  return (
    <input
      autoFocus
      value={v}
      aria-label={`「${props.name}」改名`}
      maxLength={30}
      onFocus={(e) => e.target.select()}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => done(true)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && !e.nativeEvent.isComposing) done(true);
        else if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          done(false);
        }
      }}
      className="w-full min-w-0 rounded-control border bg-card px-2 py-0.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
    />
  );
}

const PROMPT_KEY = [...JANITOR_KEY, "prompt"];

/** What the Agent is told: its SKILL (mine to rewrite; 恢复默认 brings the built-in one back) and what the
 *  program writes each time (the opening and the situation now). */
function AgentPrompt() {
  const qc = useQueryClient();
  const toast = useToast();
  const prompt = useQuery({ queryKey: PROMPT_KEY, queryFn: janitorApi.agentPrompt, refetchOnMount: "always" });
  const [draft, setDraft] = useState<string | null>(null);
  const [showing, setShowing] = useState(false);
  const p = prompt.data;
  if (!p) return null;
  const text = draft ?? p.skill;
  const dirty = draft != null && draft !== p.skill;
  const save = (skill: string | null) =>
    void janitorApi.agentSkill(skill).then(
      (v) => (qc.setQueryData(PROMPT_KEY, v), setDraft(null)),
      (e) => toast(errorText(e), "error"),
    );
  return (
    <Section
      bare
      title="发给 Agent 的说明"
      help="SKILL 是 Agent 做事的规矩，能改；开头和「现在的情况」由程序每次写好。"
      actions={
        <>
          <Button variant="ghost" onClick={() => { void prompt.refetch(); setShowing(true); }}>开头和现在的情况</Button>
          {p.custom && !dirty && <Button onClick={() => save(null)}>恢复默认</Button>}
          {dirty && <Button onClick={() => setDraft(null)}>还原</Button>}
          <Button variant="primary" disabled={!dirty || !text.trim()} onClick={() => save(text)}>保存</Button>
        </>
      }
    >
      <MdField label="SKILL" value={text} onChange={setDraft} rows={16} className="font-reading text-xs" />
      {showing && createPortal(<PromptHead text={`${p.head}\n\n## 现在的情况\n${p.situation}`} onClose={() => setShowing(false)} />, document.body)}
    </Section>
  );
}

function PromptHead(props: { text: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    showDialog(ref.current);
  }, []);
  return (
    <dialog ref={ref} aria-label="开头和现在的情况" onCancel={(e) => { e.preventDefault(); props.onClose(); }} className="m-auto h-fit max-h-[80vh] w-[720px] max-w-[calc(100vw-48px)] rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30">
      <div className="flex max-h-[80vh] flex-col">
        <div className="flex items-center gap-2 border-b px-5 py-3">
          <p className="min-w-0 flex-1 truncate text-base font-bold">开头和现在的情况</p>
          <IconButton label="关闭" onClick={props.onClose}><X size={16} /></IconButton>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <pre className="selectable font-sans text-sm leading-relaxed whitespace-pre-wrap">{props.text}</pre>
        </div>
      </div>
    </dialog>
  );
}
