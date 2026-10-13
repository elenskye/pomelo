// What the 小拾 page and the 小窗 share about one conversation:
// - useLiveReply: a reply being written is laid over the saved path as it streams (a new question shows
//   at once); at most ~25 repaints a second; when the stream is lost (reload) the reading progress
//   comes from the status and the saved checkpoints are polled.
// - usePrepare: memories and project passages chosen before a question is saved (through 8081 when
//   there are any); if 8081 can't be used a card asks: send without, retry or cancel (PrepNotices).
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { Button } from "@/shared/ui";
import { LIBRARY_KEY, STATUS_KEY, pickyApi, type ChatMessage, type MessageFile, type Note, type Passage, type PickyStatus, type Query, type ReadingProgress, type Sent, type StreamEvent, type UsedMemory } from "./api";

export const messagesKey = (id: number | null) => ["picky", "messages", id];

type Live = {
  /** send: a new question (shown at once); version: an edit / regenerate (shown once the path has it). */
  kind: "send" | "version";
  prompt: string;
  sessionId: number | null;
  messageId: number | null;
  runId: number | null;
  content: string;
  reasoning: string;
  reasoningStart: number | null;
  reasoningMs: number | null;
  done: boolean;
  /** A new question's files and page (shown at once with it). */
  files: MessageFile[];
  page: Query | null;
  reading: ReadingProgress | null;
  notes: Note[];
  /** Lookups in Pomelo so far. */
  queries: Query[];
};

export function useLiveReply(opts: { current: number | null; status: PickyStatus | undefined; onSent: (sessionId: number) => void; onStart?: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const [live, setLive] = useState<Live | null>(null);
  const liveRef = useRef<Live | null>(null);
  const paintTimer = useRef(0);
  const { current, status } = opts;
  const generating = !!status?.generating;

  // at most ~25 repaints a second while text streams in (a timer: animation frames pause when hidden)
  const paint = useCallback(() => {
    if (paintTimer.current) return;
    paintTimer.current = window.setTimeout(() => {
      paintTimer.current = 0;
      setLive(liveRef.current && { ...liveRef.current });
    }, 40);
  }, []);
  useEffect(() => () => clearTimeout(paintTimer.current), []);

  const finishLive = useCallback(
    async (l: Live) => {
      if (l.sessionId != null) await qc.invalidateQueries({ queryKey: messagesKey(l.sessionId) });
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
      if (liveRef.current === l) {
        liveRef.current = null;
        setLive(null);
      }
    },
    [qc],
  );

  const onEvent = useCallback(
    (e: StreamEvent) => {
      const l = liveRef.current;
      if (!l) return;
      if (e.kind === "reasoning") {
        l.reasoningStart ??= Date.now();
        l.reasoning += e.text;
      } else if (e.kind === "content") {
        if (l.reasoningStart != null && l.reasoningMs == null) l.reasoningMs = Date.now() - l.reasoningStart;
        l.content += e.text;
      } else if (e.kind === "reading") {
        l.reading = e.total > 0 ? { current: e.current, total: e.total, label: e.label } : null;
      } else if (e.kind === "note") {
        const i = l.notes.findIndex((n) => n.label === e.label);
        if (i >= 0) l.notes[i] = { label: e.label, content: e.content };
        else l.notes.push({ label: e.label, content: e.content });
      } else if (e.kind === "query") {
        const { kind: _, ...q } = e;
        l.queries.push(q);
      } else {
        l.done = true;
        l.reasoningMs = e.reasoningMs ?? l.reasoningMs;
        if (l.messageId != null) void finishLive(l); // else: run() finishes once it knows the ids
      }
      paint();
    },
    [finishLive, paint],
  );

  /** Starts a reply (send / edit / regenerate) and lays it over the path as it streams. */
  const run = async (kind: Live["kind"], prompt: string, call: (on: (e: StreamEvent) => void) => Promise<Sent>, files: MessageFile[] = [], page: Query | null = null): Promise<Sent | null> => {
    if (liveRef.current || generating) return null;
    const l: Live = { kind, prompt, sessionId: current, messageId: null, runId: null, content: "", reasoning: "", reasoningStart: null, reasoningMs: null, done: false, files, page, reading: null, notes: [], queries: [] };
    liveRef.current = l;
    setLive({ ...l });
    opts.onStart?.();
    try {
      const sent = await call(onEvent);
      Object.assign(l, { sessionId: sent.sessionId, messageId: sent.messageId, runId: sent.runId });
      setLive({ ...l }); // the ids now, before the saved path (which has these messages) comes back
      opts.onSent(sent.sessionId);
      await qc.invalidateQueries({ queryKey: messagesKey(sent.sessionId) });
      void qc.invalidateQueries({ queryKey: LIBRARY_KEY });
      void qc.invalidateQueries({ queryKey: STATUS_KEY });
      if (l.done) void finishLive(l);
      else paint();
      return sent;
    } catch (err) {
      toast(errorText(err), "error");
      liveRef.current = null;
      setLive(null);
      return null;
    }
  };

  /** The saved path with the reply being written laid over it. */
  const shownWith = (saved: ChatMessage[]): ChatMessage[] => {
    let shown = saved;
    if (live && live.sessionId === current) {
      const overlay = (m: ChatMessage): ChatMessage => ({
        ...m,
        content: live.content,
        reasoning: live.reasoning,
        reasoningMs: live.reasoningMs,
        state: live.done ? m.state : "streaming",
        reading: live.done ? null : live.reading,
        notes: live.notes.length ? live.notes : m.notes,
        queries: live.queries.length ? [...live.queries] : m.queries,
      });
      if (live.messageId != null && saved.some((m) => m.id === live.messageId)) shown = saved.map((m) => (m.id === live.messageId ? overlay(m) : m));
      else if (live.kind === "send") {
        const base = { createdAt: "", model: null, error: null, reasoning: "", reasoningMs: null, versionIndex: 1, versionCount: 1, notes: [], memories: [], queries: [], page: null };
        shown = [
          ...saved,
          { ...base, id: -1, role: "user", content: live.prompt, state: "complete", files: live.files, page: live.page },
          overlay({ ...base, id: -2, role: "assistant", content: "", state: "streaming", files: [] }),
        ];
      }
    }
    // the stream was lost (the page reloaded meanwhile): the reading progress comes from the status
    if ((!live || live.sessionId !== current) && current != null && status?.activeSession === current && status.activeReading) {
      const r = status.activeReading;
      shown = shown.map((m) => (m.role === "assistant" && m.state === "streaming" ? { ...m, reading: r } : m));
    }
    return shown;
  };

  return { live, run, shownWith };
}

type PrepFail = { title: string; withoutLabel: string; message: string; without: () => void; retry: () => void };
export type Prepared = { passages: Passage[]; memories: UsedMemory[] } | { error: string; memory: boolean; retrieval: boolean };

/** `draftProject` / `draftOff`: a new conversation's project and 「本会话关闭记忆」. */
export function usePrepare(opts: { current: number | null; draftProject: number | null; draftOff: boolean; projectId: number | null }) {
  const [preparing, setPreparing] = useState<string | null>(null);
  const [prepFail, setPrepFail] = useState<PrepFail | null>(null);
  const skipNext = useRef(false);
  const { current, draftProject, draftOff, projectId } = opts;

  /** Memories and passages for a question (through 8081 when there are any); an error = 8081 couldn't be used. */
  const prepareFor = async (question: string, exclude: number[], nextOff: boolean): Promise<Prepared> => {
    if (skipNext.current) {
      skipNext.current = false;
      return { passages: [], memories: [] };
    }
    const proj = current == null ? draftProject : null;
    let memory = false;
    let retrieval = false;
    try {
      const [mp, rp] = await Promise.all([
        pickyApi.memoryPlan(current, proj, draftOff, nextOff),
        projectId != null ? pickyApi.retrievalPlan(current, proj, exclude) : Promise.resolve(null),
      ]);
      memory = mp.off == null && mp.count > 0;
      retrieval = !!rp?.readyNames.length;
      if (!memory && !retrieval) return { passages: [], memories: [] };
      setPreparing(memory && retrieval ? "正在挑选记忆并检索资料…" : retrieval ? "正在检索项目资料…" : "正在挑选相关记忆…");
      const memories = memory ? await pickyApi.chooseMemories(current, proj, draftOff, nextOff, question) : [];
      const passages = retrieval ? await pickyApi.retrieve(current, proj, question, exclude) : [];
      return { passages, memories };
    } catch (e) {
      return { error: errorText(e), memory, retrieval };
    } finally {
      setPreparing(null);
    }
  };
  /** Shows the card for a failed preparation; `again` repeats the send (once without, or as is). */
  const failCard = (f: { error: string; memory: boolean; retrieval: boolean }, again: () => void) =>
    setPrepFail({
      title: f.memory && f.retrieval ? "无法挑选记忆和检索资料" : f.retrieval ? "无法检索项目资料" : "无法挑选记忆",
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
