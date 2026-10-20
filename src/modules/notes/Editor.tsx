// Note text box (new note + editing in place): grows with the text; ⌘↩ saves, Esc cancels;
// ⌘B / ⌘U / ⌘⇧H wrap the selection in ** / ++ / ==; ↩ continues a list, Tab / ⇧Tab indent it;
// typing `#…` suggests existing tags. Never acts while an IME is composing.
// Looks like the card while typing: a plaintext-only editable box drawn one block per line, with
// NoteBody's spacing (paragraphs 0.75em apart, list items 4px, a blank line 0.9em) and its marks
// (`**` `==` `++` and list markers kept, dimmed). The text is the model: the browser types and
// deletes inside a line, everything else (Enter, paste, drop, shortcuts) edits the text here; after
// each change the lines are drawn again and the caret put back by its offset in the text. While an
// IME composes nothing is redrawn. The box keeps its own ⌘Z / ⌘⇧Z (the page's ⌘Z skips editables).
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";
import { lineStart, select, selectionIn, serialize } from "@/shared/editable";
import { atoms, pairMarks, type Mark } from "./NoteBody";

export type EditorHandle = { focus: () => void };

const LIST = /^( *)(?:([-*])|(\d+)([.)])) (.*)$/;
const MARK = "text-muted-foreground/45";

// ---------- drawing ----------

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const span = (cls: string, text: string) => (cls ? `<span class="${cls}">${esc(text)}</span>` : `<span>${esc(text)}</span>`);

/** One line's inside: every character of it, marks dimmed, styled like NoteBody's Inline. */
function lineHtml(line: string): string {
  const li = LIST.exec(line);
  const head = li ? line.slice(0, line.length - li[5].length) : "";
  const text = line.slice(head.length);
  const spans = atoms(text);
  const marks = pairMarks(text, spans);
  const on: Record<Mark, boolean> = { "**": false, "==": false, "++": false };
  let out = head ? span(MARK, head) : "";
  const styled = (s: string, cls = "") =>
    span([cls, on["**"] && "font-bold", on["=="] && "rounded-[3px] bg-[var(--status-doing-bg)] px-0.5 text-foreground", on["++"] && "underline decoration-1 underline-offset-[3px]"].filter(Boolean).join(" "), s);
  let buf = "";
  const flush = () => {
    if (buf) out += styled(buf);
    buf = "";
  };
  for (let i = 0; i < text.length; ) {
    const mark = marks.get(i);
    if (mark) {
      flush();
      out += span(MARK, mark);
      on[mark] = !on[mark];
      i += 2;
      continue;
    }
    const a = spans.find((s) => s.start === i);
    if (a) {
      flush();
      out += styled(text.slice(a.start, a.end), a.kind === "tag" ? "text-primary" : "text-primary underline underline-offset-2");
      i = a.end;
      continue;
    }
    buf += text[i];
    i++;
  }
  flush();
  return out;
}

/** NoteBody's gaps: a blank line is a 0.9em spacer; a line after text sits 0.75em below it, a list
 * item after a list item 4px. */
function lineClass(lines: string[], i: number): string {
  if (!lines[i].trim()) return lines.length > 1 ? "h-[0.9em] leading-[0.9em]" : "";
  if (i === 0 || !lines[i - 1].trim()) return "";
  return LIST.test(lines[i]) && LIST.test(lines[i - 1]) ? "mt-1" : "mt-[0.75em]";
}

function docHtml(text: string): string {
  const lines = text.split("\n");
  return lines.map((l, i) => `<div class="${lineClass(lines, i)}">${l ? lineHtml(l) : "<br>"}</div>`).join("");
}


/** The `#partial` tag being typed right before the caret, if any. */
function typingTag(value: string, caret: number): { start: number; text: string } | null {
  const m = /(?:^|\s)#([^\s#]*)$/u.exec(value.slice(0, caret));
  if (!m) return null;
  return { start: caret - m[1].length - 1, text: m[1] };
}

type Snap = { text: string; sel: [number, number] };
const HISTORY = 200;

export const Editor = forwardRef<
  EditorHandle,
  {
    value: string;
    onChange: (v: string) => void;
    onSubmit: () => void;
    onCancel?: () => void;
    tags: string[];
    placeholder?: string;
    autoFocus?: boolean;
    minRows?: number;
  }
>(function Editor(props, handle) {
  const ref = useRef<HTMLDivElement>(null);
  // what the box shows, its selection, and its own undo / redo
  const doc = useRef<Snap>({ text: props.value, sel: [props.value.length, props.value.length] });
  const past = useRef<Snap[]>([]);
  const future = useRef<Snap[]>([]);
  const typed = useRef<{ at: number; kind: "insert" | "delete" | null }>({ at: 0, kind: null });
  const composing = useRef(false);
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null); // caret where Esc closed the list

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
        if (past.current.length > HISTORY) past.current.shift();
      }
      future.current = [];
    }
    typed.current = { at: kind ? now : 0, kind };
    doc.current = { text, sel };
    draw(sel);
    setCaret(sel[0]);
    setActive(0);
    props.onChange(text);
  };

  /** Replace [start, end) with `text`; the caret after it unless given. */
  const replace = (start: number, end: number, text: string, sel?: [number, number]) => {
    const v = doc.current.text;
    const at = start + text.length;
    commit(v.slice(0, start) + text + v.slice(end), sel ?? [at, at]);
  };

  const travel = (back: boolean) => {
    const from = back ? past.current : future.current;
    const to = back ? future.current : past.current;
    const snap = from.pop();
    if (!snap) return;
    to.push(doc.current);
    doc.current = snap;
    typed.current = { at: 0, kind: null };
    draw(snap.sel);
    setCaret(snap.sel[0]);
    props.onChange(snap.text);
  };

  // the browser typed or deleted: read the text back
  const sync = () => {
    const el = ref.current;
    if (!el || composing.current) return;
    const text = serialize(el);
    const sel = selectionIn(el) ?? [text.length, text.length];
    commit(text, sel, true);
  };

  // first draw; a value set from outside (cleared after 记下, another note) starts afresh
  useLayoutEffect(() => {
    if (props.value === doc.current.text && ref.current?.childNodes.length) return;
    doc.current = { text: props.value, sel: [props.value.length, props.value.length] };
    past.current = [];
    future.current = [];
    draw(doc.current.sel);
  }, [props.value]); // eslint-disable-line react-hooks/exhaustive-deps

  useImperativeHandle(
    handle,
    () => ({
      focus: () => {
        const el = ref.current;
        if (!el) return;
        el.focus();
        if (!selectionIn(el)) select(el, doc.current.text.length);
      },
    }),
    [],
  );

  useEffect(() => {
    const el = ref.current;
    if (props.autoFocus && el) {
      el.focus();
      select(el, doc.current.text.length);
    }
  }, [props.autoFocus]);

  // the caret, for the tag list
  useEffect(() => {
    const onSel = () => {
      const el = ref.current;
      const s = el && !composing.current ? selectionIn(el) : null;
      if (s) {
        doc.current.sel = s;
        setCaret(s[0]);
      }
    };
    document.addEventListener("selectionchange", onSel);
    return () => document.removeEventListener("selectionchange", onSel);
  }, []);

  const value = doc.current.text;
  const typing = focused ? typingTag(value, caret) : null;
  const suggestions =
    typing && dismissed !== caret && !props.tags.includes(typing.text) // a complete tag: ↩ is a new line again
      ? props.tags
          .filter((t) => t !== typing.text && t.toLowerCase().includes(typing.text.toLowerCase()))
          .sort((a, b) => Number(!a.startsWith(typing.text)) - Number(!b.startsWith(typing.text)))
          .slice(0, 8)
      : [];
  const pick = (tag: string) => {
    if (!typing) return;
    const next = doc.current.text[caret] === " " ? "" : " ";
    replace(typing.start, caret, `#${tag}${next}`);
  };

  const wrap = (mark: string) => {
    const value = doc.current.text;
    const [s, e] = selectionIn(ref.current!) ?? doc.current.sel;
    const n = mark.length;
    if (value.slice(s - n, s) === mark && value.slice(e, e + n) === mark) {
      replace(s - n, e + n, value.slice(s, e), [s - n, e - n]); // already wrapped → unwrap
    } else {
      replace(s, e, mark + value.slice(s, e) + mark, [s + n, e + n]);
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229 || composing.current) return;
    const k = e.key.toLowerCase();
    if (e.metaKey && !e.altKey && !e.ctrlKey && k === "z") {
      e.preventDefault();
      e.stopPropagation();
      travel(!e.shiftKey);
      return;
    }
    if (suggestions.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length);
        return;
      }
      if ((e.key === "Enter" && !e.metaKey) || e.key === "Tab") {
        e.preventDefault();
        pick(suggestions[Math.min(active, suggestions.length - 1)]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(caret);
        return;
      }
    }
    if (e.metaKey && e.key === "Enter") {
      e.preventDefault();
      props.onSubmit();
      return;
    }
    if (e.key === "Escape" && props.onCancel) {
      e.preventDefault();
      e.stopPropagation();
      props.onCancel();
      return;
    }
    if (e.metaKey && !e.altKey && ((k === "b" && !e.shiftKey) || (k === "u" && !e.shiftKey) || (k === "h" && e.shiftKey))) {
      e.preventDefault();
      wrap(k === "b" ? "**" : k === "u" ? "++" : "==");
      return;
    }
    const value = doc.current.text;
    const [s, end] = selectionIn(e.currentTarget) ?? doc.current.sel;
    const ls = lineStart(value, s);
    const le = value.indexOf("\n", ls) < 0 ? value.length : value.indexOf("\n", ls);
    const li = LIST.exec(value.slice(ls, le));
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      if (li && s === end && !e.shiftKey) {
        if (!li[5].trim()) {
          if (li[1].length >= 2) replace(ls, ls + 2, "", [s - 2, s - 2]); // empty nested item: one level up
          else replace(ls, le, ""); // empty top item: end the list
        } else {
          const marker = li[2] ? `${li[2]} ` : `${Number(li[3]) + 1}${li[4]} `;
          replace(s, s, `\n${li[1]}${marker}`);
        }
      } else {
        replace(s, end, "\n");
      }
      return;
    }
    if (e.key === "Tab" && li) {
      e.preventDefault();
      if (e.shiftKey) {
        if (li[1].length >= 2) replace(ls, ls + 2, "", [s - 2, end - 2]);
      } else replace(ls, ls, "  ", [s + 2, end + 2]);
    }
  };

  // what the browser would do in its own way: done here, on the text
  const onBeforeInput = (e: InputEvent) => {
    const el = ref.current!;
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
        e.preventDefault(); // a dragged piece is copied, not moved
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
  useEffect(() => {
    const el = ref.current!;
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
    <div className="relative">
      <div
        ref={ref}
        role="textbox"
        aria-multiline
        aria-label={props.placeholder ?? "笔记"}
        contentEditable="plaintext-only"
        suppressContentEditableWarning
        spellCheck={false}
        autoCorrect="off"
        autoCapitalize="off"
        onInput={sync}
        onCompositionStart={() => (composing.current = true)}
        onCompositionEnd={() => {
          composing.current = false;
          requestAnimationFrame(sync);
        }}
        onKeyDown={onKeyDown}
        onCopy={(e) => copy(e, false)}
        onCut={(e) => copy(e, true)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        style={{ minHeight: `${(props.minRows ?? 3) * 1.625}em` }}
        className="block max-h-[60vh] w-full overflow-y-auto leading-relaxed break-words whitespace-pre-wrap outline-none"
      />
      {!value && props.placeholder && <div className="pointer-events-none absolute top-0 left-0 leading-relaxed text-muted-foreground">{props.placeholder}</div>}
      {suggestions.length > 0 && (
        <div
          role="listbox"
          aria-label="标签"
          onMouseDown={(e) => e.preventDefault()}
          className="absolute top-full left-0 z-30 mt-1 max-w-full min-w-48 animate-pop-in rounded-card border bg-popover p-1 shadow-lg"
        >
          {suggestions.map((t, i) => (
            <div
              key={t}
              role="option"
              aria-selected={i === active}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(t)}
              className={`cursor-default truncate rounded-md px-3 py-1 text-sm ${i === active ? "bg-muted" : ""}`}
            >
              #{t}
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
