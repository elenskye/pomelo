// 速记小窗 · 问柚子 (Rust capture.rs): chats right in the window, in a conversation of its own (out of 柚子's
// list and searches); 新对话 throws it away, 「在 Pomelo 里继续对话」 lists it and opens it on the 柚子 page.
// The 柚子 page's model (switched here too) and memories; 思考 as a new conversation's. Records a reply
// links open in the main window. Loaded only when the tab is (it brings 柚子's Markdown).
import { useEffect, useRef, useState, type Ref } from "react";
import { createPortal } from "react-dom";
import { MemoryRouter, useLocation, useNavigate } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Send, Square, SquarePen } from "lucide-react";
import { errorText, useToast } from "@/shared/feedback";
import { Button, IconButton, TextArea } from "@/shared/ui";
import { pickyApi, usePickyStatus, type ChatMessage } from "@/modules/picky/api";
import { toolClass } from "@/modules/picky/Composer";
import { openFile } from "@/modules/picky/Files";
import { IssueNotice, ModelMenu } from "@/modules/picky/ModelMenu";
import { ThinkingButton } from "@/modules/picky/Thinking";
import { Transcript, type TranscriptHandle } from "@/modules/picky/Transcript";
import { PrepNotices, activeIn, messagesKey, useLiveReply, usePrepare } from "@/modules/picky/useChat";
import { captureApi } from "./api";

const CHAT_KEY = ["capture", "chat"];

type Props = {
  text: string;
  onText: (text: string) => void;
  inputRef: Ref<HTMLTextAreaElement>;
  /** The header's place for the model menu. */
  slot: HTMLElement | null;
};

export default function CaptureChat(props: Props) {
  return (
    <MemoryRouter>
      <Forward />
      <Chat {...props} />
    </MemoryRouter>
  );
}

/** A record opened from the transcript (it navigates, as on the 柚子 page) goes to the main window. */
function Forward() {
  const loc = useLocation();
  const navigate = useNavigate();
  const toast = useToast();
  const hit = (loc.state as { open?: unknown } | null)?.open;
  useEffect(() => {
    if (!hit) return;
    void navigate("/", { replace: true, state: null });
    captureApi.open(loc.pathname, hit).catch((e) => toast(errorText(e), "error"));
  }, [hit]); // eslint-disable-line react-hooks/exhaustive-deps
  return null;
}

function Chat(props: Props) {
  const qc = useQueryClient();
  const toast = useToast();
  const status = usePickyStatus().data;
  const chat = useQuery({ queryKey: CHAT_KEY, queryFn: captureApi.chat });
  const current = chat.data?.id ?? null;
  const [draftThinking, setDraftThinking] = useState(true);
  const thinking = chat.data ? chat.data.thinking : draftThinking;
  const [draftEffort, setDraftEffort] = useState<string | null>(null);
  const effort = chat.data ? (chat.data.effort ?? null) : draftEffort;
  const transcript = useRef<TranscriptHandle>(null);
  const { live, busyHere, run, shownWith, followUp } = useLiveReply({
    current,
    status,
    // the window's conversation now (set at once: the reply being written follows it)
    onSent: (id) => qc.setQueryData(CHAT_KEY, { id, thinking, effort }),
    onStart: () => transcript.current?.toBottom(),
  });
  const activeHere = activeIn(status, current);
  const messages = useQuery({
    queryKey: messagesKey(current),
    queryFn: () => (current == null ? Promise.resolve([] as ChatMessage[]) : pickyApi.messages(current)),
    refetchInterval: activeHere && !live ? 1000 : false,
  });
  const saved = messages.data ?? [];
  const shown = shownWith(saved);
  const prep = usePrepare({ current, draftProject: null, draftOff: false, projectId: null, knowledge: null, knowledgeOn: false });
  const busy = busyHere || prep.preparing != null;
  const talking = shown.length > 0;
  const issue = status && status.selection === "local" ? status.local.issue : null;

  // the window is taller while there's a conversation (and back when the tab goes)
  useEffect(() => void captureApi.fit(talking).catch(() => {}), [talking]);
  useEffect(() => () => void captureApi.fit(false).catch(() => {}), []);

  const send = async () => {
    const text = props.text.trim();
    if (!text || busy) return;
    prep.clearFail();
    const got = await prep.prepareFor(text, [], false);
    if ("error" in got) return prep.failCard(got, () => void send());
    props.onText("");
    const extras = { attachments: [], files: [], passages: got.passages, memories: got.memories };
    const sent = await run("send", text, (on) => pickyApi.send(current, null, null, false, current == null ? thinking : null, current == null ? effort : null, text, extras, null, [], null, on, true));
    if (!sent) props.onText(text);
  };
  const versionRun = async (m: ChatMessage, text: string | null) => {
    const q = text != null ? m : saved[saved.findIndex((x) => x.id === m.id) - 1];
    prep.clearFail();
    const got = await prep.prepareFor(text ?? q?.content ?? "", [], false);
    if ("error" in got) return prep.failCard(got, () => void versionRun(m, text));
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
  const toggleThinking = () => {
    if (!chat.data) return setDraftThinking(!thinking);
    void captureApi.chatThinking(!thinking).then(
      () => qc.invalidateQueries({ queryKey: CHAT_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  };
  const pickEffort = (next: string | null) => {
    if (!chat.data) return setDraftEffort(next);
    void captureApi.chatEffort(next).then(
      () => qc.invalidateQueries({ queryKey: CHAT_KEY }),
      (e) => toast(errorText(e), "error"),
    );
  };
  const clear = () =>
    void captureApi.chatClear().then(
      () => qc.setQueryData(CHAT_KEY, null),
      (e) => toast(errorText(e), "error"),
    );
  const keep = () =>
    void captureApi.chatKeep().then(
      () => qc.setQueryData(CHAT_KEY, null),
      (e) => toast(errorText(e), "error"),
    );

  return (
    <>
      {props.slot && status && !status.error && createPortal(<ModelMenu status={status} />, props.slot)}
      <div className="flex min-h-0 flex-1 flex-col gap-2">
        {status?.error ? (
          <p className="selectable flex-1 px-2 pt-6 text-center text-sm text-destructive">{status.error}</p>
        ) : (
          talking && (
            <Transcript
              ref={transcript}
              compact
              sessionKey={`capture-${current}`}
              messages={shown}
              busy={busy}
              onAskFollowUp={followUp}
              empty={null}
              onEdit={(m, text) => void versionRun(m, text)}
              onRegenerate={(m) => void versionRun(m, null)}
              onSwitch={switchVersion}
              onOpenFile={(id) => openFile(id, toast)}
              onStop={stop}
            />
          )
        )}
        {issue && <IssueNotice issue={issue} compact />}
        <PrepNotices prep={prep} />
        <TextArea
          ref={props.inputRef}
          value={props.text}
          onChange={(e) => props.onText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
              e.preventDefault();
              void send();
            }
          }}
          placeholder={talking ? "接着问……" : "问柚子……"}
          aria-label="问柚子"
          rows={talking ? 2 : undefined}
          className={talking ? "max-h-28 shrink-0" : "min-h-0 flex-1"}
          autoFocus
        />
      </div>
      <footer className="flex shrink-0 items-center gap-1.5">
        {talking && (
          <IconButton label="新对话" disabled={busy} onClick={clear}>
            <SquarePen size={15} strokeWidth={1.5} />
          </IconButton>
        )}
        <ThinkingButton on={thinking} effort={effort} onToggle={toggleThinking} onEffort={pickEffort} className={toolClass} />
        <span className="flex-1" />
        {talking && current != null && (
          <Button variant="ghost" onClick={keep}>
            在 Pomelo 里继续对话
          </Button>
        )}
        {busyHere ? (
          <Button onClick={stop}>
            <Square size={12} className="fill-current" /> 停止
          </Button>
        ) : (
          <Button variant="primary" disabled={!props.text.trim() || busy || !status || !!status.error} onClick={() => void send()}>
            <Send size={14} /> 问
          </Button>
        )}
      </footer>
    </>
  );
}
