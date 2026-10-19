// 小拾 helps in the edit panels (v0.3.5 阶段 4; rules in src-tauri/src/picky/assist.rs): results only go into
// the panel (the user saves as usual, ⌘Z works) or into a list to pick from (「加上」 adds in one undoable step).
// - <Polish>: a small button in the field's corner (on hover / focus) opens a box: this time's request,
//   then the rewritten text with the changes marked, 「替换」 puts it into the field.
// - <IdeaList>: suggestions with checkboxes (all on), 「加上」 / 取消.
// - <QuickFill>: the top line of a new-record panel (一句话填好): a sentence → the panel's fields, checked
//   by Rust (dates from today, names matched); filled fields show 浅柚红 (useFilled) until edited.
// - assistBusyText: what the wait says (a stopped local model is started first).
// Light on purpose: module pages import this (no Markdown / formula libraries).
import { useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Sparkles, X } from "lucide-react";
import { useHiddenModules } from "@/app/hidden";
import { diff } from "@/shared/diff";
import { errorText } from "@/shared/feedback";
import { Button, IconButton, TextInput } from "@/shared/ui";
import { usePickyStatus, type PickyStatus } from "./api";

export type WorkFill = { creator: string | null; country: string | null; kindId: number | null };
export type JobFill = { company: string | null; position: string | null; location: string | null };
export type ItemIdea = { name: string; kindId: number | null };
/** What a sentence filled (only its module's fields; ids already checked). Amount in cents. */
export type QuickFillResult = {
  flow: "out" | "in" | null;
  amount: number | null;
  currency: "CNY" | "SGD" | "HKD" | "USD" | null;
  day: string | null;
  kindId: number | null;
  note: string | null;
  title: string | null;
  startDate: string | null;
  dueDate: string | null;
  groupId: number | null;
  goalId: number | null;
  company: string | null;
  position: string | null;
  location: string | null;
  statusId: number | null;
  creator: string | null;
  country: string | null;
  status: "unmarked" | "todo" | "doing" | "done" | null;
  rating: number | null;
  finishedYear: string | null;
  name: string | null;
  departOn: string | null;
  templateId: number | null;
};
export type QuickModule = "finance" | "todo" | "jobs" | "books" | "films" | "packing";

export const assistApi = {
  /** `module`: the module the text belongs to (a cloud model may not get some). */
  polish: (module: string, text: string, request: string) => invoke<string>("picky_polish", { module, text, request }),
  fillWork: (module: "books" | "films", title: string) => invoke<WorkFill>("picky_fill_work", { module, title }),
  fillJob: (jd: string) => invoke<JobFill>("picky_fill_job", { jd }),
  splitTask: (taskId: number) => invoke<string[]>("picky_split_task", { taskId }),
  splitNote: (noteId: number) => invoke<string[]>("picky_split_note", { noteId }),
  tripMissing: (tripId: number) => invoke<ItemIdea[]>("picky_trip_missing", { tripId }),
  quickFill: (module: QuickModule, sentence: string) => invoke<QuickFillResult>("picky_quick_fill", { module, sentence }),
};

/** A help started from a field's label row (small, 淡墨, a star). */
export function AssistAction(props: { onClick: () => void; disabled?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      disabled={props.disabled}
      onClick={props.onClick}
      className="-my-0.5 inline-flex shrink-0 items-center gap-1 rounded-control px-1.5 py-0.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60"
    >
      <Sparkles size={12} strokeWidth={1.75} className={props.disabled ? "animate-pulse" : ""} /> {props.children}
    </button>
  );
}

/** These helps are 小拾's: gone while 小拾 is hidden. */
export function useAssistOn() {
  const [hidden] = useHiddenModules();
  return !hidden.includes("picky");
}

/** Fields a help filled in (浅柚红 until edited by hand). */
export function useFilled<K extends string>() {
  const [filled, setFilled] = useState<Set<K>>(new Set());
  return {
    mark: (keys: K[]) => setFilled((s) => new Set([...s, ...keys])),
    clear: (k: K) => setFilled((s) => (s.has(k) ? new Set([...s].filter((x) => x !== k)) : s)),
    /** Text inputs: the field itself. */
    cls: (k: K) => (filled.has(k) ? "bg-accent" : ""),
    /** Other controls (pickers, dates, segments): a 浅柚红 halo around them. */
    halo: (k: K) => (filled.has(k) ? "w-fit rounded-control shadow-[0_0_0_3px_var(--accent)]" : "w-fit"),
  };
}

/** 一句话填好: the top line of a new-record panel. Enter asks; `onFill` puts the values into the panel. */
export function QuickFill(props: { module: QuickModule; onFill: (f: QuickFillResult) => void; autoFocus?: boolean }) {
  const status = usePickyStatus().data;
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!useAssistOn()) return null;
  const run = async () => {
    if (busy || !text.trim()) return;
    setBusy(true);
    setError(null);
    try {
      props.onFill(await assistApi.quickFill(props.module, text));
      setText("");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-1">
      <div className="relative">
        <Sparkles size={14} strokeWidth={1.75} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-[var(--module-picky)]" />
        <TextInput
          aria-label="一句话填好"
          placeholder="一句话填好"
          autoFocus={props.autoFocus}
          value={text}
          readOnly={busy}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            // never the panel's form: Enter here fills
            if (e.key === "Enter" && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void run();
            }
          }}
          maxLength={300}
          className="pl-8"
        />
      </div>
      {busy && <p className="animate-pulse text-xs text-muted-foreground">{assistBusyText(status, "正在填…")}</p>}
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}

/** What a help says while it waits. */
export const assistBusyText = (s: PickyStatus | undefined, doing: string) =>
  s && s.selection === "local" && !s.local.active ? "正在启动本地模型…" : doing;

/** Wraps a note field: the corner button and, when open, the 润色 box under it. */
export function Polish(props: { module: string; value: string; onChange: (text: string) => void; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  if (!useAssistOn()) return <>{props.children}</>;
  return (
    <div className="group/polish relative">
      {props.children}
      {!open && props.value.trim() && (
        <button
          type="button"
          aria-label="润色"
          title="润色"
          onClick={() => setOpen(true)}
          className="absolute top-1 right-1 rounded-control bg-card/90 p-1 text-muted-foreground opacity-0 transition-opacity duration-150 outline-none group-focus-within/polish:opacity-100 group-hover/polish:opacity-100 hover:text-[var(--module-picky)] focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <Sparkles size={14} strokeWidth={1.75} />
        </button>
      )}
      {open && (
        <PolishBox
          module={props.module}
          value={props.value}
          onReplace={(t) => {
            props.onChange(t);
            setOpen(false);
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

type PolishState = { kind: "idle" } | { kind: "working" } | { kind: "done"; from: string; text: string } | { kind: "error"; message: string };

function PolishBox(props: { module: string; value: string; onReplace: (text: string) => void; onClose: () => void }) {
  const status = usePickyStatus().data;
  const [request, setRequest] = useState("");
  const [state, setState] = useState<PolishState>({ kind: "idle" });
  const run = async () => {
    if (state.kind === "working") return;
    setState({ kind: "working" });
    try {
      const text = await assistApi.polish(props.module, props.value, request);
      setState({ kind: "done", from: props.value, text });
    } catch (e) {
      setState({ kind: "error", message: errorText(e) });
    }
  };
  return (
    <div className="mt-1.5 space-y-2 rounded-card border bg-card p-2.5 text-sm shadow-sm">
      <div className="flex items-center gap-2">
        <TextInput
          aria-label="这次的要求"
          placeholder="要求"
          value={request}
          autoFocus
          onChange={(e) => setRequest(e.target.value)}
          onKeyDown={(e) => {
            // never the panel's form: Enter here means 润色
            if (e.key === "Enter" && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void run();
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              props.onClose();
            }
          }}
          maxLength={200}
          className="min-w-0 flex-1"
        />
        <Button variant="primary" disabled={state.kind === "working"} onClick={() => void run()}>
          润色
        </Button>
        <IconButton label="关闭润色" onClick={props.onClose}>
          <X size={14} />
        </IconButton>
      </div>
      {state.kind === "working" && <p className="animate-pulse text-xs text-muted-foreground">{assistBusyText(status, "正在润色…")}</p>}
      {state.kind === "error" && <p className="text-xs text-destructive">{state.message}</p>}
      {state.kind === "done" && (
        <>
          <DiffView from={state.from} to={state.text} />
          <div className="flex gap-2">
            <Button variant="primary" onClick={() => props.onReplace(state.text)}>
              替换
            </Button>
            <Button variant="ghost" onClick={() => void run()}>
              再改一次
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

/** The new text with what changed marked: removed struck through, added on 浅柚红. */
export function DiffView(props: { from: string; to: string }) {
  return (
    <p className="selectable max-h-60 overflow-y-auto rounded-control bg-background px-2.5 py-2 leading-relaxed whitespace-pre-wrap">
      {diff(props.from, props.to).map((p, i) =>
        p.kind === "same" ? (
          <span key={i}>{p.text}</span>
        ) : p.kind === "del" ? (
          <del key={i} className="text-muted-foreground decoration-destructive/60">
            {p.text}
          </del>
        ) : (
          <ins key={i} className="rounded-[3px] bg-accent text-accent-foreground no-underline">
            {p.text}
          </ins>
        ),
      )}
    </p>
  );
}

/** Suggestions to pick from (all on); 「加上」 gets the picked ones. */
export function IdeaList<T>(props: { items: T[]; label: (item: T) => ReactNode; addLabel?: string; busy?: boolean; onAdd: (picked: T[]) => void; onCancel: () => void }) {
  const [off, setOff] = useState<Set<number>>(new Set());
  const picked = props.items.filter((_, i) => !off.has(i));
  return (
    <div className="space-y-2 rounded-card border bg-card p-2.5 text-sm shadow-sm">
      <ul className="space-y-1">
        {props.items.map((it, i) => (
          <li key={i}>
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                checked={!off.has(i)}
                onChange={() => setOff((s) => (s.has(i) ? new Set([...s].filter((x) => x !== i)) : new Set(s).add(i)))}
                className="mt-1 size-3.5 accent-[var(--primary)]"
              />
              <span>{props.label(it)}</span>
            </label>
          </li>
        ))}
      </ul>
      <div className="flex gap-2">
        <Button variant="primary" disabled={!picked.length || props.busy} onClick={() => props.onAdd(picked)}>
          {props.addLabel ?? "加上"}
          {picked.length > 0 && picked.length < props.items.length ? ` ${picked.length} 个` : ""}
        </Button>
        <Button variant="ghost" onClick={props.onCancel}>
          取消
        </Button>
      </div>
    </div>
  );
}
