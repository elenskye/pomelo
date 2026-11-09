// The shared live editor (念念不忘, 速记小窗, 呦呦漫游; new modules use LiteEditor or their own Flavor): a
// plain-text editable box drawn one block (div) per line by a Flavor, the text being the model. The
// browser types and deletes inside a line; everything else (↩, paste, drop, shortcuts) edits the text
// here; after each change the lines are drawn again and the caret put back by its offset (editable.ts).
// While an IME composes nothing is redrawn. Own ⌘Z / ⌘⇧Z (the page's ⌘Z skips editables); typing, or
// deleting, in a row is one step.
// - The blank line the caret is on right after an edit is drawn as the line about to be written
//   (`pending`: its full height and the gap it will have), so ↩ shows the new paragraph at once instead
//   of when its first character arrives. It is drawn as a blank line again once the caret leaves it
//   (not while the mouse is down: redrawing would break a drag-selection).
// - Suggest (e.g. #tags): a list at the caret while `at` finds something being typed; ↑↓, ↩ / Tab, Esc.
//   It is fixed-positioned (under the typed trigger, or above it when there's no room below) in the
//   page or the dialog around the box, so no scrolling box clips it.
// - A right-click opens MdMenu at the pointer: the flavor's formats (applied to the selection, or its lines) and
//   the two icon libraries (an icon goes in at the caret). `:pm-<key>:` is drawn as its icon (inlineIcon.ts).
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ClipboardEvent,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { lineStart, pointAt, select, selectionIn, serialize } from "./editable";
import { MdMenu } from "./MdMenu";

/** What ↩ continues on a line: a list item or a quote; `next` starts the new line (indent + marker). */
export type LineKind = { kind: "list" | "quote"; indent: number; empty: boolean; next: string };

/** One row of the right-click list (`"-"`: a line between groups): `wrap` puts a mark around the selection (again =
 *  off), `line` starts each selected line with it (again = off; `1. ` counts up), `around` puts the selection between
 *  two pieces (`block`: on lines of their own), `insert` adds a line of its own. `keys`: its shortcut, shown. */
export type Format = "-" | ({ label: string; keys?: string } & ({ wrap: string } | { line: string } | { around: [string, string]; block?: boolean } | { insert: string }));

export type Flavor = {
  /** The box's HTML, one div per line (every character drawn once). `pending`: the blank line to draw
   * as a line about to be written. */
  draw: (text: string, pending: number | null) => string;
  /** ↩ continues lists / quotes (an empty one ends, a nested empty item goes up a level); Tab / ⇧Tab
   * indent list items by 2 spaces. */
  line?: (line: string) => LineKind | null;
  /** ⌘-shortcuts wrapping the selection in a mark (again = unwrap): key (lowercase), with ⇧ or not. */
  wraps?: { key: string; shift?: boolean; mark: string }[];
  /** The right-click list's formats. */
  formats?: Format[];
  /** What starts a line as a heading, list item or quote: a `line` format takes its place. */
  linePrefix?: RegExp;
};

export type Suggest = {
  label: string;
  /** What is being typed right before the caret: where it starts (the trigger) and the query. */
  at: (text: string, caret: number) => { start: number; query: string } | null;
  items: (query: string) => string[];
  show?: (item: string) => ReactNode;
  /** The text replacing [start, caret); `after` is the text after the caret. */
  insert: (item: string, after: string) => string;
};

export type LiveTextHandle = {
  focus: () => void;
  /** Lines in as their own paragraph at the caret (else at the end). */
  insertLines: (lines: string[]) => void;
  /** The last selection in the box (offsets in the text). */
  selection: () => [number, number];
  replaceRange: (start: number, end: number, text: string) => void;
};

type Snap = { text: string; sel: [number, number] };
const HISTORY = 300;

/** The line index of `pos` when that line is blank. */
function blankAt(text: string, pos: number): number | null {
  const ls = lineStart(text, pos);
  const le = text.indexOf("\n", pos);
  if (text.slice(ls, le < 0 ? text.length : le).trim()) return null;
  return text.slice(0, ls).split("\n").length - 1;
}

export const LiveText = forwardRef<
  LiveTextHandle,
  {
    value: string;
    onChange: (v: string) => void;
    flavor: Flavor;
    label: string;
    onSubmit?: () => void;
    onCancel?: () => void;
    suggest?: Suggest;
    placeholder?: string;
    autoFocus?: boolean;
    readOnly?: boolean;
    /** Where a caret goes when the box is focused without one: "end" (default) or "start". */
    focusAt?: "start" | "end";
    className?: string;
    style?: CSSProperties;
    /** The editable element, for callers that need it (a drop target). */
    boxRef?: RefObject<HTMLDivElement | null>;
  }
>(function LiveText(props, handle) {
  const own = useRef<HTMLDivElement>(null);
  const ref = props.boxRef ?? own;
  const start = (v: string): [number, number] => (props.focusAt === "start" ? [0, 0] : [v.length, v.length]);
  const doc = useRef<Snap>({ text: props.value, sel: start(props.value) });
  const past = useRef<Snap[]>([]);
  const future = useRef<Snap[]>([]);
  const typed = useRef<{ at: number; kind: "insert" | "delete" | null }>({ at: 0, kind: null });
  const composing = useRef(false);
  const pending = useRef<number | null>(null);
  const pressing = useRef(false);
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null); // caret where Esc closed the list
  const [menu, setMenu] = useState<{ x: number; y: number; sel: [number, number] } | null>(null);

  /** Draw the text; `mark`: a blank line holding the (collapsed) caret is drawn as pending. */
  const draw = (sel?: [number, number], mark = true) => {
    const el = ref.current;
    if (!el) return;
    const text = doc.current.text;
    pending.current = mark && sel && sel[0] === sel[1] ? blankAt(text, sel[0]) : null;
    el.innerHTML = props.flavor.draw(text, pending.current);
    if (sel && document.activeElement === el) select(el, sel[0], sel[1]);
  };

  /** A new text: remembered for ⌘Z, drawn, reported. */
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
  const replace = (from: number, to: number, text: string, sel?: [number, number]) => {
    const v = doc.current.text;
    const at = from + text.length;
    commit(v.slice(0, from) + text + v.slice(to), sel ?? [at, at]);
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
    commit(text, selectionIn(el) ?? [text.length, text.length], true);
  };

  // first draw; a value set from outside (cleared after 记下, another note / work) starts afresh
  useLayoutEffect(() => {
    if (props.value === doc.current.text && ref.current?.childNodes.length) return;
    doc.current = { text: props.value, sel: start(props.value) };
    past.current = [];
    future.current = [];
    draw(doc.current.sel);
  }, [props.value]); // eslint-disable-line react-hooks/exhaustive-deps

  const insertLines = (lines: string[]) => {
    if (!lines.length) return;
    const v = doc.current.text;
    const el = ref.current;
    const [s, e] = (el && selectionIn(el)) ?? (document.activeElement === el ? doc.current.sel : [v.length, v.length]);
    const before = s === 0 || v[s - 1] === "\n" ? (s >= 2 && v[s - 2] !== "\n" ? "\n" : "") : "\n\n";
    const after = e >= v.length ? "\n" : v[e] === "\n" ? (v[e + 1] === "\n" ? "" : "\n") : "\n\n";
    const block = lines.join("\n\n");
    const at = s + before.length + block.length;
    el?.focus();
    replace(s, e, before + block + after, [at, at]);
  };

  useImperativeHandle(handle, () => ({
    focus: () => {
      const el = ref.current;
      if (!el) return;
      el.focus();
      if (!selectionIn(el)) select(el, props.focusAt === "start" ? 0 : doc.current.text.length);
    },
    insertLines,
    selection: () => doc.current.sel,
    replaceRange: (from, to, text) => replace(from, to, text, [from, from + text.length]),
  }));

  useEffect(() => {
    const el = ref.current;
    if (props.autoFocus && el) {
      el.focus();
      select(el, doc.current.text.length);
    }
  }, [props.autoFocus]); // eslint-disable-line react-hooks/exhaustive-deps

  // the caret: for the suggestions, and a pending line goes back to blank once the caret leaves it
  const redraw = useRef(draw);
  redraw.current = draw;
  useEffect(() => {
    const onSel = () => {
      const el = ref.current;
      const s = el && !composing.current ? selectionIn(el) : null;
      if (!s) return;
      doc.current.sel = s;
      setCaret(s[0]);
      const p = pending.current;
      if (p !== null && !pressing.current && (s[0] !== s[1] || doc.current.text.slice(0, lineStart(doc.current.text, s[0])).split("\n").length - 1 !== p)) redraw.current(s, false);
    };
    const up = () => {
      if (!pressing.current) return;
      pressing.current = false;
      onSel();
    };
    document.addEventListener("selectionchange", onSel);
    window.addEventListener("pointerup", up);
    return () => {
      document.removeEventListener("selectionchange", onSel);
      window.removeEventListener("pointerup", up);
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // ---------- suggestions ----------

  const value = doc.current.text;
  const typing = focused && props.suggest ? props.suggest.at(value, caret) : null;
  const items = typing && dismissed !== caret ? props.suggest!.items(typing.query) : [];
  const listed = items.length > 0;
  const list = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    if (!listed || !typing) return setPos(null);
    const place = () => {
      const el = ref.current;
      const box = list.current;
      if (!el || !box) return;
      const r = document.createRange();
      const [n, o] = pointAt(el, typing.start);
      const [n2, o2] = pointAt(el, Math.min(typing.start + 1, doc.current.text.length));
      r.setStart(n, o);
      r.setEnd(n2, o2);
      let b = r.getBoundingClientRect();
      if (!b.height) b = el.getBoundingClientRect();
      const h = box.offsetHeight;
      const w = box.offsetWidth;
      const below = b.bottom + 4 + h <= window.innerHeight - 8 || b.top - 4 - h < 8;
      setPos({ left: Math.max(8, Math.min(b.left - 4, window.innerWidth - w - 8)), top: below ? b.bottom + 4 : b.top - 4 - h });
    };
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  }, [listed, typing?.start, caret, items.length]); // eslint-disable-line react-hooks/exhaustive-deps

  const pick = (item: string) => {
    if (!typing || !props.suggest) return;
    replace(typing.start, caret, props.suggest.insert(item, doc.current.text.slice(caret)));
  };

  const wrap = (mark: string, at?: [number, number]) => {
    const v = doc.current.text;
    const [s, e] = at ?? selectionIn(ref.current!) ?? doc.current.sel;
    const n = mark.length;
    if (v.slice(s - n, s) === mark && v.slice(e, e + n) === mark) replace(s - n, e + n, v.slice(s, e), [s - n, e - n]); // wrapped → unwrap
    else replace(s, e, mark + v.slice(s, e) + mark, [s + n, e + n]);
  };

  /** A format from the right-click list, on the selection it was opened over. */
  const format = (f: Exclude<Format, "-">, [s, e]: [number, number]) => {
    const v = doc.current.text;
    ref.current?.focus();
    if ("wrap" in f) return wrap(f.wrap, [s, e]);
    if ("insert" in f) return insertLines([f.insert]);
    // whole lines: from the start of the first selected line to the end of the last
    const ls = lineStart(v, s);
    const last = e > s && v[e - 1] === "\n" ? e - 1 : e;
    const le = v.indexOf("\n", last) < 0 ? v.length : v.indexOf("\n", last);
    if ("line" in f) {
      const strip = (l: string) => (props.flavor.linePrefix ? l.replace(props.flavor.linePrefix, "") : l);
      const numbered = /^\d+[.)] $/.test(f.line);
      const has = (l: string) => (numbered ? /^ *\d+[.)] /.test(l) : l.trimStart().startsWith(f.line));
      const lines = v.slice(ls, le).split("\n");
      const off = lines.every((l) => !l.trim() || has(l)) && lines.some((l) => l.trim());
      let n = 0;
      const out = lines.map((l) => (off ? strip(l) : !l.trim() && lines.length > 1 ? l : (numbered ? `${++n}. ` : f.line) + strip(l))).join("\n");
      return replace(ls, le, out, [ls + out.length, ls + out.length]);
    }
    const [a, b] = f.around;
    if (f.block) return replace(ls, le, a + v.slice(ls, le) + b, [ls + a.length + (le - ls), ls + a.length + (le - ls)]);
    // nothing selected: the caret between the two pieces; else the selection stays selected inside them
    replace(s, e, a + v.slice(s, e) + b, [s + a.length, e + a.length]);
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
    if (listed) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length);
        return;
      }
      if ((e.key === "Enter" && !e.metaKey) || e.key === "Tab") {
        e.preventDefault();
        pick(items[Math.min(active, items.length - 1)]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setDismissed(caret);
        return;
      }
    }
    if (e.metaKey && e.key === "Enter" && props.onSubmit) {
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
    const w = e.metaKey && !e.altKey ? props.flavor.wraps?.find((x) => x.key === k && !!x.shift === e.shiftKey) : undefined;
    if (w) {
      e.preventDefault();
      wrap(w.mark);
      return;
    }
    const v = doc.current.text;
    const [s, end] = selectionIn(e.currentTarget) ?? doc.current.sel;
    const ls = lineStart(v, s);
    const le = v.indexOf("\n", ls) < 0 ? v.length : v.indexOf("\n", ls);
    const li = props.flavor.line?.(v.slice(ls, le)) ?? null;
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      if (li && s === end && !e.shiftKey) {
        if (!li.empty) replace(s, s, `\n${li.next}`);
        else if (li.kind === "list" && li.indent >= 2) replace(ls, ls + 2, "", [s - 2, s - 2]); // an empty nested item: one level up
        else replace(ls, le, ""); // empty: the list / quote ends
      } else replace(s, end, "\n");
      return;
    }
    if (e.key === "Tab" && li?.kind === "list") {
      e.preventDefault();
      if (e.shiftKey) {
        if (li.indent >= 2) replace(ls, ls + 2, "", [s - 2, end - 2]);
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

  const host = listed ? (ref.current?.closest("dialog") ?? document.body) : null;
  return (
    <div className="relative">
      <div
        ref={ref}
        role="textbox"
        aria-multiline
        aria-label={props.label}
        contentEditable={props.readOnly ? false : "plaintext-only"}
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
        onPointerDown={() => (pressing.current = true)}
        onContextMenu={(e) => {
          if (props.readOnly) return;
          e.preventDefault();
          pressing.current = false;
          setMenu({ x: e.clientX, y: e.clientY, sel: selectionIn(e.currentTarget) ?? doc.current.sel });
        }}
        style={props.style}
        className={`block w-full leading-relaxed break-words whitespace-pre-wrap outline-none ${props.className ?? ""}`}
      />
      {!value && props.placeholder && <div className="pointer-events-none absolute top-0 left-0 leading-relaxed text-muted-foreground">{props.placeholder}</div>}
      {menu && (
        <MdMenu
          x={menu.x}
          y={menu.y}
          formats={props.flavor.formats ?? []}
          host={ref.current?.closest("dialog") ?? document.body}
          onFormat={(f) => format(f, menu.sel)}
          onInsert={(text) => {
            ref.current?.focus();
            replace(menu.sel[0], menu.sel[1], text);
          }}
          onClose={() => setMenu(null)}
        />
      )}
      {host &&
        createPortal(
          <div
            ref={list}
            role="listbox"
            aria-label={props.suggest!.label}
            onMouseDown={(e) => e.preventDefault()}
            style={pos ?? { left: 0, top: 0, visibility: "hidden" }}
            className="fixed z-50 max-w-80 min-w-48 animate-pop-in rounded-card border bg-popover p-1 text-foreground shadow-lg"
          >
            {items.map((t, i) => (
              <div
                key={t}
                role="option"
                aria-selected={i === active}
                onMouseEnter={() => setActive(i)}
                onClick={() => pick(t)}
                className={`cursor-default truncate rounded-md px-3 py-1 text-sm ${i === active ? "bg-muted" : ""}`}
              >
                {props.suggest!.show?.(t) ?? t}
              </div>
            ))}
          </div>,
          host,
        )}
    </div>
  );
});
