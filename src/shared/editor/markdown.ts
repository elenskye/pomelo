// Markdown on the shared live editor (LiveText: the text is the model, one block per line, marks kept and dimmed):
// headings, quotes, lists, rules, code fences, tables and Markdoc tags by line; **bold**, *italic*, `code`, ~~del~~,
// [links](…) inline; a line that is only an image can show the picture under it (not part of the text). ↩ continues
// a list or quote, Tab / ⇧Tab indent a list item, ⌘B / ⌘I bold / italic. Used by 呦呦漫游's body (MdEditor) and
// every box where I write a prompt (MdField). Laid out as 见字如面 (index.css .lv-ln: every line a whole line, a blank one
// too, the same small gap after each; code and tables tight); an inline construct's marks show only while the caret is
// in it or next to it (`.fx.on`), a heading's `#` and a quote's `>` only on the caret's line (`.md-blk`).
import { endsIn, fxGroup, NO_ENDS, showGroups, type Ends } from "./editable";
import { iconHtml } from "./inlineIcon";
import type { Flavor, Format } from "./LiveText";

/** An image line's URL → the picture's src (null: no picture). */
type ImageSrc = (url: string) => string | null;

const MARK = "text-muted-foreground/45";
/** A format mark (** * _ ~~ \` and a link's brackets and address): shown only in the group holding the caret
 *  (index.css .md-mark, .fx); a block's (# >) only on the caret's line. Rules and code fences stay; a `- ` is a dot. */
const FMT = `md-mark ${MARK}`;
const BLK = `md-mark md-blk ${MARK}`;
const LIST = /^( *)([-*+]|\d+[.)])( +)(.*)$/;
const QUOTE = /^( *>+ ?)(.*)$/;
const HEADING = /^(#{1,6})( +)(.*)$/;
const FENCE = /^ *(```|~~~)/;
const RULE = /^ *([-*_])( *\1){2,} *$/;
const TAG_LINE = /^ *\{%.*%\} *$/;
const IMAGE_LINE = /^ *!\[([^\]]*)\]\(([^)\s]+)(?: +"[^"]*")?\) *$/;
const HEAD_CLASS = ["", "text-[1.45em] font-bold", "text-[1.3em] font-bold", "text-[1.15em] font-bold", "font-bold", "font-bold", "font-bold"];

/** Text into HTML (text and attribute values). */
export const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const span = (cls: string, text: string) => (text ? (cls ? `<span class="${cls}">${esc(text)}</span>` : `<span>${esc(text)}</span>`) : "");

type Style = { bold?: boolean; italic?: boolean; strike?: boolean; link?: boolean };
const cls = (s: Style) => [s.bold && "font-bold", s.italic && "italic", s.strike && "line-through", s.link && "text-primary underline underline-offset-2"].filter(Boolean).join(" ");

// code span · image · link · **bold** / __bold__ · *italic* · _italic_ (not inside words) · ~~del~~ · {% tag %} · <url> · :pm-icon:
const INLINE =
  /(`+)(.+?)\1(?!`)|!\[([^\]]*)\]\(([^)]*)\)|\[([^\]]*)\]\(([^)]*)\)|(\*\*|__)(?=\S)(.*?\S)\7|\*(?=[^\s*])(.*?[^\s*])\*(?!\*)|(?<![A-Za-z0-9_])_(?=\S)(.*?\S)_(?![A-Za-z0-9_])|~~(?=\S)(.*?\S)~~|\{%.*?%\}|<https?:\/\/[^>\s]+>|:pm-([a-z0-9-]+):/g;

/** A line's inline marks: every character drawn once, in order; each construct with marks a group (`.fx`). `at`: where
 * `text` starts in its line; `ends`: the selection's ends in the line (offsets in it). */
function inline(text: string, st: Style = {}, at = 0, ends: Ends = NO_ENDS): string {
  let out = "";
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    out += span(cls(st), text.slice(last, m.index));
    last = m.index + m[0].length;
    const [all] = m;
    const a = at + m.index;
    const fx = (html: string) => fxGroup(a, a + all.length, ends, html);
    const sub = (t: string, s2: Style, skip: number) => inline(t, s2, a + skip, ends);
    if (m[1] !== undefined) out += fx(span(FMT, m[1]) + span("rounded-sm bg-muted px-0.5 font-mono text-[0.9em]", m[2]) + span(FMT, m[1]));
    else if (m[3] !== undefined) out += span(MARK, "![") + span(cls(st), m[3]) + span(MARK, `](${m[4]})`);
    else if (m[5] !== undefined) out += fx(span(FMT, "[") + sub(m[5], { ...st, link: true }, 1) + span(FMT, `](${m[6]})`));
    else if (m[7] !== undefined) out += fx(span(FMT, m[7]) + sub(m[8], { ...st, bold: true }, m[7].length) + span(FMT, m[7]));
    else if (m[9] !== undefined) out += fx(span(FMT, "*") + sub(m[9], { ...st, italic: true }, 1) + span(FMT, "*"));
    else if (m[10] !== undefined) out += fx(span(FMT, "_") + sub(m[10], { ...st, italic: true }, 1) + span(FMT, "_"));
    else if (m[11] !== undefined) out += fx(span(FMT, "~~") + sub(m[11], { ...st, strike: true }, 2) + span(FMT, "~~"));
    else if (m[12] !== undefined) out += iconHtml(m[12]) ?? span(cls(st), all);
    else out += span(all.startsWith("<") ? "text-primary" : MARK, all);
  }
  return out + span(cls(st), text.slice(last));
}

/** One line (not in a code fence); `tight`: no gap after it (a table row). */
function lineHtml(line: string, imageSrc: ImageSrc, ends: Ends): { cls: string; html: string; tight?: boolean } {
  let m: RegExpExecArray | null;
  if ((m = HEADING.exec(line))) return { cls: `${HEAD_CLASS[m[1].length]} mt-1`, html: span(BLK, m[1] + m[2]) + inline(m[3], { bold: true }, m[1].length + m[2].length, ends) };
  if (RULE.test(line) || TAG_LINE.test(line)) return { cls: "", html: span(MARK, line) };
  if (/^ *\|/.test(line)) return { cls: "font-mono text-[13px]", html: inline(line, {}, 0, ends), tight: true };
  if ((m = IMAGE_LINE.exec(line))) {
    const src = imageSrc(m[2]);
    const pic = src ? `<span contenteditable="false" class="block select-none py-1"><img src="${esc(src)}" alt="" draggable="false" class="max-h-56 max-w-full rounded-control border object-contain"></span>` : "";
    return { cls: "", html: inline(line, {}, 0, ends) + pic };
  }
  if ((m = QUOTE.exec(line))) return { cls: "border-l-2 pl-3 text-muted-foreground", html: span(BLK, m[1]) + inline(m[2], {}, m[1].length, ends) };
  if ((m = LIST.exec(line))) {
    const head = /\d/.test(m[2]) ? span(MARK, m[1] + m[2] + m[3]) : `${esc(m[1])}<span class="lv-bul">${esc(m[2])}</span>${esc(m[3])}`;
    return { cls: "", html: head + inline(m[4], {}, m[1].length + m[2].length + m[3].length, ends) };
  }
  return { cls: "", html: inline(line, {}, 0, ends) };
}

function docHtml(text: string, imageSrc: ImageSrc, active?: number | null, sel?: [number, number] | null): string[] {
  let fence = false;
  let start = 0;
  return text.split("\n").map((l, i) => {
    const ends = i === active ? endsIn(sel, start, l.length) : NO_ENDS;
    start += l.length + 1;
    const cur = i === active ? " cur" : "";
    // a code fence is tight (no gap between its lines)
    if (FENCE.test(l)) {
      fence = !fence;
      return `<div class="font-mono text-[13px]">${span(MARK, l)}</div>`;
    }
    if (fence) return `<div class="bg-muted/40 px-1 font-mono text-[13px]">${l ? esc(l) : "<br>"}</div>`;
    if (!l.trim()) return `<div class="lv-ln${cur}">${l ? esc(l) : "<br>"}</div>`;
    const { cls: c, html, tight } = lineHtml(l, imageSrc, ends);
    return `<div class="${tight ? "" : "lv-ln"}${cur}${c ? ` ${c}` : ""}">${html}</div>`;
  });
}

/** What the right-click list offers in Markdown. */
const FORMATS: Format[] = [
  { label: "加粗", keys: "⌘B", wrap: "**", group: "文字" },
  { label: "斜体", keys: "⌘I", wrap: "*", group: "文字" },
  { label: "删除线", wrap: "~~", group: "文字" },
  { label: "行内代码", wrap: "`", group: "文字" },
  { label: "链接", around: ["[", "](https://)"], group: "文字" },
  { label: "大标题", line: "# ", group: "段落" },
  { label: "中标题", line: "## ", group: "段落" },
  { label: "小标题", line: "### ", group: "段落" },
  { label: "列表", line: "- ", group: "段落" },
  { label: "编号列表", line: "1. ", group: "段落" },
  { label: "引用", line: "> ", group: "段落" },
  { label: "代码块", around: ["```\n", "\n```"], block: true, group: "插入" },
  { label: "分隔线", insert: "---", group: "插入" },
];

/** Markdown on LiveText; `image`: where a line that is only an image shows its picture from (none: no picture). */
export function markdownFlavor(image?: ImageSrc): Flavor {
  const src: ImageSrc = image ?? (() => null);
  return {
    caretAware: true,
    draw: (text, _pending, active, sel) => docHtml(text, src, active, sel),
    onCaret: showGroups,
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
    formats: FORMATS,
    linePrefix: /^ *(?:#{1,6} +|[-*+] +|\d+[.)] +|>+ ?)/,
  };
}
