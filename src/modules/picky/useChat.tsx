// What the 小拾 page and the 小窗 share about conversations:
// - useLiveReply: replies being written are laid over their saved paths as they stream (a new question
//   shows at once). Each conversation has its own (several write side by side; a reply waiting for the
//   local model shows 排队中); at most ~25 repaints a second; when the stream is lost (reload) the
//   reading progress and the queue come from the status and the saved checkpoints are polled.
// - usePrepare: memories and project / 知识库 passages chosen before a question is saved (through 8081 when
//   there are any); if 8081 can't be used a card asks: send without, retry or cancel (PrepNotices).
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { LIBRARY_KEY, STATUS_KEY, pickyApi, type ActiveRun, type KbDraft, type ChatMessage, type MessageFile, type Note, type Passage, type PickyStatus, type Query, type ReadingProgress, type Sent, type Source, type StreamEvent, type Card, type FileEdit, type UsedMemory } from "./api";

export const messagesKey = (id: number | null) => ["picky", "messages", id];

type Live = {
  /** send: a new question (shown at once); version: an edit / regenerate (shown once the path has it). */
  kind: "send" | "version";
  prompt: string;
  /** null: a new conversation, until the question is saved. */
  sessionId: number | null;
  messageId: number | null;
  runId: number | null;
  content: string;
  reasoning: string;
  reasoningStart: number | null;
  reasoningMs: number | null;
  done: boolean;
  /** Waiting for the local model. */
  queued: boolean;
  /** A new question's files and page (shown at once with it). */
  files: MessageFile[];
  page: Query | null;
  reading: ReadingProgress | null;
  notes: Note[];
  /** Lookups in Pomelo so far. */
  queries: Query[];
  /** Numbered sources so far (the whole list each time). */
  sources: Source[];
  /** What it proposed to write so far. */
  card: Card | null;
  /** 改文件: the changes it proposed. */
  edit: FileEdit | null;
};

/** What the status says is being written in `session` (a reply or a summary). */
export const activeIn = (status: PickyStatus | undefined, session: number | null): ActiveRun | null => (session == null ? null : (status?.active.find((a) => a.session === session) ?? null));

export function useLiveReply(opts: { current: number | null; status: PickyStatus | undefined; onSent: (sessionId: number) => void; onStart?: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [lives, setLives] = useState<Live[]>([]);
  const livesRef = useRef<Live[]>([]);
  const paintTimer = useRef(0);
  const { current, status } = opts;

  const repaint = useCallback(() => setLives(livesRef.current.map((l) => ({ ...l }))), []);
  // at most ~25 repaints a second while text streams in (a timer: animation frames pause when hidden)
  const paint = useCallback(() => {
    if (paintTimer.current) return;
    paintTimer.current = window.setTimeout(() => {
      paintTimer.current = 0;
      repaint();
    }, 40);
  }, [repaint]);
  useEffect(() => () => clearTimeout(paintTimer.current), []);

  // replies this page didn't start (a 定时任务, or from before a reload) ended: show how they ended
  const lastActive = useRef<ActiveRun[]>([]);
  const activeKey = (status?.active ?? []).map((a) => a.run).join();
  useEffect(() => {
    const now = status?.active ?? [];
    const ended = lastActive.current.filter((a) => !now.some((x) => x.run === a.run));
    ended.forEach((a) => void qc.invalidateQueries({ queryKey: messagesKey(a.session) }));
    if (ended.length) void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
    lastActive.current = now;
  }, [activeKey]); // eslint-disable-line react-hooks/exhaustive-deps

  const finishLive = useCallback(
    async (l: Live) => {
      if (l.sessionId != null) await qc.invalidateQueries({ queryKey: messagesKey(l.sessionId) });
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
      if (livesRef.current.includes(l)) {
        livesRef.current = livesRef.current.filter((x) => x !== l);
        repaint();
      }
    },
    [qc, repaint],
  );

  /** The events of one reply, into its own live state. */
  const handler = (l: Live) => (e: StreamEvent) => {
    if (!livesRef.current.includes(l)) return;
    if (e.kind === "reasoning") {
      l.queued = false;
      l.reasoningStart ??= Date.now();
      l.reasoning += e.text;
    } else if (e.kind === "content") {
      l.queued = false;
      if (l.reasoningStart != null && l.reasoningMs == null) l.reasoningMs = Date.now() - l.reasoningStart;
      l.content += e.text;
    } else if (e.kind === "queued") {
      l.queued = e.on;
    } else if (e.kind === "reading") {
      l.reading = e.total > 0 ? { current: e.current, total: e.total, label: e.label } : null;
    } else if (e.kind === "note") {
      const i = l.notes.findIndex((n) => n.label === e.label);
      if (i >= 0) l.notes[i] = { label: e.label, content: e.content };
      else l.notes.push({ label: e.label, content: e.content });
    } else if (e.kind === "query") {
      const { kind: _, ...q } = e;
      l.queries.push(q);
    } else if (e.kind === "sources") {
      l.sources = e.list;
    } else if (e.kind === "card") {
      l.card = e.card;
    } else if (e.kind === "earlierCard") {
      // the message that has it, in the saved path (reloaded once the reply ends anyway)
      if (l.sessionId != null) qc.setQueryData<ChatMessage[]>(messagesKey(l.sessionId), (list) => list?.map((m) => (m.card?.runId === e.card.runId ? { ...m, card: e.card } : m)));
    } else if (e.kind === "edit") {
      l.edit = e.edit;
    } else {
      l.done = true;
      l.queued = false;
      l.reasoningMs = e.reasoningMs ?? l.reasoningMs;
      if (l.messageId != null) void finishLive(l); // else: run() finishes once it knows the ids
    }
    paint();
  };

  /** The reply being written in the open conversation (or the new question of a draft). */
  const live = lives.find((l) => l.sessionId === current) ?? null;
  /** The open conversation is writing (here, or per the status). */
  const busyHere = live != null || activeIn(status, current) != null;

  /** Starts a reply (send / edit / regenerate) and lays it over the path as it streams. */
  const run = async (kind: Live["kind"], prompt: string, call: (on: (e: StreamEvent) => void) => Promise<Sent>, files: MessageFile[] = [], page: Query | null = null): Promise<Sent | null> => {
    if (livesRef.current.some((l) => l.sessionId === current) || activeIn(status, current)) return null;
    const l: Live = { kind, prompt, sessionId: current, messageId: null, runId: null, content: "", reasoning: "", reasoningStart: null, reasoningMs: null, done: false, queued: false, files, page, reading: null, notes: [], queries: [], sources: [], card: null, edit: null };
    livesRef.current = [...livesRef.current, l];
    repaint();
    opts.onStart?.();
    try {
      const sent = await call(handler(l));
      Object.assign(l, { sessionId: sent.sessionId, messageId: sent.messageId, runId: sent.runId });
      repaint(); // the ids now, before the saved path (which has these messages) comes back
      opts.onSent(sent.sessionId);
      await qc.invalidateQueries({ queryKey: messagesKey(sent.sessionId) });
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
      if (l.done) void finishLive(l);
      else paint();
      return sent;
    } catch (err) {
      toast(errorText(err), "error");
      livesRef.current = livesRef.current.filter((x) => x !== l);
      repaint();
      return null;
    }
  };

  /** The saved path with the reply being written laid over it. */
  const shownWith = (saved: ChatMessage[]): ChatMessage[] => {
    let shown = saved;
    if (live) {
      const overlay = (m: ChatMessage): ChatMessage => ({
        ...m,
        content: live.content,
        reasoning: live.reasoning,
        reasoningMs: live.reasoningMs,
        state: live.done ? m.state : "streaming",
        queued: !live.done && live.queued,
        reading: live.done ? null : live.reading,
        notes: live.notes.length ? live.notes : m.notes,
        queries: live.queries.length ? [...live.queries] : m.queries,
        sources: live.sources.length ? live.sources : m.sources,
        card: live.card ?? m.card,
        edit: live.edit ?? m.edit,
      });
      if (live.messageId != null && saved.some((m) => m.id === live.messageId)) shown = saved.map((m) => (m.id === live.messageId ? overlay(m) : m));
      else if (live.kind === "send") {
        const base = { createdAt: "", model: null, error: null, reasoning: "", reasoningMs: null, versionIndex: 1, versionCount: 1, notes: [], memories: [], queries: [], sources: [], card: null, page: null };
        shown = [
          ...saved,
          { ...base, id: -1, role: "user", content: live.prompt, state: "complete", files: live.files, page: live.page },
          overlay({ ...base, id: -2, role: "assistant", content: "", state: "streaming", files: [] }),
        ];
      }
    }
    // the stream was lost (the page reloaded meanwhile): the reading progress and the queue come from the status
    const a = activeIn(status, current);
    if (!live && a && a.kind === "reply") shown = shown.map((m) => (m.role === "assistant" && m.state === "streaming" ? { ...m, reading: a.reading, queued: a.queued } : m));
    return shown;
  };

  /** Conversations writing now (this page's and the status's): the dots in the list. */
  const writing = [...new Set([...lives.flatMap((l) => (l.sessionId != null && !l.done ? [l.sessionId] : [])), ...(status?.active ?? []).map((a) => a.session)])];

  return { live, busyHere, run, shownWith, writing };
}

type PrepFail = { title: string; withoutLabel: string; message: string; without: () => void; retry: () => void };
/** `retrieval`: what it searched (项目资料 / 知识库 / both), if anything. */
export type Prepared = { passages: Passage[]; memories: UsedMemory[] } | { error: string; memory: boolean; retrieval: string | null };

/** `draftProject` / `draftOff`: a new conversation's project and 「本会话关闭记忆」; `knowledge`: a draft's own 知识库
 *  switch and libraries, `knowledgeOn`: the conversation's switch. */
export function usePrepare(opts: { current: number | null; draftProject: number | null; draftOff: boolean; projectId: number | null; knowledge: KbDraft | null; knowledgeOn: boolean }) {
  const [preparing, setPreparing] = useState<string | null>(null);
  const [prepFail, setPrepFail] = useState<PrepFail | null>(null);
  const skipNext = useRef(false);
  const { current, draftProject, draftOff, projectId, knowledge, knowledgeOn } = opts;

  /** Memories and passages for a question (through 8081 when there are any); an error = 8081 couldn't be used. */
  const prepareFor = async (question: string, exclude: number[], nextOff: boolean): Promise<Prepared> => {
    if (skipNext.current) {
      skipNext.current = false;
      return { passages: [], memories: [] };
    }
    const proj = current == null ? draftProject : null;
    let memory = false;
    let retrieval: string | null = null;
    try {
      const [mp, rp] = await Promise.all([
        pickyApi.memoryPlan(current, proj, draftOff, nextOff),
        projectId != null || knowledgeOn ? pickyApi.retrievalPlan(current, proj, exclude, knowledge) : Promise.resolve(null),
      ]);
      memory = mp.off == null && mp.count > 0;
      const files = !!rp?.readyNames.length;
      const kb = rp?.knowledge ?? null;
      const searchKb = !!kb && kb.ready + kb.pending > 0;
      retrieval = files && searchKb ? "项目资料和知识库" : searchKb ? "知识库" : files ? "项目资料" : null;
      if (!memory && !retrieval) return { passages: [], memories: [] };
      const doing = kb?.pending ? `更新并检索${retrieval}` : `检索${retrieval}`;
      setPreparing(memory && retrieval ? `正在挑选记忆并${doing}…` : retrieval ? `正在${doing}…` : "正在挑选相关记忆…");
      const memories = memory ? await pickyApi.chooseMemories(current, proj, draftOff, nextOff, question) : [];
      const passages = retrieval ? await pickyApi.retrieve(current, proj, question, exclude, knowledge) : [];
      return { passages, memories };
    } catch (e) {
      return { error: errorText(e), memory, retrieval };
    } finally {
      setPreparing(null);
    }
  };
  /** Shows the card for a failed preparation; `again` repeats the send (once without, or as is). */
  const failCard = (f: { error: string; memory: boolean; retrieval: string | null }, again: () => void) =>
    setPrepFail({
      title: f.memory && f.retrieval ? `无法挑选记忆和检索${f.retrieval}` : f.retrieval ? `无法检索${f.retrieval}` : "无法挑选记忆",
      withoutLabel: f.memory && f.retrieval ? "不用记忆和检索发送" : f.retrieval ? "不检索直接发送" : "不用记忆发送",
      message: f.error,
      without: () => {
        skipNext.current = true;
        again();
      },
      retry: again,
    });
  return { prepareFor, failCard, preparing, prepFail, clearFail: () => setPrepFail(null) };
}

/** 「正在挑选相关记忆…」 and the card when 8081 couldn't be used. */
export function PrepNotices(props: { prep: ReturnType<typeof usePrepare> }) {
  const { preparing, prepFail, clearFail } = props.prep;
  return (
    <>
      {prepFail && (
        <div className="rounded-card border bg-card px-3 py-2 text-sm">
          <p className="font-bold text-destructive">{prepFail.title}</p>
          <p className="mt-0.5 text-muted-foreground">{prepFail.message}</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <Button
              className="py-1"
              onClick={() => {
                clearFail();
                prepFail.without();
              }}
            >
              {prepFail.withoutLabel}
            </Button>
            <Button
              className="py-1"
              onClick={() => {
                clearFail();
                prepFail.retry();
              }}
            >
              重试
            </Button>
            <Button variant="ghost" className="py-1" onClick={clearFail}>
              取消
            </Button>
          </div>
        </div>
      )}
      {preparing && <p className="animate-pulse px-1 text-xs text-muted-foreground">{preparing}</p>}
    </>
  );
}
