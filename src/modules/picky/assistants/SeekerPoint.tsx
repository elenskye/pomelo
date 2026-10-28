// A point's text box, drawn while typing the way the Word export reads it (word.rs `styled`): each
// line on its own, split at 「**」, every other piece bold; the marks stay, dimmed. 念念不忘's way
// (shared/editable.ts): the text is the model, the browser types inside a line, everything else
// (↩, paste, drop, ⌘B) edits the text here; after each change the lines are drawn again and the caret
// put back. Nothing is redrawn while an IME composes. The box keeps its own ⌘Z / ⌘⇧Z.
import { useEffect, useLayoutEffect, useRef, type ClipboardEvent, type KeyboardEvent } from "react";
import { select, selectionIn, serialize } from "@/shared/editable";

const MARK = "text-muted-foreground/45";
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function lineHtml(line: string): string {
  if (!line) return "<br>";
  return line
    .split("**")
    .map((piece, i) => (i > 0 ? `<span class="${MARK}">**</span>` : "") + (piece ? `<span${i % 2 ? ' class="font-bold"' : ""}>${esc(piece)}</span>` : ""))
    .join("");
}
const docHtml = (text: string) => text.split("\n").map((l) => `<div>${lineHtml(l)}</div>`).join("");

type Snap = { text: string; sel: [number, number] };

export function PointEditor(props: { value: string; disabled: boolean; label: string; onChange: (text: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const doc = useRef<Snap>({ text: props.value, sel: [props.value.length, props.value.length] });
  const past = useRef<Snap[]>([]);
  const future = useRef<Snap[]>([]);
  const typed = useRef<{ at: number; kind: "insert" | "delete" | null }>({ at: 0, kind: null });
  const composing = useRef(false);

  const draw = (sel?: [number, number]) => {
    const el = ref.current;
    if (!el) return;
    el.innerHTML = docHtml(doc.current.text);
    if (sel && document.activeElement === el) select(el, sel[0], sel[1]);
  };

  /** A new text: remembered for ⌘Z (typing, or deleting, in a row is one step), drawn, reported. */
  const commit = (text: string, sel: [number, number], typing = false) => {
    const now = Date.now();
    const kind = !typing ? null : text.length >= doc.current.text.length ? "insert" : "delete";
    if (text !== doc.current.text) {
      if (!(kind && kind === typed.current.kind && now - typed.current.at < 1000)) {
        past.current.push(doc.current);
        if (past.current.length > 100) past.current.shift();
      }
      future.current = [];
    }
    typed.current = { at: kind ? now : 0, kind };
    doc.current = { text, sel };
    draw(sel);
    if (text !== props.value) props.onChange(text);
  };

  const replace = (start: number, end: number, text: string, sel?: [number, number]) => {
    const v = doc.current.text;
    const at = start + text.length;
    commit(v.slice(0, start) + text + v.slice(end), sel ?? [at, at]);
  };

  const travel = (back: boolean) => {
    const snap = (back ? past : future).current.pop();
    if (!snap) return;
    (back ? future : past).current.push(doc.current);
    doc.current = snap;
    typed.current = { at: 0, kind: null };
    draw(snap.sel);
    props.onChange(snap.text);
  };

  const sync = () => {
    const el = ref.current;
    if (!el || composing.current) return;
    const text = serialize(el);
    commit(text, selectionIn(el) ?? [text.length, text.length], true);
  };

  // a value from outside (undo, a model's edit, another point): start afresh
  useLayoutEffect(() => {
    if (props.value === doc.current.text && ref.current?.childNodes.length) return;
    doc.current = { text: props.value, sel: [props.value.length, props.value.length] };
    past.current = [];
    future.current = [];
    draw();
  }, [props.value]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onSel = () => {
      const el = ref.current;
      const s = el && !composing.current ? selectionIn(el) : null;
      if (s) doc.current.sel = s;
    };
    document.addEventListener("selectionchange", onSel);
    return () => document.removeEventListener("selectionchange", onSel);
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229 || composing.current) return;
    const k = e.key.toLowerCase();
    if (e.metaKey && !e.altKey && !e.ctrlKey && k === "z") {
      e.preventDefault();
      e.stopPropagation();
      travel(!e.shiftKey);
      return;
    }
    if (e.metaKey && !e.altKey && !e.shiftKey && k === "b") {
      e.preventDefault();
      const v = doc.current.text;
      const [s, end] = selectionIn(e.currentTarget) ?? doc.current.sel;
      if (v.slice(s - 2, s) === "**" && v.slice(end, end + 2) === "**") replace(s - 2, end + 2, v.slice(s, end), [s - 2, end - 2]);
      else replace(s, end, `**${v.slice(s, end)}**`, [s + 2, end + 2]);
      return;
    }
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      const [s, end] = selectionIn(e.currentTarget) ?? doc.current.sel;
      replace(s, end, "\n");
    }
  };

  // what the browser would do in its own way: done here, on the text
  useEffect(() => {
    const el = ref.current!;
    const onBeforeInput = (e: InputEvent) => {
      const [s, end] = selectionIn(el) ?? doc.current.sel;
      switch (e.inputType) {
        case "insertParagraph":
        case "insertLineBreak":
          e.preventDefault();
          replace(s, end, "\n");
          break;
        case "insertFromPaste":
        case "insertFromDrop":
        case "insertFromYank":
          e.preventDefault();
          replace(s, end, (e.dataTransfer?.getData("text/plain") ?? e.data ?? "").replace(/\r\n?/g, "\n"));
          break;
        case "deleteByDrag":
          e.preventDefault();
          break;
        case "historyUndo":
        case "historyRedo":
          e.preventDefault();
          travel(e.inputType === "historyUndo");
          break;
        case "formatBold":
        case "formatItalic":
        case "formatUnderline":
          e.preventDefault();
          break;
      }
    };
    el.addEventListener("beforeinput", onBeforeInput);
    return () => el.removeEventListener("beforeinput", onBeforeInput);
  });

  const copy = (e: ClipboardEvent<HTMLDivElement>, cut: boolean) => {
    const [s, end] = selectionIn(e.currentTarget) ?? [0, 0];
    if (s === end) return;
    e.preventDefault();
    e.clipboardData.setData("text/plain", doc.current.text.slice(s, end));
    if (cut) replace(s, end, "");
  };

  return (
    <div
      ref={ref}
      role="textbox"
      aria-multiline
      aria-label={props.label}
      aria-disabled={props.disabled || undefined}
      contentEditable={props.disabled ? false : "plaintext-only"}
      suppressContentEditableWarning
      spellCheck={false}
      onInput={sync}
      onCompositionStart={() => (composing.current = true)}
      onCompositionEnd={() => {
        composing.current = false;
        requestAnimationFrame(sync);
      }}
      onKeyDown={onKeyDown}
      onCopy={(e) => copy(e, false)}
      onCut={(e) => copy(e, true)}
      className="-mx-1 block min-h-[1.5em] cursor-text rounded-sm px-1 leading-relaxed break-words whitespace-pre-wrap outline-none select-text focus:bg-background focus:ring-1 focus:ring-ring/50"
    />
  );
}
