// The website's Markdown body, drawn while typing (念念不忘's way: the text is the model, one block per
// line, marks kept and dimmed; shared/editable.ts maps the caret). Headings, quotes, lists, rules,
// code fences, tables and Markdoc tags by line; **bold**, *italic*, `code`, ~~del~~, [links](…) inline;
// a line that is only an image shows the picture under it (not part of the text). ↩ continues a list
// or quote, Tab / ⇧Tab indent a list item, ⌘B / ⌘I bold / italic; the box keeps its own ⌘Z / ⌘⇧Z.
// Images dropped or pasted on it go into the work's image folder (Rust) and in as their own lines.
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, type ClipboardEvent, type KeyboardEvent } from "react";
import { lineStart, select, selectionIn, serialize } from "@/shared/editable";
import { useDropTarget } from "@/shared/attachments/target";
import { siteSrc } from "./api";

export type MdEditorHandle = {
  focus: () => void;
  insertLines: (lines: string[]) => void;
  /** The last selection in the box (offsets in the text). */
  selection: () => [number, number];
  replaceRange: (start: number, end: number, text: string) => void;
};

const MARK = "text-muted-foreground/45";
const LIST = /^( *)([-*+]|\d+[.)])( +)(.*)$/;
const QUOTE = /^( *>+ ?)(.*)$/;
const HEADING = /^(#{1,6})( +)(.*)$/;
const FENCE = /^ *(```|~~~)/;
const RULE = /^ *([-*_])( *\1){2,} *$/;
const TAG_LINE = /^ *\{%.*%\} *$/;
const IMAGE_LINE = /^ *!\[([^\]]*)\]\(([^)\s]+)(?: +"[^"]*")?\) *$/;
const HEAD_CLASS = ["", "text-[1.45em] font-bold", "text-[1.3em] font-bold", "text-[1.15em] font-bold", "font-bold", "font-bold", "font-bold"];

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const span = (cls: string, text: string) => (text ? (cls ? `<span class="${cls}">${esc(text)}</span>` : `<span>${esc(text)}</span>`) : "");

type Style = { bold?: boolean; italic?: boolean; strike?: boolean; link?: boolean };
const cls = (s: Style) => [s.bold && "font-bold", s.italic && "italic", s.strike && "line-through", s.link && "text-primary underline underline-offset-2"].filter(Boolean).join(" ");

// code span · image · link · **bold** / __bold__ · *italic* · _italic_ (not inside words) · ~~del~~ · {% tag %} · <url>
const INLINE =
  /(`+)(.+?)\1(?!`)|!\[([^\]]*)\]\(([^)]*)\)|\[([^\]]*)\]\(([^)]*)\)|(\*\*|__)(?=\S)(.*?\S)\7|\*(?=[^\s*])(.*?[^\s*])\*(?!\*)|(?<![A-Za-z0-9_])_(?=\S)(.*?\S)_(?![A-Za-z0-9_])|~~(?=\S)(.*?\S)~~|\{%.*?%\}|<https?:\/\/[^>\s]+>/g;

/** A line's inline marks: every character drawn once, in order. */
function inline(text: string, st: Style = {}): string {
  let out = "";
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    out += span(cls(st), text.slice(last, m.index));
    last = m.index + m[0].length;
    const [all] = m;
    if (m[1] !== undefined) out += span(MARK, m[1]) + span("rounded-sm bg-muted px-0.5 font-mono text-[0.9em]", m[2]) + span(MARK, m[1]);
    else if (m[3] !== undefined) out += span(MARK, "![") + span(cls(st), m[3]) + span(MARK, `](${m[4]})`);
    else if (m[5] !== undefined) out += span(MARK, "[") + inline(m[5], { ...st, link: true }) + span(MARK, `](${m[6]})`);
    else if (m[7] !== undefined) out += span(MARK, m[7]) + inline(m[8], { ...st, bold: true }) + span(MARK, m[7]);
    else if (m[9] !== undefined) out += span(MARK, "*") + inline(m[9], { ...st, italic: true }) + span(MARK, "*");
    else if (m[10] !== undefined) out += span(MARK, "_") + inline(m[10], { ...st, italic: true }) + span(MARK, "_");
    else if (m[11] !== undefined) out += span(MARK, "~~") + inline(m[11], { ...st, strike: true }) + span(MARK, "~~");
    else out += span(all.startsWith("<") ? "text-primary" : MARK, all);
  }
  return out + span(cls(st), text.slice(last));
}

function imageSrc(url: string): string | null {
  if (url.startsWith("/") && !url.startsWith("//")) return siteSrc(url);
  return /^https?:\/\//.test(url) ? url : null;
}

function lineHtml(line: string): { cls: string; html: string } {
  let m: RegExpExecArray | null;
  if ((m = HEADING.exec(line))) return { cls: `${HEAD_CLASS[m[1].length]} mt-1`, html: span(MARK, m[1] + m[2]) + inline(m[3], { bold: true }) };
  if (RULE.test(line) || TAG_LINE.test(line)) return { cls: "", html: span(MARK, line) };
  if (/^ *\|/.test(line)) return { cls: "font-mono text-[13px]", html: inline(line) };
  if ((m = IMAGE_LINE.exec(line))) {
    const src = imageSrc(m[2]);
    const pic = src ? `<span contenteditable="false" class="block select-none py-1"><img src="${esc(src)}" alt="" draggable="false" class="max-h-56 max-w-full rounded-control border object-contain"></span>` : "";
    return { cls: "", html: inline(line) + pic };
  }
  if ((m = QUOTE.exec(line))) return { cls: "border-l-2 pl-3 text-muted-foreground", html: span(MARK, m[1]) + inline(m[2]) };
  if ((m = LIST.exec(line))) return { cls: "", html: span(MARK, m[1] + m[2] + m[3]) + inline(m[4]) };
  return { cls: "", html: inline(line) };
}

function docHtml(text: string): string {
  let fence = false;
  return text
    .split("\n")
    .map((l) => {
      if (FENCE.test(l)) {
        fence = !fence;
        return `<div class="font-mono text-[13px]">${span(MARK, l)}</div>`;
      }
      if (fence) return `<div class="bg-muted/40 px-1 font-mono text-[13px]">${l ? esc(l) : "<br>"}</div>`;
      if (!l.trim()) return `<div class="h-[0.8em] leading-[0.8em]"><br></div>`;
      const { cls: c, html } = lineHtml(l);
      return `<div class="${c}">${html}</div>`;
    })
    .join("");
}

type Snap = { text: string; sel: [number, number] };
const HISTORY = 300;

export const MdEditor = forwardRef<MdEditorHandle, { value: string; onChange: (v: string) => void; onImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; label: string; onError: (e: unknown) => void; readOnly?: boolean }>(function MdEditor(props, handle) {
  const ref = useRef<HTMLDivElement>(null);
  const doc = useRef<Snap>({ text: props.value, sel: [0, 0] });
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
    props.onChange(text);
  };

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
    props.onChange(snap.text);
  };

  const sync = () => {
    const el = ref.current;
    if (!el || composing.current) return;
    const text = serialize(el);
    commit(text, selectionIn(el) ?? [text.length, text.length], true);
  };

  // a value from outside (another work, a reload, the translation) starts afresh
  useLayoutEffect(() => {
    if (props.value === doc.current.text && ref.current?.childNodes.length) return;
    doc.current = { text: props.value, sel: [0, 0] };
    past.current = [];
    future.current = [];
    draw();
  }, [props.value]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Lines in as their own paragraph at the caret (else at the end). */
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
      if (!selectionIn(el)) select(el, 0);
    },
    insertLines,
    selection: () => doc.current.sel,
    replaceRange: (start, end, text) => replace(start, end, text, [start, start + text.length]),
  }));

  useEffect(() => {
    const onSel = () => {
      const el = ref.current;
      const s = el && !composing.current ? selectionIn(el) : null;
      if (s) doc.current.sel = s;
    };
    document.addEventListener("selectionchange", onSel);
    return () => document.removeEventListener("selectionchange", onSel);
  }, []);

  const wrap = (mark: string) => {
    const value = doc.current.text;
    const [s, e] = selectionIn(ref.current!) ?? doc.current.sel;
    const n = mark.length;
    if (value.slice(s - n, s) === mark && value.slice(e, e + n) === mark) replace(s - n, e + n, value.slice(s, e), [s - n, e - n]);
    else replace(s, e, mark + value.slice(s, e) + mark, [s + n, e + n]);
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
    if (e.metaKey && !e.altKey && !e.shiftKey && (k === "b" || k === "i")) {
      e.preventDefault();
      wrap(k === "b" ? "**" : "*");
      return;
    }
    const value = doc.current.text;
    const [s, end] = selectionIn(e.currentTarget) ?? doc.current.sel;
    const ls = lineStart(value, s);
    const le = value.indexOf("\n", ls) < 0 ? value.length : value.indexOf("\n", ls);
    const line = value.slice(ls, le);
    const li = LIST.exec(line);
    const quote = QUOTE.exec(line);
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      if (s === end && !e.shiftKey && (li || quote)) {
        const rest = li ? li[4] : quote![2];
        if (!rest.trim()) {
          if (li && li[1].length >= 2) replace(ls, ls + 2, "", [s - 2, s - 2]); // an empty nested item: one level up
          else replace(ls, le, ""); // empty: the list / quote ends
        } else if (li) {
          const marker = /\d/.test(li[2]) ? `${Number(li[2].slice(0, -1)) + 1}${li[2].slice(-1)}` : li[2];
          replace(s, s, `\n${li[1]}${marker}${li[3]}`);
        } else replace(s, s, `\n${quote![1]}`);
      } else replace(s, end, "\n");
      return;
    }
    if (e.key === "Tab" && li) {
      e.preventDefault();
      if (e.shiftKey) {
        if (li[1].length >= 2) replace(ls, ls + 2, "", [s - 2, end - 2]);
      } else replace(ls, ls, "  ", [s + 2, end + 2]);
    }
  };

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

  const take = (p: Promise<string[]>) => void p.then((urls) => insertLines(urls.map((u) => `![](${u})`)), props.onError);
  const dropping = useDropTarget(ref, { paths: (paths) => take(props.onImages({ paths })), files: (files) => take(props.onImages({ files })) }, !props.readOnly);

  return (
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
      className={`min-h-[12em] w-full rounded-control text-[15px] leading-relaxed break-words whitespace-pre-wrap outline-none ${dropping ? "ring-2 ring-primary" : ""} ${props.readOnly ? "text-muted-foreground" : ""}`}
    />
  );
});
