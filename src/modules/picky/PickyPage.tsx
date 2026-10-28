// 小拾: conversation list (collapsible; or the search panel) · message area · input. Opening the page
// returns to the last conversation. 「新对话」 (also inside a project) is only a draft until its first
// question is sent. A search hit (here or from ⌘K) opens its conversation at that message, switching to
// its version if needed. Replies stream in over a
// Channel and are laid over the saved path (Rust also checkpoints them, so after a reload while one is
// being written the saved text is polled instead).
// Phase 4: templates (the input's "/" menu and 模板 menu, the empty page's pinned cards), 指令 (a draft
// keeps its own choice here until its first question), 管理, and the context check while typing: near
// the limit a notice offers to summarise earlier turns (总结并继续 sends the waiting question after).
// Phase 5: attachments per draft (+ menu, the library, files dropped on the window), the project files a
// question uses (资料; the choice stays for later questions, restored from the last question), and
// passages retrieved before a question in a project is saved — if 8081 can't be used, a card asks:
// send without, retry or cancel. A reply that reads material in segments shows its progress and notes.
// Phase 6: memories are chosen in the same step (记忆 menu: this question / this conversation; a draft
// keeps its own conversation switch); 「/记住 …」 opens a confirmation card instead of sending.
// v0.4.5 阶段 7: the 知识库 button (and its manager from the list's bottom); opening the page compares the
// library folders with their index. 阶段 8: a 助手 (left column) opens its own page in place of the
// conversation; it is remembered like the last conversation. Work (改文件, 新建 Word) is the plugin
// library's Word 工具箱 / 文本工具箱 now: 小拾 uses them when it decides to (no entry here).
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Bird, Globe, PanelLeftClose, PanelLeftOpen, Pin, Plus, SquarePen } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useToast } from "@/shared/feedback";
import { useOpenRequest } from "@/shared/open";
import { usePageKeys } from "@/shared/panel";
import { Button, IconButton, PageHeader } from "@/shared/ui";
import {
  LIBRARY_KEY,
  MEMORIES_KEY,
  STATUS_KEY,
  isBool,
  parseRemember,
  pickyApi,
  useLibrary,
  usePickyStatus,
  usePickyUi,
  useProjectFiles,
  useTemplates,
  currentFile,
  projectFilesKey,
  usableFile,
  WEB_SKILL,
  type Assessment,
  type ChatHit,
  type ChatMessage,
  type FileInfo,
  type MessageFile,
  type SessionSetting,
} from "./api";
import { ChatSearch } from "./ChatSearch";
import { Composer, toolClass, type ComposerHandle } from "./Composer";
import { ConversationList } from "./ConversationList";
import { AttachmentLibrary, FileChip, FilesButton, ProjectFiles, WebDialog, openFile, useImport } from "./Files";
import { InstructionsButton, NO_SETTING } from "./Instructions";
import { KnowledgeButton, KnowledgeDialog, useKnowledge } from "./Knowledge";
import { ManageDialog, type ManageTab } from "./Manage";
import { PluginsDialog, SkillChips, useAskedSkills } from "./Plugins";
import { SchedulesDialog } from "./Schedules";
import { MemoryButton, RememberDialog } from "./Memory";
import { summary } from "./slots";
import { IssueNotice, ModelMenu } from "./ModelMenu";
import { ThinkingButton, useThinking } from "./Thinking";
import { Transcript, type TranscriptHandle } from "./Transcript";
import { PrepNotices, activeIn, messagesKey, useLiveReply, usePrepare } from "./useChat";
import { useTabs } from "@/app/tabs";

const asMessageFile = (f: FileInfo, label: string | null = null, fileId: number | null = null): MessageFile => ({ ...f, fileId, version: null, label });
const EMPTY_QUESTION = "请总结这些附件。";

const MESSAGES = messagesKey;
const LAST = "ui.session";
const COLLAPSED = "ui.list_collapsed";

export default function PickyPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const status = usePickyStatus().data;
  const library = useLibrary();
  const [sessionId, setSessionId] = useState<number | null | undefined>(undefined); // undefined = not chosen yet
  const tabs = useTabs();
  /** A draft started inside a project. */
  const [draftProject, setDraftProject] = useState<number | null>(null);
  const [searching, setSearching] = useState(false);
  const [reveal, setReveal] = useState<{ id: number; query: string; key: number } | null>(null);
  const [collapsed, setCollapsed] = usePickyUi(COLLAPSED, false, isBool);
  const transcript = useRef<TranscriptHandle>(null);
  const composer = useRef<ComposerHandle>(null);
  const templates = useTemplates().data ?? [];
  const [manage, setManage] = useState<ManageTab | null>(null);
  const [plugins, setPlugins] = useState(false);
  const [schedules, setSchedules] = useState(false);
  /** Drafts' own instructions, by draft key. */
  const [draftSettings, setDraftSettings] = useState<Record<string, SessionSetting>>({});
  const [draftText, setDraftText] = useState("");
  const [assessment, setAssessment] = useState<Assessment | null>(null);
  const [dismissed, setDismissed] = useState(false);
  /** The summary being written here: its conversation and run. */
  const [summary_, setSummary] = useState<{ session: number; run: number | null } | null>(null);
  /** After 总结并继续: the question to send once the slot is free (if it is still the draft). */
  const [continueAfter, setContinueAfter] = useState<{ session: number; text: string } | null>(null);
  /** Drafts' attachments and chosen project files, by draft key. */
  const [attach, setAttach] = useState<Record<string, FileInfo[]>>({});
  const [fileSel, setFileSel] = useState<Record<string, number[]>>({});
  const [libraryOpen, setLibraryOpen] = useState(false);
  /** The project whose files are being managed. */
  const [filesFor, setFilesFor] = useState<number | null>(null);
  /** 「本条使用记忆」 (back on after each question and in another conversation); drafts' own 「本会话关闭记忆」. */
  const [nextMemoryOn, setNextMemoryOn] = useState(true);
  const [draftMemoryOff, setDraftMemoryOff] = useState<Record<string, boolean>>({});
  /** /记住 waiting for confirmation. */
  const [capture, setCapture] = useState<{ entries: string[]; remainder: string; projectId: number | null } | null>(null);
  const [dropping, setDropping] = useState(false);
  const { choose, importPaths, importPasted } = useImport();
  const [webOpen, setWebOpen] = useState(false);
  const [kbOpen, setKbOpen] = useState(false);
  useEffect(() => void pickyApi.kbScan().catch(() => {}), []);

  const lib = library.data;
  const allSessions = lib ? [...lib.pinned, ...lib.projects.flatMap((p) => p.sessions), ...lib.scheduled, ...lib.recent, ...lib.archived] : null;
  // back to the last conversation (if it still exists), else the newest
  useEffect(() => {
    if (sessionId !== undefined || !allSessions) return;
    const list = allSessions;
    const newest = [...list].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))[0];
    pickyApi
      .uiGet<number>(LAST)
      .catch(() => null)
      .then((last) => setSessionId(list.some((s) => s.id === last) ? last : (newest?.id ?? null)));
  }, [sessionId, lib]); // eslint-disable-line react-hooks/exhaustive-deps

  const current = sessionId ?? null;
  // a 定时任务 wrote something meanwhile: it shows in the list
  useEffect(() => {
    void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
  }, [status?.unseen, (status?.active ?? []).map((a) => a.session).join()]); // eslint-disable-line react-hooks/exhaustive-deps
  // opening a 定时 conversation takes its dot away
  const unseenHere = !!allSessions?.find((s) => s.id === current)?.unseen;
  useEffect(() => {
    if (current == null || !unseenHere) return;
    void pickyApi.seen(current).then(() => Promise.all([qc.invalidateQueries({ queryKey: LIBRARY_KEY }), qc.invalidateQueries({ queryKey: STATUS_KEY })]));
  }, [current, unseenHere]); // eslint-disable-line react-hooks/exhaustive-deps
  const open = (id: number | null) => {
    setSessionId(id);
    setDraftProject(null);
    if (id != null) pickyApi.uiSet(LAST, id).catch(() => {});
  };
  const { live, busyHere, run, shownWith, writing } = useLiveReply({ current, status, onSent: open, onStart: () => transcript.current?.toBottom() });
  const activeHere = activeIn(status, current);
  const messages = useQuery({
    queryKey: MESSAGES(current),
    queryFn: () => (current == null ? Promise.resolve([] as ChatMessage[]) : pickyApi.messages(current)),
    enabled: sessionId !== undefined,
    // reloaded while a reply is being written: follow the saved checkpoints
    refetchInterval: activeHere && !live ? 1000 : false,
  });

  const stop = () => {
    const runId = live?.runId ?? (summary_?.session === current ? summary_.run : null) ?? activeHere?.run;
    if (runId != null) void pickyApi.stop(runId).catch((e) => toast(errorText(e), "error"));
  };

  const switchVersion = (m: ChatMessage, delta: -1 | 1) =>
    void pickyApi.switchVersion(m.id, delta).then(
      () => qc.invalidateQueries({ queryKey: MESSAGES(current) }),
      (e) => toast(errorText(e), "error"),
    );

  const newChat = (projectId: number | null = null) => {
    open(null);
    setDraftProject(projectId);
    setTimeout(() => composer.current?.focus());
  };

  /** Opens a conversation at a message (showing that message's version first). */
  const openAt = async (sessionId: number, messageId: number | null, query: string) => {
    try {
      if (messageId != null) {
        await pickyApi.reveal(messageId);
        await qc.invalidateQueries({ queryKey: MESSAGES(sessionId) });
        setReveal({ id: messageId, query, key: Date.now() });
      }
      open(sessionId);
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  useOpenRequest((h) => void openAt(h.id, h.parent, ""));

  // the open conversation was deleted (or undone away): back to a fresh draft (an open 助手 stays)
  useEffect(() => {
    if (allSessions && current != null && !live && !allSessions.some((s) => s.id === current)) {
      setSessionId(null);
      setDraftProject(null);
    }
  }); // eslint-disable-line react-hooks/exhaustive-deps

  usePageKeys({
    onNew: () => newChat(),
    onFind: () => transcript.current?.openFind(),
    onEscape: () => transcript.current?.closeFind(),
  });

  // the saved path, with the reply being written laid over it (a new question shows at once)
  const saved = messages.data ?? [];
  const shown = shownWith(saved);

  const summarizingHere = (summary_ != null && summary_.session === current) || activeHere?.kind === "summary";
  const draftKey = current == null ? `new:${draftProject ?? ""}` : `s${current}`;
  const ask = useAskedSkills(draftKey);
  const web = ask.skills.find((s) => s.key === WEB_SKILL);
  const thinking = useThinking(current, draftKey);
  const knowledge = useKnowledge(current, draftKey, draftProject);
  const draftSetting = draftSettings[draftKey] ?? NO_SETTING;

  // the project the open conversation (or draft) is in, and its files
  const rawProject = current == null ? draftProject : (allSessions?.find((x) => x.id === current)?.projectId ?? null);
  const projectId = rawProject != null && lib?.projects.some((x) => x.id === rawProject) ? rawProject : null;
  const projectFiles = useProjectFiles(projectId).data ?? [];
  const atts = attach[draftKey] ?? [];
  // chosen files: this draft's choice, else what the last question used (still readable)
  const lastUsed = [...saved].reverse().find((m) => m.role === "user")?.files.flatMap((f) => (f.fileId != null ? [f.fileId] : [])) ?? [];
  const sel = (fileSel[draftKey] ?? lastUsed).filter((id, i, all) => all.indexOf(id) === i && projectFiles.some((f) => f.id === id && usableFile(f)));
  const selFiles = sel.flatMap((id) => projectFiles.filter((f) => f.id === id));
  const toggleFile = (id: number) => setFileSel((m) => ({ ...m, [draftKey]: sel.includes(id) ? sel.filter((x) => x !== id) : [...sel, id] }));
  const plan = useQuery({
    queryKey: ["picky", "retrieval-plan", current, current == null ? draftProject : null, sel, projectFiles.map((f) => `${f.id}:${f.index.kind}`).join(), knowledge.on, knowledge.own.join(), status?.selection],
    queryFn: () => pickyApi.retrievalPlan(current, current == null ? draftProject : null, sel, knowledge.forSend),
    enabled: projectId != null || knowledge.on,
  });

  const keyRef = useRef(draftKey);
  keyRef.current = draftKey;
  const addToDraft = (files: FileInfo[], key = keyRef.current) =>
    setAttach((m) => {
      const have = m[key] ?? [];
      return { ...m, [key]: [...have, ...files.filter((f) => !have.some((h) => h.id === f.id))] };
    });
  const removeAttachment = (id: number) => setAttach((m) => ({ ...m, [draftKey]: atts.filter((a) => a.id !== id) }));
  /** An image the local model can't see says so on its chip. */
  const unseen = (f: FileInfo) => (f.format === "图片" && !status?.selection.startsWith("cloud:") ? "本地模型看不了图片" : null);
  // files dropped on the window become attachments of the open draft
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((e) => {
          const d = e.payload;
          if (d.type === "enter" || d.type === "over") setDropping(true);
          else if (d.type === "leave") setDropping(false);
          else {
            setDropping(false);
            const key = keyRef.current;
            void importPaths(d.paths).then((fs) => addToDraft(fs, key));
          }
        }),
      )
      .then((f) => (alive ? (unlisten = f) : f()))
      .catch(() => {});
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const draftOff = current == null && !!draftMemoryOff[draftKey];
  useEffect(() => setNextMemoryOn(true), [draftKey]);
  const memoryPlan = useQuery({
    queryKey: [...MEMORIES_KEY, "plan", current, current == null ? draftProject : null, draftOff, nextMemoryOn, status?.selection],
    queryFn: () => pickyApi.memoryPlan(current, current == null ? draftProject : null, draftOff, !nextMemoryOn),
  });

  const prep = usePrepare({ current, draftProject, draftOff, projectId, knowledge: knowledge.forSend, knowledgeOn: knowledge.on });
  const { prepareFor, failCard, preparing } = prep;
  // this conversation is writing (others may be too: each has its own)
  const busy = busyHere || summarizingHere || preparing != null;

  // how full the conversation is with the draft as the next question (debounced; stale answers dropped)
  const prev = useRef<Assessment | null>(null);
  const shape = saved.map((m) => `${m.id}:${m.state}:${m.content.length}`).join(",");
  useEffect(() => {
    setDismissed(false);
    prev.current = null;
    setAssessment(null);
  }, [current]);
  useEffect(() => {
    if (current == null || busy) return setAssessment(null);
    let live = true;
    const t = setTimeout(() => {
      pickyApi.context(current, draftText).then(
        (a) => {
          if (!live) return;
          const p = prev.current;
          if (a && (!a.fits || (p != null && a.dropped > p.dropped))) setDismissed(false);
          prev.current = a;
          setAssessment(a);
        },
        () => live && setAssessment(null),
      );
    }, 300);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [current, draftText, busy, shape, status?.selection, status?.local.active?.key]); // eslint-disable-line react-hooks/exhaustive-deps

  const summarize = async () => {
    if (current == null) return;
    const session = current;
    const waiting = composer.current?.text().trim() ?? "";
    setSummary({ session, run: null });
    try {
      const run = await pickyApi.summarize(session, (e) => {
        setSummary(null);
        setAssessment(null);
        prev.current = null;
        void qc.invalidateQueries({ queryKey: STATUS_KEY });
        if (e.state === "completed") {
          setDismissed(false);
          if (waiting) setContinueAfter({ session, text: waiting });
        } else if (e.error) toast(e.error, "error");
      });
      setSummary((x) => (x && x.session === session ? { session, run } : x));
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
    } catch (err) {
      setSummary(null);
      toast(errorText(err), "error");
    }
  };
  useEffect(() => {
    if (!continueAfter || busy) return;
    setContinueAfter(null);
    if (current === continueAfter.session && composer.current?.text().trim() === continueAfter.text) composer.current.send();
  }, [continueAfter, busy, current]);

  const sendText = async (text: string) => {
    const remember = parseRemember(text);
    if (remember) {
      setCapture({ ...remember, projectId });
      return false; // the draft stays until the card is saved
    }
    const key = draftKey;
    const files = sel;
    prep.clearFail();
    const got = await prepareFor(text || EMPTY_QUESTION, files, !nextMemoryOn);
    if ("error" in got) {
      failCard(got, () => composer.current?.send());
      return false;
    }
    const shownFiles = [...atts.map((a) => asMessageFile(a)), ...selFiles.flatMap((f) => (currentFile(f) ? [asMessageFile(currentFile(f)!, "资料", f.id)] : []))];
    const extras = { attachments: atts.map((a) => a.id), files, passages: got.passages, memories: got.memories };
    const sent = await run(
      "send",
      text || EMPTY_QUESTION,
      (on) => pickyApi.send(current, current == null ? draftProject : null, current == null ? draftSetting : null, draftOff, thinking.forSend, text, extras, null, ask.asked, knowledge.forSend, on),
      shownFiles,
    );
    if (!sent) return false;
    ask.clear(key);
    knowledge.clear(key);
    setNextMemoryOn(true);
    setAttach(({ [key]: _, ...rest }) => rest);
    if (key.startsWith("new:")) {
      setDraftSettings(({ [key]: _, ...rest }) => rest);
      setDraftMemoryOff(({ [key]: _, ...rest }) => rest);
      setFileSel(({ [key]: _, ...rest }) => ({ ...rest, [`s${sent.sessionId}`]: files }));
    }
    return true;
  };

  /** /记住 confirmed: save, then send the rest of the draft. */
  const saveRemember = async (entries: string[], project: number | null, localOnly: boolean) => {
    const rest = capture?.remainder ?? "";
    await pickyApi.remember(entries, project, localOnly, current);
    void qc.invalidateQueries({ queryKey: MEMORIES_KEY });
    setCapture(null);
    toast(entries.length > 1 ? `记住了 ${entries.length} 条` : "记住了");
    composer.current?.setText(rest);
    if (rest) setTimeout(() => composer.current?.send());
  };

  /** An edit (text) or a regenerate / retry (null) of `m`, with passages for its question. */
  const versionRun = async (m: ChatMessage, text: string | null) => {
    const q = text != null ? m : saved[saved.findIndex((x) => x.id === m.id) - 1];
    const exclude = (q?.files ?? []).flatMap((f) => (f.fileId != null ? [f.fileId] : []));
    prep.clearFail();
    // regenerate / retry use memories whenever the conversation allows; an edit follows 「本条使用记忆」
    const got = await prepareFor(text ?? q?.content ?? "", exclude, text != null && !nextMemoryOn);
    if ("error" in got) {
      failCard(got, () => void versionRun(m, text));
      return;
    }
    if (text != null) setNextMemoryOn(true);
    void run("version", text ?? "", (on) => (text != null ? pickyApi.edit(m.id, text, got.passages, got.memories, on) : pickyApi.regenerate(m.id, got.passages, got.memories, on)));
  };
  const pinnedTemplates = templates.filter((t) => t.pinned).slice(0, 4);
  const issue = status && status.selection === "local" ? status.local.issue : null;
  const ListToggle = collapsed ? PanelLeftOpen : PanelLeftClose;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <PageHeader
        icon={Bird}
        color="var(--module-picky)"
        title="小拾"
        right={
          <div className="flex items-center justify-end gap-2">
            <IconButton label={collapsed ? "展开对话列表" : "收起对话列表"} onClick={() => setCollapsed(!collapsed)}>
              <ListToggle size={16} strokeWidth={1.5} />
            </IconButton>
            <Button disabled={current == null && draftProject == null && !live} onClick={() => newChat()}>
              <SquarePen size={14} strokeWidth={1.5} /> 新对话
            </Button>
            {status && !status.error && <ModelMenu status={status} />}
          </div>
        }
      />
      {status?.error ? (
        <div className="flex flex-1 items-center justify-center px-16">
          <p className="selectable max-w-xl text-center text-sm text-destructive">{status.error}</p>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 gap-4 pb-5 pl-8">
          {!collapsed &&
            (searching ? (
              <ChatSearch onOpen={(h: ChatHit, q) => void openAt(h.sessionId, h.messageId, q)} onClose={() => setSearching(false)} />
            ) : (
              lib && (
                <ConversationList
                  library={lib}
                  current={current}
                  assistant={null}
                  onAssistant={(key) => tabs.open(`/${key}`)}
                  generating={writing}
                  onOpen={open}
                  onNewInProject={(p) => newChat(p)}
                  onSearch={() => setSearching(true)}
                  onManage={() => setManage("templates")}
                  onPlugins={() => setPlugins(true)}
                  onSchedules={() => setSchedules(true)}
                  onKnowledge={() => setKbOpen(true)}
                  onProjectFiles={setFilesFor}
                />
              )
            ))}
            <div className="flex min-w-0 flex-1 flex-col pr-0">
              <Transcript
                ref={transcript}
                sessionKey={String(current)}
                messages={shown}
                busy={busy}
                empty={
                  <div className="flex flex-col items-center gap-6">
                    <p>小拾在这儿，想聊点什么？</p>
                    {pinnedTemplates.length > 0 && (
                      <div className="grid grid-cols-2 gap-3 text-sm">
                        {pinnedTemplates.map((t) => (
                          <button
                            key={t.id}
                            type="button"
                            onClick={() => composer.current?.insert(t)}
                            className="flex min-h-[70px] w-60 flex-col items-start gap-1 rounded-card border bg-card px-3 py-2.5 text-left outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring"
                          >
                            <span className="flex w-full items-center gap-1.5 font-bold">
                              <Pin size={13} strokeWidth={1.75} className="shrink-0 text-muted-foreground" />
                              <span className="truncate">{t.title}</span>
                            </span>
                            <span className="line-clamp-2 text-xs leading-normal text-muted-foreground">{summary(t)}</span>
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                }
                onEdit={(m, text) => void versionRun(m, text)}
                onRegenerate={(m) => void versionRun(m, null)}
                onSwitch={switchVersion}
                onOpenFile={(id) => openFile(id, toast)}
                onStop={stop}
                reveal={reveal}
              />
              <div className="px-8 pt-2">
                <div className="mx-auto max-w-3xl space-y-2">
                  {issue && <IssueNotice issue={issue} compact />}
                  {summarizingHere ? (
                    <div className="rounded-card border bg-card px-3 py-2 text-sm">
                      <p className="font-bold">正在总结较早的内容…</p>
                    </div>
                  ) : (
                    assessment?.nearLimit &&
                    !dismissed && (
                      <div className="rounded-card border bg-card px-3 py-2 text-sm">
                        <p className="font-bold">这段对话已用掉约 {assessment.percent}% 的上下文</p>
                        <div className="mt-2 flex gap-2">
                          <Button className="py-1" disabled={!assessment.summarizable || busy} onClick={() => void summarize()}>
                            总结并继续
                          </Button>
                          <Button variant="ghost" className="py-1" onClick={() => setDismissed(true)}>
                            以后再说
                          </Button>
                        </div>
                      </div>
                    )
                  )}
                  <PrepNotices prep={prep} />
                  {current == null && draftProject != null && (
                    <p className="px-1 text-xs text-muted-foreground">
                      项目：
                      {lib?.projects.find((p) => p.id === draftProject)?.name}
                    </p>
                  )}
                  <Composer
                    ref={composer}
                    onPasteFiles={(files) => {
                      const key = draftKey;
                      void importPasted(files).then((fs) => addToDraft(fs, key));
                    }}
                    draftKey={draftKey}
                    busy={busy}
                    templates={templates}
                    onManageTemplates={() => setManage("templates")}
                    onTextChange={setDraftText}
                    allowEmpty={atts.length > 0}
                    dropping={dropping}
                    skills={ask.usable}
                    onAskSkill={ask.toggle}
                    chips={
                      (atts.length > 0 || selFiles.length > 0 || ask.asked.length > 0) && (
                        <div className="flex flex-wrap gap-1.5 px-3 pt-2.5">
                          <SkillChips skills={ask.skills} asked={ask.asked} onRemove={ask.toggle} />
                          {atts.map((f) => (
                            <FileChip key={f.id} file={f} note={unseen(f)} onOpen={() => openFile(f.id, toast)} onRemove={() => removeAttachment(f.id)} />
                          ))}
                          {selFiles.map((f) => {
                            const cur = currentFile(f)!;
                            return <FileChip key={`p${f.id}`} project file={cur} note={unseen(cur)} onOpen={() => openFile(cur.id, toast)} onRemove={() => toggleFile(f.id)} />;
                          })}
                        </div>
                      )
                    }
                    before={
                      <Dropdown<string | null>
                        plain
                        ariaLabel="附件"
                        value={null}
                        options={[
                          { value: "file", label: "添加文件…" },
                          { value: "web", label: "添加网页…" },
                          { value: "library", label: "附件库…" },
                        ]}
                        onChange={(v) => {
                          const key = draftKey;
                          if (v === "file") void choose(true).then((fs) => addToDraft(fs, key));
                          else if (v === "web") setWebOpen(true);
                          else if (v === "library") setLibraryOpen(true);
                        }}
                        renderTrigger={() => <Plus size={15} strokeWidth={1.75} />}
                        triggerClassName={toolClass}
                      />
                    }
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
                        <KnowledgeButton kb={knowledge} plan={plan.data?.knowledge ?? null} onManage={() => setKbOpen(true)} />
                        <InstructionsButton
                          sessionId={current}
                          projectId={current == null ? draftProject : null}
                          draft={draftSetting}
                          onDraftChange={(v) => setDraftSettings((m) => ({ ...m, [draftKey]: v }))}
                          busy={busy}
                          onManage={() => setManage("global")}
                          files={
                            projectId != null
                              ? {
                                  selected: selFiles.map((f) => currentFile(f)?.name ?? "资料"),
                                  plan: plan.data ?? null,
                                  cloud: !!status?.selection.startsWith("cloud:"),
                                  onManage: () => setFilesFor(projectId),
                                }
                              : null
                          }
                        />
                        <MemoryButton
                          plan={memoryPlan.data ?? null}
                          nextOn={nextMemoryOn}
                          conversationOff={memoryPlan.data?.conversationOff ?? false}
                          onNext={setNextMemoryOn}
                          onConversation={(off) => {
                            if (current == null) setDraftMemoryOff((m) => ({ ...m, [draftKey]: off }));
                            else
                              void pickyApi.conversationMemory(current, off).then(
                                () => qc.invalidateQueries({ queryKey: MEMORIES_KEY }),
                                (e) => toast(errorText(e), "error"),
                              );
                          }}
                          onManage={() => setManage("memories")}
                        />
                        {projectId != null && (
                          <FilesButton
                            files={projectFiles}
                            selected={sel}
                            onToggle={toggleFile}
                            onAdd={() => {
                              const key = draftKey;
                              void choose(true).then(async (fs) => {
                                const added: number[] = [];
                                for (const f of fs) {
                                  try {
                                    added.push(await pickyApi.projectFileAdd(projectId, f.id));
                                  } catch (e) {
                                    toast(errorText(e), "error");
                                  }
                                }
                                await qc.invalidateQueries({ queryKey: projectFilesKey(projectId) });
                                void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
                                setFileSel((m) => ({ ...m, [key]: [...sel, ...added] }));
                              });
                            }}
                            onManage={() => setFilesFor(projectId)}
                          />
                        )}
                      </>
                    }
                    onSend={sendText}
                    onStop={stop}
                  />
                </div>
              </div>
            </div>
        </div>
      )}
      {plugins && <PluginsDialog onClose={() => setPlugins(false)} />}
      {kbOpen && <KnowledgeDialog onClose={() => setKbOpen(false)} />}
      {schedules && <SchedulesDialog onOpenSession={open} onClose={() => setSchedules(false)} />}
      {manage && (
        <ManageDialog tab={manage} projects={lib?.projects ?? []} sessionIds={allSessions?.map((x) => x.id) ?? []} onOpenSession={open} onClose={() => setManage(null)} />
      )}
      {capture && (
        <RememberDialog entries={capture.entries} projects={lib?.projects ?? []} projectId={capture.projectId} onSave={saveRemember} onClose={() => setCapture(null)} />
      )}
      {webOpen && <WebDialog onAdded={(f) => addToDraft([f])} onClose={() => setWebOpen(false)} />}
      {libraryOpen && <AttachmentLibrary pickLabel="使用" disabled={(f) => atts.some((a) => a.id === f.id)} onPick={(f) => addToDraft([f])} onClose={() => setLibraryOpen(false)} />}
      {filesFor != null && (
        <ProjectFiles
          projectId={filesFor}
          projectName={lib?.projects.find((x) => x.id === filesFor)?.name ?? ""}
          onClose={() => {
            setFilesFor(null);
            void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
          }}
        />
      )}
    </div>
  );
}
