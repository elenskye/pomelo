// 小拾 小窗 (v0.3.5): a column on the right of any page (⌘J / the bird in the sidebar; not on the 小拾
// page). An ordinary conversation (kept in 「最近」, continued on the 小拾 page): the header switches
// to a recent one or starts a new one, and uses the page's model. Above the input a pill names the
// page (or the open / picked records); it goes with the next question unless × — ids only, Rust
// fetches the content under its rules (芝麻开门 sends nothing). Memories and project passages are
// chosen as on the page; attachments, 资料 and instructions stay on the 小拾 page. Skills can be asked
// for here too (「/名称」, 联网), and the 知识库 button works as on the page.
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bird, ChevronDown, Globe, MapPin, SquarePen, X } from "lucide-react";
import { moduleLabel } from "@/app/modules";
import { payload, pillText, type PageContext } from "@/app/pageContext";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { IconButton } from "@/shared/ui";
import { LIBRARY_KEY, MEMORIES_KEY, STATUS_KEY, WEB_SKILL, parseRemember, pickyApi, useLibrary, usePickyStatus, usePickyUi, useTemplates, type ChatMessage, type Query } from "./api";
import { Composer, toolClass, type ComposerHandle } from "./Composer";
import { SkillChips, useAskedSkills } from "./Plugins";
import { openFile } from "./Files";
import { ManageDialog } from "./Manage";
import { RememberDialog } from "./Memory";
import { IssueNotice, ModelMenu } from "./ModelMenu";
import { KnowledgeButton, KnowledgeDialog, useKnowledge } from "./Knowledge";
import { ThinkingButton, useThinking } from "./Thinking";
import { Transcript, type TranscriptHandle } from "./Transcript";
import { PrepNotices, activeIn, messagesKey, useLiveReply, usePrepare } from "./useChat";

const LAST = "ui.side_session";
const isId = (v: unknown): v is number | null => v === null || typeof v === "number";
const RECENT = 8;

export function SideChat(props: { context: PageContext | null; floating: boolean; onClose: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const status = usePickyStatus().data;
  const lib = useLibrary().data;
  const templates = useTemplates().data ?? [];
  const [sessionId, setSessionId] = usePickyUi<number | null>(LAST, null, isId);
  const transcript = useRef<TranscriptHandle>(null);
  const composer = useRef<ComposerHandle>(null);
  const [manage, setManage] = useState(false);
  const [kbOpen, setKbOpen] = useState(false);
  const [capture, setCapture] = useState<{ entries: string[]; remainder: string; projectId: number | null } | null>(null);
  /** The pill × : this context (by its contents) stays out until the page says something else. */
  const [dropped, setDropped] = useState<string | null>(null);

  const all = useMemo(() => (lib ? [...lib.pinned, ...lib.projects.flatMap((p) => p.sessions), ...lib.scheduled, ...lib.recent, ...lib.archived] : []), [lib]);
  const current = sessionId != null && (!lib || all.some((s) => s.id === sessionId)) ? sessionId : null;
  // opening a 定时 conversation here takes its dot away too
  const unseenHere = !!all.find((s) => s.id === current)?.unseen;
  useEffect(() => {
    if (current == null || !unseenHere) return;
    void pickyApi.seen(current).then(() => Promise.all([qc.invalidateQueries({ queryKey: LIBRARY_KEY }), qc.invalidateQueries({ queryKey: STATUS_KEY })]));
  }, [current, unseenHere]); // eslint-disable-line react-hooks/exhaustive-deps
  const open = (id: number | null) => setSessionId(id);
  const { live, busyHere, run, shownWith } = useLiveReply({ current, status, onSent: open, onStart: () => transcript.current?.toBottom() });
  const activeHere = activeIn(status, current);
  const messages = useQuery({
    queryKey: messagesKey(current),
    queryFn: () => (current == null ? Promise.resolve([] as ChatMessage[]) : pickyApi.messages(current)),
    refetchInterval: activeHere && !live ? 1000 : false,
  });
  const saved = messages.data ?? [];
  const shown = shownWith(saved);

  const ctxKey = props.context ? JSON.stringify(props.context) : null;
  useEffect(() => setDropped((d) => (d === ctxKey ? d : null)), [ctxKey]);
  const pill = props.context && ctxKey !== dropped ? pillText(props.context, props.context.module === "home" ? "首页" : props.context.module === "settings" ? "设置" : moduleLabel(props.context.module)) : null;
  const draftKey = `side:${current ?? "new"}`;
  const ask = useAskedSkills(draftKey);
  const web = ask.skills.find((s) => s.key === WEB_SKILL);
  const thinking = useThinking(current, draftKey);
  const knowledge = useKnowledge(current, draftKey, null);
  const projectId = (current != null && all.find((s) => s.id === current)?.projectId) || null;
  const prep = usePrepare({ current, draftProject: null, draftOff: false, projectId, knowledge: knowledge.forSend, knowledgeOn: knowledge.on });
  const busy = busyHere || prep.preparing != null;
  const kbPlan = useQuery({
    queryKey: ["picky", "retrieval-plan", "side", current, knowledge.on, knowledge.own.join(), status?.selection],
    queryFn: () => pickyApi.retrievalPlan(current, null, [], knowledge.forSend),
    enabled: knowledge.on,
  });

  const send = async (text: string) => {
    const remember = parseRemember(text);
    if (remember) {
      setCapture({ ...remember, projectId });
      return false;
    }
    prep.clearFail();
    const got = await prep.prepareFor(text, [], false);
    if ("error" in got) {
      prep.failCard(got, () => composer.current?.send());
      return false;
    }
    const ctx = pill && props.context ? payload(props.context) : null;
    const shownPage: Query | null = pill ? { module: props.context!.module, label: pill, state: "ok", result: "", records: [] } : null;
    const extras = { attachments: [], files: [], passages: got.passages, memories: got.memories };
    const key = draftKey;
    const sent = await run("send", text, (on) => pickyApi.send(current, null, null, false, thinking.forSend, text, extras, ctx, ask.asked, knowledge.forSend, on), [], shownPage);
    if (sent) {
      ask.clear(key);
      knowledge.clear(key);
    }
    return !!sent;
  };

  const versionRun = async (m: ChatMessage, text: string | null) => {
    const q = text != null ? m : saved[saved.findIndex((x) => x.id === m.id) - 1];
    prep.clearFail();
    const got = await prep.prepareFor(text ?? q?.content ?? "", [], false);
    if ("error" in got) {
      prep.failCard(got, () => void versionRun(m, text));
      return;
    }
    void run("version", text ?? "", (on) => (text != null ? pickyApi.edit(m.id, text, got.passages, got.memories, on) : pickyApi.regenerate(m.id, got.passages, got.memories, on)));
  };
  const stop = () => {
    const runId = live?.runId ?? activeHere?.run;
    if (runId != null) void pickyApi.stop(runId).catch((e) => toast(errorText(e), "error"));
  };
  const switchVersion = (m: ChatMessage, delta: -1 | 1) =>
    void pickyApi.switchVersion(m.id, delta).then(
      () => qc.invalidateQueries({ queryKey: messagesKey(current) }),
      (e) => toast(errorText(e), "error"),
    );
  const saveRemember = async (entries: string[], project: number | null, localOnly: boolean) => {
    const rest = capture?.remainder ?? "";
    await pickyApi.remember(entries, project, localOnly, current);
    void qc.invalidateQueries({ queryKey: MEMORIES_KEY });
    setCapture(null);
    toast(entries.length > 1 ? `记住了 ${entries.length} 条` : "记住了");
    composer.current?.setText(rest);
    if (rest) setTimeout(() => composer.current?.send());
  };

  // recent conversations for the switcher (newest first) and the one open now
  const recent = [...all].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).slice(0, RECENT);
  if (current != null && !recent.some((s) => s.id === current)) {
    const mine = all.find((s) => s.id === current);
    if (mine) recent.unshift(mine);
  }
  const title = current == null ? "新对话" : (all.find((s) => s.id === current)?.title ?? "对话");
  const issue = status && status.selection === "local" ? status.local.issue : null;

  return (
    <aside
      aria-label="小拾"
      className={`flex w-[380px] shrink-0 flex-col border-l bg-background ${props.floating ? "absolute inset-y-0 right-0 z-20 shadow-2xl" : ""}`}
    >
      <header data-tauri-drag-region className="flex h-16 shrink-0 items-end gap-1.5 px-4 pb-3">
        <Bird size={16} strokeWidth={1.75} className="mb-1.5 shrink-0" style={{ color: "var(--module-picky)" }} />
        <Dropdown<number | null>
          ariaLabel="切换对话"
          value={current}
          onChange={open}
          options={[{ value: null, label: "新对话" }, ...recent.map((s) => ({ value: s.id, label: <span className="block max-w-64 truncate">{s.title}</span>, group: "最近" }))]}
          renderTrigger={() => (
            <span className="flex min-w-0 items-center gap-1">
              <span className="truncate font-bold">{title}</span>
              <ChevronDown size={14} className="shrink-0 text-muted-foreground" />
            </span>
          )}
          triggerClassName="min-w-0 flex-1 rounded-control px-1.5 py-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
        />
        {status && !status.error && <ModelMenu status={status} />}
        <IconButton label="新对话" disabled={current == null && !live} onClick={() => (open(null), setTimeout(() => composer.current?.focus()))}>
          <SquarePen size={15} strokeWidth={1.5} />
        </IconButton>
        <IconButton label="收起小拾" onClick={props.onClose}>
          <X size={16} strokeWidth={1.5} />
        </IconButton>
      </header>
      {status?.error ? (
        <p className="selectable flex-1 px-6 pt-10 text-center text-sm text-destructive">{status.error}</p>
      ) : (
        <>
          <Transcript
            ref={transcript}
            compact
            sessionKey={`side-${current}`}
            messages={shown}
            busy={busy}
            empty={<p className="text-base">小拾在这儿，想聊点什么？</p>}
            onEdit={(m, text) => void versionRun(m, text)}
            onRegenerate={(m) => void versionRun(m, null)}
            onSwitch={switchVersion}
            onOpenFile={(id) => openFile(id, toast)}
            onStop={stop}
          />
          <div className="space-y-2 px-4 pt-2 pb-4">
            {issue && <IssueNotice issue={issue} compact />}
            <PrepNotices prep={prep} />
            <Composer
              ref={composer}
              draftKey={draftKey}
              busy={busy}
              templates={templates}
              onManageTemplates={() => setManage(true)}
              skills={ask.usable}
              onAskSkill={ask.toggle}
              tools={
                <>
                  <ThinkingButton on={thinking.on} onToggle={thinking.toggle} className={toolClass} />
                  {web?.enabled && (
                    <button
                      type="button"
                      aria-pressed={ask.asked.includes(WEB_SKILL)}
                      onClick={() => ask.toggle(WEB_SKILL)}
                      className={`${toolClass} ${ask.asked.includes(WEB_SKILL) ? "bg-accent text-accent-foreground" : ""}`}
                    >
                      <Globe size={14} strokeWidth={1.5} /> 联网
                    </button>
                  )}
                  <KnowledgeButton kb={knowledge} plan={kbPlan.data?.knowledge ?? null} onManage={() => setKbOpen(true)} />
                </>
              }
              chips={
                (pill || ask.asked.length > 0) && (
                  <div className="flex flex-wrap gap-1.5 px-3 pt-2.5">
                    <SkillChips skills={ask.skills} asked={ask.asked} onRemove={ask.toggle} />
                    {pill && (
                      <span className="flex max-w-full items-center gap-1 rounded-full bg-accent py-0.5 pr-1 pl-2 text-xs text-accent-foreground">
                        <MapPin size={12} strokeWidth={1.75} className="shrink-0" />
                        <span className="truncate">{pill}</span>
                        <button
                          type="button"
                          aria-label="这次不带"
                          onClick={() => setDropped(ctxKey)}
                          className="shrink-0 rounded-full p-0.5 outline-none hover:bg-black/5 focus-visible:ring-2 focus-visible:ring-ring dark:hover:bg-white/10"
                        >
                          <X size={12} />
                        </button>
                      </span>
                    )}
                  </div>
                )
              }
              onSend={send}
              onStop={stop}
            />
          </div>
        </>
      )}
      {manage && <ManageDialog tab="templates" projects={lib?.projects ?? []} sessionIds={all.map((s) => s.id)} onOpenSession={open} onClose={() => setManage(false)} />}
      {kbOpen && <KnowledgeDialog onClose={() => setKbOpen(false)} />}
      {capture && <RememberDialog entries={capture.entries} projects={lib?.projects ?? []} projectId={capture.projectId} onSave={saveRemember} onClose={() => setCapture(null)} />}
    </aside>
  );
}
