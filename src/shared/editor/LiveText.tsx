// The shared live editor (念念不忘, 速记小窗, 呦呦漫游; new modules use LiteEditor or their own Flavor): a
// plain-text editable box drawn one block (div) per line by a Flavor, the text being the model. The
// browser types and deletes inside a line; everything else (↩, paste, drop, shortcuts) edits the text
// here; after each change the lines are drawn again and the caret put back by its offset (editable.ts).
// Own ⌘Z / ⌘⇧Z (the page's ⌘Z skips editables); typing, or deleting, in a row is one step.
// - Only lines that changed are touched: the flavor draws one HTML string per line, compared with what was drawn last;
//   a line that only gained or lost a class keeps its nodes (the caret stays put), a line the browser already typed
//   just so is left alone, and the caret is put back only when its place moved.
// - An IME (拼音) owns the box while it composes: nothing is drawn or read back, and nothing is drawn with text not read
//   back yet (`dirty`). Its text is read the frame after it ends (an IME may commit part and go on composing at once),
//   checked against what it committed where it began, and if WebKit left the DOM otherwise, the text wins (drawn again).
//   Keys, edits and the page's own redraws first read back what the IME left.
// - The blank line the caret is on right after an edit is drawn as the line about to be written
//   (`pending`: its full height and the gap it will have), so ↩ shows the new paragraph at once instead
//   of when its first character arrives. It is drawn as a blank line again once the caret leaves it
//   (not while the mouse is down: redrawing would break a drag-selection).
// - Suggest (e.g. #tags): a list at the caret while `at` finds something being typed; ↑↓, ↩ / Tab, Esc.
//   It is fixed-positioned (under the typed trigger, or above it when there's no room below) in the
//   page or the dialog around the box, so no scrolling box clips it; in a small window (速记小窗) it takes the roomier
//   side and scrolls inside it, never over the line being written.
// - A right-click opens MdMenu at the pointer: the flavor's formats (applied to the selection, or its lines) and
//   the two icon libraries (an icon goes in at the caret). `:pm-<key>:` is drawn as its icon (inlineIcon.ts).
// - A caret-aware flavor (见字如面) is told the caret's line and selection and drawn again whenever that line changes (not
//   while the mouse is down or an IME composes); moves within the line go to its `onCaret` (classes only), so only the
//   marks of what the caret is in show.
import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
  useState,
  type ClipboardEvent,
  type CompositionEvent,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { lineStart, lineStartsOf, select, selectionIn, serialize, pointAt } from "./editable";
import { MdMenu } from "./MdMenu";

/** What ↩ continues on a line: a list item or a quote; `next` starts the new line (indent + marker). */
export type LineKind = { kind: "list" | "quote"; indent: number; empty: boolean; next: string };

/** One row of the right-click list (`"-"`: a line between groups): `wrap` puts a mark around the selection (again =
 *  off), `line` starts each selected line with it (again = off; `1. ` counts up), `around` puts the selection between
 *  two pieces (`block`: on lines of their own), `insert` adds a line of its own. `keys`: its shortcut, shown. */
export type Format = "-" | ({ label: string; keys?: string; /** Bold (记到念念不忘, 柚子链接…). */ lead?: boolean; icon?: ReactNode; /** The row it folds into with the others of this name (文字, 段落…: a second strip beside the list). */ group?: string } & ({ wrap: string } | { line: string } | { around: [string, string]; block?: boolean } | { insert: string } | { act: (sel: [number, number]) => void }));

export type Flavor = {
  /** The box's HTML, one div per line (every character drawn once), one string each. `pending`: the blank line to draw
   * as a line about to be written. */
  draw: (text: string, pending: number | null, active?: number | null, sel?: [number, number] | null) => string[];
  /** Drawn again when the caret moves to another line (`active` = its line, `sel` the selection; null when the box isn't
   * focused). */
  caretAware?: boolean;
  /** The caret moved within its line (no redraw): the flavor may restyle what is drawn (classes only, so the next draw
   * with this selection comes out the same). */
  onCaret?: (box: HTMLElement, sel: [number, number], starts: number[]) => void;
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
  /** Replace [start, end) with `text` and put the selection at `sel` (one ⌘Z step). */
  edit: (start: number, end: number, text: string, sel: [number, number]) => void;
  /** A format (as the right-click list has them) on the current selection. */
  applyFormat: (f: Exclude<Format, "-">) => void;
  /** Draw again (the flavor's look changed: folds, a picture loaded). */
  redraw: () => void;
  text: () => string;
  select: (start: number, end?: number) => void;
  /** Read back text an IME left (before saving). */
  flush: () => void;
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

const OPEN = /^<div(?: class="([^"]*)")?(?: style="([^"]*)")?>/;
/** A line's HTML as its own tag's class / style and the rest. */
const parts = (h: string) => {
  const m = OPEN.exec(h);
  return m ? { cls: m[1] ?? null, style: m[2] ?? null, inner: h.slice(m[0].length) } : null;
};
const tpl = typeof document === "undefined" ? null : document.createElement("template");
const nodes = (html: string) => {
  tpl!.innerHTML = html;
  return tpl!.content;
};

/** Brings the box from `prev` (what was drawn) to `next`, touching only lines that differ; true when nodes were replaced. */
function paint(el: HTMLElement, prev: string[], next: string[]): boolean {
  const kids = Array.from(el.children) as HTMLElement[];
  const n = kids.length;
  if (!prev.length || el.childNodes.length !== n || kids.some((k) => k.nodeName !== "DIV")) {
    el.innerHTML = next.join("");
    return true;
  }
  const lim = Math.min(prev.length, next.length, n);
  let p = 0;
  while (p < lim && prev[p] === next[p]) p++;
  let s = 0;
  while (s < lim - p && prev[prev.length - 1 - s] === next[next.length - 1 - s]) s++;
  if (n === prev.length && n === next.length) {
    let replaced = false;
    for (let i = p; i < n - s; i++) {
      if (prev[i] === next[i]) continue;
      const k = kids[i];
      const a = parts(prev[i]);
      const b = parts(next[i]);
      if (a && b && a.inner === b.inner) {
        // only the line's own class / style: its nodes (and a caret in them) stay
        if (b.cls === null) k.removeAttribute("class");
        else k.className = b.cls;
        if (b.style === null) k.removeAttribute("style");
        else k.setAttribute("style", b.style);
        continue;
      }
      if (k.outerHTML === next[i]) continue; // the browser typed it just so
      k.replaceWith(nodes(next[i]));
      replaced = true;
    }
    return replaced;
  }
  // lines came or went (↩, a deletion across lines): the middle is drawn again
  const anchor = kids[n - s] ?? null;
  for (let i = p; i < n - s; i++) kids[i].remove();
  el.insertBefore(nodes(next.slice(p, next.length - s).join("")), anchor);
  return true;
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
    /** Keys of the caller's own (before the box's); true = handled. */
    onKey?: (e: KeyboardEvent<HTMLDivElement>) => boolean;
    /** Pasted files (a screenshot): the caller stores them and writes them in. */
    onFiles?: (files: File[]) => void;
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
  /** The box shows text not read back yet (an IME composed it). */
  const dirty = useRef(false);
  /** Waiting for the frame after an IME ended to read its text. */
  const settling = useRef(false);
  /** An IME at work: the text and selection when it began, and what it has committed since (compositionend's data). */
  const ime = useRef<{ base: Snap; committed: string } | null>(null);
  /** What the box shows: the text, each line's HTML and where each line starts. */
  const drawn = useRef<{ text: string; html: string[]; starts: number[] }>({ text: "", html: [], starts: [0] });
  const pending = useRef<number | null>(null);
  const pressing = useRef(false);
  const [caret, setCaret] = useState(0);
  const [focused, setFocused] = useState(false);
  const [active, setActive] = useState(0);
  const [dismissed, setDismissed] = useState<number | null>(null); // caret where Esc closed the list
  const [menu, setMenu] = useState<{ x: number; y: number; sel: [number, number] } | null>(null);
  const drawnLine = useRef<number | null>(null);
  /** The line of `pos` in the text the box shows (else counted). */
  const lineOf = (text: string, pos: number) => {
    if (drawn.current.text !== text) return text.slice(0, lineStart(text, pos)).split("\n").length - 1;
    const st = drawn.current.starts;
    let lo = 0;
    let hi = st.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (st[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };
  /** Line starts when the box shows the model's text (the fast way to offsets), else undefined. */
  const known = () => (!dirty.current && drawn.current.text === doc.current.text ? drawn.current.starts : undefined);

  /** Draw the text; `mark`: a blank line holding the (collapsed) caret is drawn as pending. Never under an IME, never
   * over text not read back (that is read first, which draws). */
  const draw = (sel?: [number, number], mark = true) => {
    const el = ref.current;
    if (!el || composing.current) return;
    if (dirty.current) return void (!settling.current && sync());
    const text = doc.current.text;
    pending.current = mark && sel && sel[0] === sel[1] ? blankAt(text, sel[0]) : null;
    const focused = document.activeElement === el;
    const starts = drawn.current.text === text ? drawn.current.starts : lineStartsOf(text);
    drawn.current = { ...drawn.current, text, starts };
    const active = props.flavor.caretAware && focused ? lineOf(text, (sel ?? doc.current.sel)[0]) : null;
    drawnLine.current = active;
    const html = props.flavor.draw(text, pending.current, active, active === null ? null : (sel ?? doc.current.sel));
    const replaced = paint(el, drawn.current.html, html);
    drawn.current.html = html;
    if (!sel || !focused) return;
    const now = window.getSelection();
    const at = replaced || now?.anchorNode === el || now?.focusNode === el ? null : selectionIn(el, starts);
    if (!at || at[0] !== sel[0] || at[1] !== sel[1]) select(el, sel[0], sel[1], starts);
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
    const changed = text !== doc.current.text;
    doc.current = { text, sel };
    draw(sel);
    setCaret(sel[0]);
    setActive(0);
    if (changed) props.onChange(text);
  };

  /** Text an IME left and not read back yet: read it now (before an edit of ours works on the text). */
  const flush = () => {
    if (dirty.current && !composing.current) sync();
  };

  /** Replace [start, end) with `text`; the caret after it unless given. */
  const replace = (from: number, to: number, text: string, sel?: [number, number]) => {
    flush();
    const v = doc.current.text;
    const at = from + text.length;
    commit(v.slice(0, from) + text + v.slice(to), sel ?? [at, at]);
  };

  const travel = (back: boolean) => {
    flush();
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

  // the browser typed or deleted: read the text back. After an IME the text is what it committed where it began: WebKit
  // sometimes leaves its DOM otherwise (on a line with no text yet it took a heading's marks and the line break with it,
  // or added a line break after) — then that is drawn again from the text.
  function sync() {
    const el = ref.current;
    if (!el) return;
    if (composing.current) return void (dirty.current = true);
    dirty.current = false;
    const text = serialize(el);
    const m = ime.current;
    ime.current = null;
    if (m) {
      const [s0, s1] = m.base.sel;
      const meant = m.base.text.slice(0, s0) + m.committed + m.base.text.slice(s1);
      if (text !== meant) {
        drawn.current.html = [];
        const at = s0 + m.committed.length;
        commit(meant, [at, at], true);
        return;
      }
    }
    commit(text, selectionIn(el, lineStartsOf(text)) ?? [text.length, text.length], true);
  }
  const onInput = () => {
    if (composing.current || settling.current) dirty.current = true;
    else sync();
  };
  // an IME: hands off while it composes; its text is read the frame after it ends (or a moment later when no frame
  // comes: a hidden window), unless it has started composing again by then
  const onCompositionStart = () => {
    // a new run of composing (not the next piece of one still being read back): where it begins, the text read first
    if (!ime.current) {
      if (dirty.current && !settling.current) sync();
      const el = ref.current;
      const sel = (el && !dirty.current && selectionIn(el, known())) || doc.current.sel;
      ime.current = { base: { text: doc.current.text, sel }, committed: "" };
    }
    composing.current = true;
    dirty.current = true;
  };
  const onCompositionEnd = (e: CompositionEvent<HTMLDivElement>) => {
    if (ime.current) ime.current.committed += e.data ?? "";
    composing.current = false;
    dirty.current = true;
    if (settling.current) return;
    settling.current = true;
    let done = false;
    const go = () => {
      if (done) return;
      done = true;
      settling.current = false;
      if (!composing.current) sync();
    };
    requestAnimationFrame(go);
    window.setTimeout(go, 120);
  };

  // first draw; a value set from outside (cleared after 记下, another note / work) starts afresh
  useLayoutEffect(() => {
    if (props.value === doc.current.text && ref.current?.childNodes.length) return;
    doc.current = { text: props.value, sel: start(props.value) };
    past.current = [];
    future.current = [];
    if (!composing.current) dirty.current = false;
    draw(doc.current.sel);
  }, [props.value]); // eslint-disable-line react-hooks/exhaustive-deps

  const insertLines = (lines: string[]) => {
    if (!lines.length) return;
    flush();
    const v = doc.current.text;
    const el = ref.current;
    const [s, e] = (el && selectionIn(el, known())) ?? (document.activeElement === el ? doc.current.sel : [v.length, v.length]);
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
      if (!selectionIn(el, known())) select(el, props.focusAt === "start" ? 0 : doc.current.text.length, undefined, known());
    },
    insertLines,
    selection: () => (flush(), doc.current.sel),
    replaceRange: (from, to, text) => replace(from, to, text, [from, from + text.length]),
    edit: (from, to, text, sel) => replace(from, to, text, sel),
    applyFormat: (f) => (flush(), format(f, (ref.current && selectionIn(ref.current, known())) ?? doc.current.sel)),
    redraw: () => draw(doc.current.sel, false),
    text: () => (flush(), doc.current.text),
    select: (from, to) => {
      const el = ref.current;
      if (!el || composing.current) return;
      flush();
      el.focus();
      doc.current.sel = [from, to ?? from];
      draw(doc.current.sel, false);
      select(el, from, to ?? from, known());
    },
    flush,
  }));

  useEffect(() => {
    const el = ref.current;
    if (props.autoFocus && el) {
      el.focus();
      const at = props.focusAt === "start" ? 0 : doc.current.text.length;
      select(el, at, at, known());
    }
  }, [props.autoFocus]); // eslint-disable-line react-hooks/exhaustive-deps

  // the caret: for the suggestions, and a pending line goes back to blank once the caret leaves it
  const redraw = useRef(draw);
  redraw.current = draw;
  useEffect(() => {
    const onSel = () => {
      const el = ref.current;
      if (!el || composing.current) return;
      // an IME's text not read back: the offsets would be off (and a redraw would wipe it)
      if (dirty.current) return void (!settling.current && el.contains(window.getSelection()?.anchorNode ?? null) && sync());
      const s = selectionIn(el, known());
      if (!s) return;
      doc.current.sel = s;
      setCaret(s[0]);
      if (pressing.current) return;
      const line = lineOf(doc.current.text, s[0]);
      if (props.flavor.caretAware && line !== drawnLine.current) return void redraw.current(s, false);
      const p = pending.current;
      if (p !== null && (s[0] !== s[1] || line !== p)) return void redraw.current(s, false);
      const st = known();
      if (props.flavor.onCaret && st) props.flavor.onCaret(el, s, st);
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
  const [pos, setPos] = useState<{ left: number; top: number; maxHeight: number } | null>(null);
  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [active, listed]);
  useLayoutEffect(() => {
    if (!listed || !typing) return setPos(null);
    const place = () => {
      const el = ref.current;
      const box = list.current;
      if (!el || !box) return;
      const r = document.createRange();
      const [n, o] = pointAt(el, typing.start, known());
      const [n2, o2] = pointAt(el, Math.min(typing.start + 1, doc.current.text.length), known());
      r.setStart(n, o);
      r.setEnd(n2, o2);
      let b = r.getBoundingClientRect();
      if (!b.height) b = el.getBoundingClientRect();
      // its whole height (it may be capped already), the room under and over the line
      const full = box.scrollHeight + box.offsetHeight - box.clientHeight;
      const w = box.offsetWidth;
      const [under, over] = [window.innerHeight - 8 - (b.bottom + 4), b.top - 4 - 8];
      const below = full <= under || under >= over;
      const room = Math.max(80, below ? under : over);
      setPos({ left: Math.max(8, Math.min(b.left - 4, window.innerWidth - w - 8)), top: below ? b.bottom + 4 : b.top - 4 - Math.min(full, room), maxHeight: room });
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
    if (!typing || !props.suggest || dirty.current) return;
    replace(typing.start, caret, props.suggest.insert(item, doc.current.text.slice(caret)));
  };

  const wrap = (mark: string, at?: [number, number]) => {
    flush();
    const v = doc.current.text;
    const [s, e] = at ?? selectionIn(ref.current!, known()) ?? doc.current.sel;
    const n = mark.length;
    if (v.slice(s - n, s) === mark && v.slice(e, e + n) === mark) replace(s - n, e + n, v.slice(s, e), [s - n, e - n]); // wrapped → unwrap
    else replace(s, e, mark + v.slice(s, e) + mark, [s + n, e + n]);
  };

  /** A format from the right-click list, on the selection it was opened over. */
  const format = (f: Exclude<Format, "-">, [s, e]: [number, number]) => {
    flush();
    const v = doc.current.text;
    ref.current?.focus();
    if ("wrap" in f) return wrap(f.wrap, [s, e]);
    if ("insert" in f) return insertLines([f.insert]);
    if ("act" in f) return f.act([s, e]);
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
    flush();
    if (!listed && props.onKey?.(e)) return;
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
    const [s, end] = selectionIn(e.currentTarget, known()) ?? doc.current.sel;
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
    if (composing.current) return;
    const el = ref.current!;
    const at = (): [number, number] => (flush(), selectionIn(el, known()) ?? doc.current.sel);
    switch (e.inputType) {
      case "insertParagraph":
      case "insertLineBreak": {
        e.preventDefault();
        const [s, end] = at();
        replace(s, end, "\n");
        break;
      }
      case "insertFromPaste":
      case "insertFromDrop":
      case "insertFromYank": {
        e.preventDefault();
        const [s, end] = at();
        replace(s, end, (e.dataTransfer?.getData("text/plain") ?? e.data ?? "").replace(/\r\n?/g, "\n"));
        break;
      }
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
    flush();
    const [s, end] = selectionIn(e.currentTarget, known()) ?? [0, 0];
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
        onInput={onInput}
        onCompositionStart={onCompositionStart}
        onCompositionEnd={onCompositionEnd}
        onKeyDown={onKeyDown}
        onCopy={(e) => copy(e, false)}
        onPaste={(e) => {
          const files = [...e.clipboardData.files];
          if (!props.onFiles || !files.length || props.readOnly) return;
          e.preventDefault();
          props.onFiles(files);
        }}
        onCut={(e) => copy(e, true)}
        onFocus={() => {
          setFocused(true);
          if (props.flavor.caretAware) requestAnimationFrame(() => ref.current && !pressing.current && !composing.current && document.activeElement === ref.current && draw(selectionIn(ref.current, known()) ?? doc.current.sel, false));
        }}
        onBlur={() => {
          setFocused(false);
          flush();
          if (props.flavor.caretAware && drawnLine.current !== null) draw(undefined, false);
        }}
        onPointerDown={() => (pressing.current = true)}
        onContextMenu={(e) => {
          if (props.readOnly) return;
          e.preventDefault();
          pressing.current = false;
          flush();
          setMenu({ x: e.clientX, y: e.clientY, sel: selectionIn(e.currentTarget, known()) ?? doc.current.sel });
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
            className="fixed z-50 max-w-80 min-w-48 animate-pop-in overflow-y-auto rounded-card border bg-popover p-1 text-foreground shadow-lg"
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
