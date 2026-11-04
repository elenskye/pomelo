// The website's Markdown body on the shared live editor (shared/editor/LiveText: the text is the model,
// one block per line, marks kept and dimmed). This file is the Markdown flavor: headings, quotes, lists,
// rules, code fences, tables and Markdoc tags by line; **bold**, *italic*, `code`, ~~del~~, [links](…)
// inline; a line that is only an image shows the picture under it (not part of the text). ↩ continues
// a list or quote, Tab / ⇧Tab indent a list item, ⌘B / ⌘I bold / italic; the box keeps its own ⌘Z / ⌘⇧Z.
// Images dropped or pasted on it go into the work's image folder (Rust) and in as their own lines.
import { forwardRef, useImperativeHandle, useRef } from "react";
import { useDropTarget } from "@/shared/attachments/target";
import { LiveText, type Flavor, type LiveTextHandle } from "@/shared/editor/LiveText";
import { siteSrc } from "./api";

export type MdEditorHandle = LiveTextHandle;

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

function docHtml(text: string, pending: number | null): string {
  let fence = false;
  return text
    .split("\n")
    .map((l, i) => {
      if (FENCE.test(l)) {
        fence = !fence;
        return `<div class="font-mono text-[13px]">${span(MARK, l)}</div>`;
      }
      if (fence) return `<div class="bg-muted/40 px-1 font-mono text-[13px]">${l ? esc(l) : "<br>"}</div>`;
      if (!l.trim()) return i === pending ? "<div><br></div>" : `<div class="h-[0.8em] leading-[0.8em]"><br></div>`;
      const { cls: c, html } = lineHtml(l);
      return `<div class="${c}">${html}</div>`;
    })
    .join("");
}

const FLAVOR: Flavor = {
  draw: docHtml,
  line: (line) => {
    const li = LIST.exec(line);
    if (li) {
      const marker = /\d/.test(li[2]) ? `${Number(li[2].slice(0, -1)) + 1}${li[2].slice(-1)}` : li[2];
      return { kind: "list", indent: li[1].length, empty: !li[4].trim(), next: li[1] + marker + li[3] };
    }
    const q = QUOTE.exec(line);
    return q ? { kind: "quote", indent: 0, empty: !q[2].trim(), next: q[1] } : null;
  },
  wraps: [
    { key: "b", mark: "**" },
    { key: "i", mark: "*" },
  ],
};

export const MdEditor = forwardRef<MdEditorHandle, { value: string; onChange: (v: string) => void; onImages: (take: { paths?: string[]; files?: File[] }) => Promise<string[]>; label: string; onError: (e: unknown) => void; readOnly?: boolean }>(function MdEditor(props, handle) {
  const box = useRef<HTMLDivElement>(null);
  const live = useRef<LiveTextHandle>(null);
  useImperativeHandle(handle, () => ({
    focus: () => live.current?.focus(),
    insertLines: (lines) => live.current?.insertLines(lines),
    selection: () => live.current?.selection() ?? [0, 0],
    replaceRange: (start, end, text) => live.current?.replaceRange(start, end, text),
  }));
  const take = (p: Promise<string[]>) => void p.then((urls) => live.current?.insertLines(urls.map((u) => `![](${u})`)), props.onError);
  const dropping = useDropTarget(box, { paths: (paths) => take(props.onImages({ paths })), files: (files) => take(props.onImages({ files })) }, !props.readOnly);
  return (
    <LiveText
      ref={live}
      boxRef={box}
      value={props.value}
      onChange={props.onChange}
      flavor={FLAVOR}
      label={props.label}
      readOnly={props.readOnly}
      focusAt="start"
      className={`min-h-[12em] rounded-control text-[15px] ${dropping ? "ring-2 ring-primary" : ""} ${props.readOnly ? "text-muted-foreground" : ""}`}
    />
  );
});
