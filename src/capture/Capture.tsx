// 速记小窗 (requirements「界面外框」): 笔记 · 待办 · 问柚子 in a small window of its own (⌥Space). The draft
// is written as you type (a moment later) and before the window hides: Esc, ⌘W and the close button give the
// focus back to the app that was in front; a click elsewhere hides it too unless the pin is on.
// The note's text scrolls inside its box, its attachments stay in a row along the box's bottom edge.
// 问柚子 chats right here (CaptureChat, loaded when first opened).
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Paperclip, Pin, Send } from "lucide-react";
import { LiteEditor, type LiveTextHandle } from "@/shared/editor/LiteEditor";
import { AttachmentStrip, pickAttachments, withFiles } from "@/shared/attachments/Attachments";
import { useAttachTarget } from "@/shared/attachments/target";
import type { FileInfo } from "@/shared/attachments/api";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { errorText } from "@/shared/feedback";
import { Button, Segmented, TextInput } from "@/shared/ui";
import { EMPTY, captureApi, type CaptureTab, type Draft, type Snapshot } from "./api";

const CaptureChat = lazy(() => import("./CaptureChat"));

const TABS: { value: CaptureTab; label: string; module: string }[] = [
  { value: "note", label: "笔记", module: "notes" },
  { value: "todo", label: "待办", module: "todo" },
  { value: "ask", label: "问柚子", module: "picky" },
];
/** Typing settles this long before the draft is written. */
const WRITE_MS = 400;
/** 「已记下 · 撤销」 stays this long. */
const SAVED_MS = 5000;

export function Capture() {
  const [meta, setMeta] = useState<Omit<Snapshot, "draft" | "files" | "pinned"> | null>(null);
  const [d, setD] = useState<Draft>(EMPTY);
  const [files, setFiles] = useState<FileInfo[]>([]);
  const [pinned, setPinned] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<{ text: string; undo?: boolean; error?: boolean } | null>(null);
  const latest = useRef(d);
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;
  const writes = useRef<Promise<void>>(Promise.resolve());
  const timer = useRef<number | undefined>(undefined);
  const statusTimer = useRef<number | undefined>(undefined);
  /** The file picker is open: losing the focus to it doesn't hide the window. */
  const picking = useRef(false);
  const hiding = useRef(false);
  const editor = useRef<LiveTextHandle>(null);
  const title = useRef<HTMLInputElement>(null);
  const question = useRef<HTMLTextAreaElement>(null);
  const box = useRef<HTMLDivElement>(null);
  /** The header's place for the chat's model menu. */
  const [slot, setSlot] = useState<HTMLDivElement | null>(null);

  const tabs = TABS.filter((t) => !meta?.hidden.includes(t.module));
  const tab: CaptureTab | null = tabs.find((t) => t.value === d.tab)?.value ?? tabs[0]?.value ?? null;

  const say = (text: string, more: { undo?: boolean; error?: boolean } = {}) => {
    window.clearTimeout(statusTimer.current);
    setStatus({ text, ...more });
    if (!more.error) statusTimer.current = window.setTimeout(() => setStatus(null), SAVED_MS);
  };
  const fail = (e: unknown) => say(errorText(e), { error: true });

  /** Writes the draft now (after any write already on its way). */
  const flush = useCallback(() => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    const next = latest.current;
    writes.current = writes.current.catch(() => {}).then(() => captureApi.draft(next));
    return writes.current;
  }, []);

  const change = (p: Partial<Draft>) => {
    const next = { ...latest.current, ...p };
    latest.current = next;
    setD(next);
    if (status && !status.error) setStatus(null);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush().catch(fail), WRITE_MS);
  };

  const focus = useCallback((t: CaptureTab | null) => {
    requestAnimationFrame(() => {
      if (t === "note") editor.current?.focus();
      else if (t === "todo") title.current?.focus();
      else if (t === "ask") question.current?.focus();
    });
  }, []);

  /** Draft first, then away; `refocus`: back to the app that was in front. */
  const hide = useCallback(
    async (refocus: boolean) => {
      if (hiding.current) return;
      hiding.current = true;
      try {
        await flush();
        await captureApi.hide(refocus);
      } catch (e) {
        fail(e);
      } finally {
        hiding.current = false;
      }
    },
    [flush], // eslint-disable-line react-hooks/exhaustive-deps
  );

  // the first look brings the draft; each time the window shows again, the groups, tags and today
  const load = useCallback(async (first: boolean) => {
    try {
      const s = await captureApi.load();
      const { draft, files: fs, pinned: pin, ...rest } = s;
      setMeta(rest);
      setPinned(pin);
      if (first) {
        latest.current = draft;
        setD(draft);
        setFiles(fs);
      }
      const shown = TABS.filter((t) => !rest.hidden.includes(t.module));
      focus(shown.find((t) => t.value === (first ? draft.tab : latest.current.tab))?.value ?? shown[0]?.value ?? null);
    } catch (e) {
      fail(e);
    }
  }, [focus]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    void load(true);
    const off = [
      listen("capture-shown", () => void load(false)),
      listen("capture-close", () => void hide(true)),
      listen("capture-blur", () => {
        if (!pinnedRef.current && !picking.current) void hide(false);
      }),
    ];
    return () => off.forEach((p) => void p.then((f) => f()));
  }, [load, hide]);

  const setDraftFiles = (next: FileInfo[]) =>
    captureApi
      .files(next.map((f) => f.id))
      .then(setFiles)
      .catch(fail);
  const dropping = useAttachTarget(box, (fs) => void setDraftFiles(withFiles(files, fs)), fail, tab === "note" && !busy);
  const pick = async () => {
    picking.current = true;
    try {
      const fs = await pickAttachments();
      if (fs.length) await setDraftFiles(withFiles(files, fs));
    } catch (e) {
      fail(e);
    } finally {
      picking.current = false;
      focus("note");
    }
  };

  const groups = meta?.groups ?? [];
  const group = groups.find((g) => g.id === d.group)?.id ?? groups[0]?.id ?? null;
  const ready = !!meta && !busy;
  const can = ready && (tab === "note" ? !!d.note.trim() : tab === "todo" ? !!d.todo.trim() && group !== null : false);

  const submit = async () => {
    if (!can || (tab !== "note" && tab !== "todo")) return;
    setBusy(true);
    try {
      await flush();
      if (tab === "note") {
        await captureApi.note(latest.current.note);
        latest.current = { ...latest.current, note: "" };
        setFiles([]);
        say("已记下", { undo: true });
      } else {
        await captureApi.todo(latest.current.todo, latest.current.due, group!);
        latest.current = { ...latest.current, todo: "", due: null, group };
        say("已记下", { undo: true });
      }
      setD(latest.current);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      focus(tab);
    }
  };

  const undo = async () => {
    try {
      const r = await captureApi.undo();
      say(r.label ? `已撤销：${r.label}` : "没有可以撤销的了");
    } catch (e) {
      fail(e);
    }
  };

  const togglePin = () => {
    const on = !pinned;
    setPinned(on);
    captureApi.pin(on).catch(fail);
  };

  return (
    <main
      className="flex h-screen flex-col gap-3 bg-background px-4 pt-3 pb-3 font-sans text-foreground"
      onKeyDown={(e) => {
        if (e.key !== "Escape" || e.defaultPrevented || e.nativeEvent.isComposing || e.keyCode === 229) return;
        e.preventDefault();
        void hide(true);
      }}
    >
      <header className="flex shrink-0 items-center gap-2">
        {tab && tabs.length > 1 && (
          <Segmented
            value={tab}
            options={tabs.map((t) => ({ value: t.value, label: t.label }))}
            onChange={(v) => {
              change({ tab: v });
              focus(v);
            }}
            ariaLabel="记什么"
          />
        )}
        <span className="flex-1" />
        <div ref={setSlot} className="flex min-w-0 items-center" />
        <button
          type="button"
          aria-label="固定"
          title="固定"
          aria-pressed={pinned}
          onClick={togglePin}
          className={`inline-flex shrink-0 items-center justify-center rounded-control p-1.5 outline-none transition-colors duration-150 hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${
            pinned ? "text-primary" : "text-muted-foreground hover:text-foreground"
          }`}
        >
          <Pin size={15} strokeWidth={1.75} className={pinned ? "fill-current" : ""} />
        </button>
      </header>

      {tab === "ask" && meta ? (
        <Suspense fallback={<div className="flex-1" />}>
          <CaptureChat text={d.ask} onText={(ask) => change({ ask })} inputRef={question} slot={slot} />
        </Suspense>
      ) : (
        <>
          {tab === "note" && meta && (
            <div ref={box} className={`flex min-h-0 flex-1 flex-col rounded-card border bg-card focus-within:ring-2 focus-within:ring-ring ${dropping ? "ring-2 ring-primary" : ""}`}>
              <div
                className="min-h-0 flex-1 cursor-text overflow-y-auto px-3 py-2"
                onMouseDown={(e) => {
                  // the empty space below the text puts the caret in it
                  if (e.target !== e.currentTarget) return;
                  e.preventDefault();
                  editor.current?.focus();
                }}
              >
                <LiteEditor ref={editor} value={d.note} onChange={(note) => change({ note })} onSubmit={() => void submit()} tags={meta.tags} placeholder="现在的想法是……" minRows={2} grow autoFocus />
              </div>
              <AttachmentStrip row files={files} onRemove={(id) => void setDraftFiles(files.filter((f) => f.id !== id))} className="shrink-0 border-t px-3 pb-2" />
            </div>
          )}
          {tab === "todo" && meta && (
            <div className="flex min-h-0 flex-1 flex-col gap-2">
              <TextInput
                ref={title}
                value={d.todo}
                onChange={(e) => change({ todo: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing && e.keyCode !== 229) {
                    e.preventDefault();
                    void submit();
                  }
                }}
                placeholder="要做的事"
                aria-label="要做的事"
                maxLength={200}
                autoFocus
              />
              <div className="flex items-center gap-2">
                <DateInput value={d.due ?? meta.today} onChange={(v) => change({ due: v && v !== meta.today ? v : null })} ariaLabel="截止日期" />
                {groups.length > 0 && <Dropdown value={group} options={groups.map((g) => ({ value: g.id as number | null, label: g.name }))} onChange={(g) => change({ group: g })} ariaLabel="分组" />}
              </div>
            </div>
          )}
          {!meta && <div className="flex-1" />}
          <footer className="flex shrink-0 items-center gap-2">
            {tab === "note" && (
              <button
                type="button"
                aria-label="添加附件"
                title="添加附件"
                disabled={!ready}
                onClick={() => void pick()}
                className="inline-flex shrink-0 items-center justify-center rounded-control p-1.5 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40"
              >
                <Paperclip size={15} strokeWidth={1.75} />
              </button>
            )}
            <span role="status" className={`min-w-0 truncate text-xs ${status?.error ? "text-destructive" : "text-muted-foreground"}`}>
              {status?.text}
            </span>
            {status?.undo && (
              <button type="button" onClick={() => void undo()} className="shrink-0 rounded-control px-1 text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring">
                撤销
              </button>
            )}
            <span className="flex-1" />
            {tab && (
              <Button variant="primary" disabled={!can} onClick={() => void submit()}>
                <Send size={14} /> 记下
              </Button>
            )}
          </footer>
        </>
      )}
    </main>
  );
}
