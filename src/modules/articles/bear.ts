// 见字如面's Markdown on LiveText, after Bear: CommonMark plus Bear's own (~underline~, ==highlight== with its colour
// patches 🟢🔴🔵🟡🟣 inside, `> [!NOTE]` callouts, $math$ / $$math$$, ```mermaid, folding), drawn as it will read.
// One div per line (LiveText), the text is the model. Marks show only where the caret is (Bear 2): an inline construct
// (`**b**`, a link, `code`, $math$ …, a `.fx` group) while an end of the selection is in it or right next to it; a block's
// (a quote's `>`, a table's pipes, a code fence, a rule) while the caret is in that block. Elsewhere they are hidden and a few are drawn as what they mean instead:
// a task's `- [ ]` is a checkbox, a picture's `![](…)` the picture, a formula / diagram its rendering, a link's
// address hidden. Rendered bits are `data-pv` (editable.ts: they count for nothing in the text) and
// contenteditable=false; clicking one puts the caret into its source (Editor.tsx).
// A heading's `#` marks (after Bear 2) sit in the left margin as one unit the caret steps over, drawn as the level
// (lines and its number) on every line, the caret's too; ⌫ right after it takes it whole (the line is text again).
// Folding (headings, list items with children) is the page's state, keyed by the line's text and its occurrence; Bear's
// mark that a line is folded (`<!-- {"fold":true} -->` at its end, brought by 搬家) opens it shut, the mark drawn as a mark.
// 柚子链接 (shared/links): `[[目标]]` the module's sticker and the name, `[[目标|字]]` the words as a Pomelo link, a line that
// is only `![[念念不忘/…]]` the note as a small card (its source shows above it on the caret's line).
import { showGroups } from "@/shared/editor/editable";
import { iconHtml } from "@/shared/editor/inlineIcon";
import { esc } from "@/shared/editor/markdown";
import type { Flavor, Format, LineKind } from "@/shared/editor/LiveText";
import { KINDS, targetOf, type Thing } from "@/shared/links/links";
import { noteCardHtml } from "@/shared/links/noteHtml";
import { markSvg } from "@/shared/marks";

export type Look = {
  /** A picture written into the text by name → its src (null: not one of the article's pictures). */
  image: (name: string) => string | null;
  /** The name is one of the article's other files. */
  file: (name: string) => boolean;
  /** What a [[link]]'s target names (null: 找不到; undefined: not known yet, drawn as alive). */
  thing: (target: string) => Thing | null | undefined;
  /** A module's sticker as HTML, small (`""`: none). */
  sticker: (module: string) => string;
  /** Rendered formula HTML (null: bad TeX). */
  math: (tex: string, display: boolean) => string | null;
  /** A diagram's SVG once rendered (null: not yet / failed — the Editor draws again when it's ready). */
  mermaid: (src: string) => { svg: string } | { error: string } | null;
  /** Highlighted HTML of a code block, one string per line (null: plain). */
  code: (src: string, lang: string) => string[] | null;
  /** Folded keys (see foldKey). */
  folds: Set<string>;
};

const span = (cls: string, text: string) => (text ? `<span class="${cls}">${esc(text)}</span>` : "");
const mk = (text: string, extra = "") => span(`mk${extra ? ` ${extra}` : ""}`, text);
const pv = (html: string, attrs = "", tag = "span") => `<${tag} data-pv contenteditable="false" ${attrs}>${html}</${tag}>`;

export const HEADING = /^(#{1,6})([ \t]+)(.*)$/;
export const LIST = /^(\s*)([-*+]|\d{1,9}[.)])([ \t]+)(\[[ xX]\][ \t]+)?(.*)$/;
const QUOTE = /^(\s*(?:>[ \t]?)+)(.*)$/;
const FENCE = /^(\s*)(`{3,}|~{3,})\s*([^`\s]*)?.*$/;
const RULE = /^ {0,3}([-*_])([ \t]*\1){2,}[ \t]*$/;
const TABLE_SEP = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;
const FOOTDEF = /^(\[\^[^\]\s]+\]:)([ \t]*)(.*)$/;
const IMAGE_ONLY = /^\s*!\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)\s*$/;
/** A line that is only a set-in thing: `![[…]]`. */
const EMBED_ONLY = /^\s*!\[\[([^\]\n]+)\]\]\s*$/;
const CALLOUT = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]([+-]?)/i;
/** The five callouts: its name (menus, the Word export), its tint and the icon that heads it in the text. */
export const CALLOUTS: Record<string, { label: string; tint: string; icon: string }> = {
  note: { label: "备注", tint: "sky", icon: "pen-line" },
  tip: { label: "提示", tint: "mint", icon: "lightbulb" },
  important: { label: "重要", tint: "grape", icon: "star" },
  warning: { label: "注意", tint: "orange", icon: "triangle-alert" },
  caution: { label: "小心", tint: "pink", icon: "flame" },
};
/** A callout's head: its icon in its tint (no words; `data-label` keeps the name for the Word export). */
const calloutHead = (c: { label: string; icon: string }) =>
  pv((markSvg(`icons/${c.icon}`) ?? "").replace(/^<svg[^>]*>/, '<svg viewBox="0 0 24 24" fill="none" aria-hidden="true">'), `class="co-head" data-label="${esc(c.label)}"`);
/** Bear's highlighter colours: the patch goes right after `==`. */
export const HIGHLIGHTS: { patch: string; label: string; tint: string | null }[] = [
  { patch: "", label: "默认", tint: null },
  { patch: "🟢", label: "绿", tint: "mint" },
  { patch: "🔴", label: "红", tint: "pink" },
  { patch: "🔵", label: "蓝", tint: "sky" },
  { patch: "🟡", label: "黄", tint: "yuzu" },
  { patch: "🟣", label: "紫", tint: "grape" },
];

/** A [[link]]'s inside: [target (Bear's \\x escapes undone), alias, the target as written]; the first | not escaped splits. */
export function wikiParts(inner: string): [string, string | undefined, string] {
  const i = inner.search(/(?<!\\)\|/);
  const raw = i < 0 ? inner : inner.slice(0, i);
  return [raw.replace(/\\([!-/:-@[-`{-~])/g, "$1"), i < 0 ? undefined : inner.slice(i + 1), raw];
}
/** Text with each \\x escape's backslash as a mark. */
const escMarks = (s: string) => s.split(/(\\(?=[!-/:-@[-`{-~]))/).map((p) => (p === "\\" ? mk(p) : esc(p))).join("");

export const decodeName = (u: string) => {
  try {
    return decodeURIComponent(u.replace(/^<|>$/g, ""));
  } catch {
    return u;
  }
};
/** An attachment's name as written in a link (articles.rs `encode_name`). */
export const encodeName = (n: string) => n.replace(/[%\s()#?<>]/g, (c) => ({ "%": "%25", " ": "%20", "(": "%28", ")": "%29", "#": "%23", "?": "%3F", "<": "%3C", ">": "%3E" })[c] ?? encodeURIComponent(c));

// ---------- inline ----------

type St = { b?: boolean; i?: boolean; s?: boolean; u?: boolean; hl?: string | null; link?: boolean };
const cls = (st: St, base = "") =>
  [base, st.b && "b", st.i && "i", st.s && "s", st.u && "u", st.hl !== undefined && `hl hl-${st.hl ?? "def"}`, st.link && "lk"].filter(Boolean).join(" ");
const txt = (st: St, t: string) => (t ? `<span${cls(st) ? ` class="${cls(st)}"` : ""}>${esc(t)}</span>` : "");

const URL_RE = /^(?:https?:\/\/|www\.)[^\s<>"'，。；！？、）】」』]+/iu;
// code · $math$ · image · [[wiki]] · [^fn] · [text](url) · **b** / __b__ · ~~s~~ · ==hl== · *i* · _i_ · ~u~ · <url> · :pm-icon: · #tag · url
const INLINE =
  /(`+)(.+?)\1(?!`)|\$(?=\S)([^$\n]*?\S)\$(?!\d)|!\[([^\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)|\[\[([^\]\n]+)\]\]|\[\^([^\]\s]+)\]|\[([^\]]*)\]\(([^)\s]*)(?:\s+"[^"]*")?\)|(\*\*|__)(?=\S)(.*?\S)\10|~~(?=\S)(.*?\S)~~|==(?=\S)(🟢|🔴|🔵|🟡|🟣)?(.*?\S)==|\*(?=[^\s*])(.*?[^\s*])\*(?!\*)|(?<![\p{L}\p{N}_])_(?=\S)(.*?\S)_(?![\p{L}\p{N}_])|(?<!~)~(?=[^\s~])([^~]*?[^\s~])~(?!~)|<(https?:\/\/[^>\s]+)>|:pm-([a-z0-9-]+):|(?<=^|\s)#([^\s#]+)|(?<![\w/])((?:https?:\/\/|www\.)[^\s<>"'，。；！？、）】」』]+)/gu;

/** Where the selection's ends are in the line being drawn (offsets in the line); none when the box isn't focused. */
type Ends = readonly number[];
const NO_ENDS: Ends = [];
/** One inline construct with marks, [a, b] in its line: its marks show while an end of the selection is in it or right
 * next to it (Bear 2); elsewhere it reads as what it means. `onCaret` keeps the class right as the caret moves. No
 * offsets in the HTML: typing before it mustn't change how its line is drawn (LiveText keeps a line the browser typed). */
const group = (a: number, b: number, ends: Ends, html: string) => `<span class="fx${ends.some((e) => e >= a && e <= b) ? " on" : ""}">${html}</span>`;

/** `at`: where `text` starts in its line (for the groups' offsets). */
function inline(text: string, look: Look, st: St = {}, at = 0, ends: Ends = NO_ENDS): string {
  let out = "";
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    out += txt(st, text.slice(last, m.index));
    last = m.index + m[0].length;
    const all = m[0];
    const a = at + m.index;
    const fx = (html: string) => group(a, a + all.length, ends, html);
    const sub = (t: string, s2: St, skip: number) => inline(t, look, s2, a + skip, ends);
    if (m[1] !== undefined) out += fx(mk(m[1]) + span(cls(st, "cd"), m[2]) + mk(m[1]));
    else if (m[3] !== undefined) {
      const html = look.math(m[3], false);
      out += html ? fx(span("msrc", all) + pv(html, 'class="mrender" data-src="inline"')) : txt(st, all);
    } else if (m[4] !== undefined) {
      const name = decodeName(m[5]);
      const src = look.image(name);
      out += src ? fx(span("mk imgsrc", all) + pv(`<img src="${esc(src)}" alt="${esc(m[4])}" draggable="false">`, 'class="img-inline"')) : span(look.file(name) ? "plainsrc" : "missing", all);
    } else if (m[6] !== undefined) {
      const [target, alias, raw] = wikiParts(m[6]);
      const t = look.thing(target.trim());
      const dead = t === null ? " dead" : "";
      if (alias !== undefined) out += fx(mk("[[") + mk(`${raw}|`, "url") + `<span class="${cls(st, `pm-lkw${dead}`)}" data-link="${esc(target.trim())}">${esc(alias)}</span>` + mk("]]"));
      else {
        // the module's name is a mark; the sticker draws in front of the name
        const { module } = targetOf(target);
        const label = module === "articles" ? "" : (KINDS.find(([k]) => k === module)?.[1] ?? "");
        const head = label && raw.startsWith(`${label}/`) ? label.length + 1 : 0;
        const mark = look.sticker(module);
        // a note is written as its time: its opening words show on hover
        const hint = t && t.module === "notes" && t.title ? ` title="${esc(t.title)}"` : "";
        out += fx(mk(`[[${raw.slice(0, head)}`) + `<span class="${cls(st, `pm-lk${dead}`)}" data-link="${esc(target.trim())}"${hint}>${mark ? pv(mark, 'class="pm-lk-mk"') : ""}${escMarks(raw.slice(head))}</span>` + mk("]]"));
      }
    } else if (m[7] !== undefined) out += fx(`<sup class="fn">${mk("[^")}${esc(m[7])}${mk("]")}</sup>`);
    else if (m[9] !== undefined) {
      const name = decodeName(m[9]);
      if (look.file(name) || look.image(name)) out += fx(mk("[") + `<span class="${cls(st, "att")}" data-file="${esc(name)}">📎 ${esc(m[8])}</span>` + mk(`](${m[9]})`, "url"));
      else out += fx(mk("[") + `<span class="${cls({ ...st, link: true })}" data-href="${esc(m[9])}">${sub(m[8], { ...st, link: true }, 1)}</span>` + mk(`](${m[9]})`, "url"));
    } else if (m[10] !== undefined) out += fx(mk(m[10]) + sub(m[11], { ...st, b: true }, m[10].length) + mk(m[10]));
    else if (m[12] !== undefined) out += fx(mk("~~") + sub(m[12], { ...st, s: true }, 2) + mk("~~"));
    else if (m[14] !== undefined) {
      const tint = HIGHLIGHTS.find((h) => h.patch && h.patch === m[13])?.tint ?? null;
      out += fx(mk(`==${m[13] ?? ""}`) + sub(m[14], { ...st, hl: tint }, 2 + (m[13]?.length ?? 0)) + mk("=="));
    } else if (m[15] !== undefined) out += fx(mk("*") + sub(m[15], { ...st, i: true }, 1) + mk("*"));
    else if (m[16] !== undefined) out += fx(mk("_") + sub(m[16], { ...st, i: true }, 1) + mk("_"));
    else if (m[17] !== undefined) out += fx(mk("~") + sub(m[17], { ...st, u: true }, 1) + mk("~"));
    else if (m[18] !== undefined) out += fx(mk("<") + `<span class="${cls({ ...st, link: true })}" data-href="${esc(m[18])}">${esc(m[18])}</span>` + mk(">"));
    else if (m[19] !== undefined) out += iconHtml(m[19]) ?? txt(st, all);
    else if (m[20] !== undefined) {
      const tag = m[20].replace(/\/+$/, "");
      out += tag && !tag.startsWith("/") && !tag.includes("//") && [...tag].length <= 50 ? `<span class="pm-hashtag" data-tags="articles" data-tag="${esc(tag)}"><span class="pm-hash">#</span>${esc(m[20])}</span>` : txt(st, all);
    } else if (m[21] !== undefined && URL_RE.test(m[21])) {
      const url = m[21].replace(/[.,;:!?)\]]+$/, "");
      out += `<span class="${cls({ ...st, link: true })}" data-href="${esc(url)}">${esc(url)}</span>` + txt(st, m[21].slice(url.length));
    } else out += txt(st, all);
  }
  return out + txt(st, text.slice(last));
}

// ---------- blocks ----------

type Block = { kind: "fence" | "math" | "table" | "quote"; from: number; to: number; lang?: string; callout?: string; fold?: boolean };

/** Code fences, $$ blocks, tables and quotes (callouts) as line ranges, inclusive. */
export function blocks(lines: string[]): Block[] {
  const out: Block[] = [];
  for (let i = 0; i < lines.length; i++) {
    const f = FENCE.exec(lines[i]);
    if (f) {
      const mark = f[2];
      let j = i + 1;
      while (j < lines.length && !(lines[j].trim().startsWith(mark[0].repeat(mark.length)) && lines[j].trim().replace(new RegExp(`^\\${mark[0]}+`), "") === "")) j++;
      out.push({ kind: "fence", from: i, to: Math.min(j, lines.length - 1), lang: (f[3] ?? "").toLowerCase() });
      i = j;
      continue;
    }
    if (lines[i].trim() === "$$" || (lines[i].trim().startsWith("$$") && !lines[i].trim().endsWith("$$"))) {
      let j = i + 1;
      while (j < lines.length && !lines[j].trim().endsWith("$$")) j++;
      if (j < lines.length) {
        out.push({ kind: "math", from: i, to: j });
        i = j;
        continue;
      }
    }
    if (/^\s*\$\$.+\$\$\s*$/.test(lines[i])) {
      out.push({ kind: "math", from: i, to: i });
      continue;
    }
    if (/^\s*\|/.test(lines[i]) && i + 1 < lines.length && TABLE_SEP.test(lines[i + 1])) {
      let j = i + 2;
      while (j < lines.length && /^\s*\|/.test(lines[j])) j++;
      out.push({ kind: "table", from: i, to: j - 1 });
      i = j - 1;
      continue;
    }
    if (QUOTE.test(lines[i])) {
      let j = i;
      while (j + 1 < lines.length && QUOTE.test(lines[j + 1])) j++;
      const c = CALLOUT.exec(QUOTE.exec(lines[i])![2]);
      out.push({ kind: "quote", from: i, to: j, callout: c?.[1].toLowerCase(), fold: c?.[2] === "-" });
      i = j;
    }
  }
  return out;
}

/** A table row's cells as [start, end) offsets in the line (between the pipes). */
export function cells(line: string): [number, number][] {
  const out: [number, number][] = [];
  let start = line.indexOf("|") + 1;
  if (start === 0) return out;
  let code = false;
  for (let i = start; i < line.length; i++) {
    if (line[i] === "`") code = !code;
    if (line[i] === "\\") {
      i++;
      continue;
    }
    if (line[i] === "|" && !code) {
      out.push([start, i]);
      start = i + 1;
    }
  }
  if (line.slice(start).trim()) out.push([start, line.length]);
  return out;
}

const wide = (s: string) => [...s].reduce((n, c) => n + (/[ᄀ-￿]/.test(c) ? 2 : 1), 0);

/** Bear's mark that a line is folded, at a heading's or list item's end (articles.rs `without_fold`). */
export const FOLD_MARK = /\s*<!--\s*\{"fold":true\}\s*-->\s*$/;

/** The folds Bear's marks ask for (a new box opens with them shut). */
export function markedFolds(text: string): Set<string> {
  const out = new Set<string>();
  if (!text.includes("<!--")) return out;
  const lines = text.split("\n");
  const fenced = new Set<number>(blocks(lines).filter((b) => b.kind === "fence" || b.kind === "math").flatMap((b) => Array.from({ length: b.to - b.from + 1 }, (_, k) => b.from + k)));
  lines.forEach((l, i) => {
    if (FOLD_MARK.test(l) && !fenced.has(i) && foldRange(lines, i, fenced)) out.add(foldKey(lines, i));
  });
  return out;
}

/** The fold key of a line: its text and how many lines with the same text came before. */
export function foldKey(lines: string[], i: number): string {
  let n = 0;
  for (let j = 0; j < i; j++) if (lines[j] === lines[i]) n++;
  return `${n}:${lines[i]}`;
}

const indentOf = (l: string) => l.replace(/\t/g, "    ").match(/^ */)![0].length;

/** Which lines a fold at line `i` hides (a heading: up to the next heading as big; a list item: its deeper lines). */
export function foldRange(lines: string[], i: number, fenced: Set<number>): [number, number] | null {
  const h = HEADING.exec(lines[i]);
  if (h && !fenced.has(i)) {
    let j = i + 1;
    while (j < lines.length && !(fenced.has(j) ? false : (HEADING.exec(lines[j])?.[1].length ?? 7) <= h[1].length)) j++;
    return j > i + 1 ? [i + 1, j - 1] : null;
  }
  const li = LIST.exec(lines[i]);
  if (li && !fenced.has(i)) {
    const base = indentOf(lines[i]);
    let j = i + 1;
    let last = i;
    while (j < lines.length && (lines[j].trim() === "" || indentOf(lines[j]) > base)) {
      if (lines[j].trim()) last = j;
      j++;
    }
    return last > i ? [i + 1, last] : null;
  }
  return null;
}

/** `margin`: a heading's marks go into the left margin as its level (not inside a quote). `at`: where `line` starts in its
 * line of text; `ends`: the selection's ends there. */
function lineHtml(line: string, look: Look, margin = false, at = 0, ends: Ends = NO_ENDS): { cls: string; html: string; style?: string } {
  let m: RegExpExecArray | null;
  if ((m = HEADING.exec(line))) {
    const n = m[1].length;
    const marks = margin ? `<span class="hg" contenteditable="false" data-level="${n}"><span class="hg-src">${esc(m[1] + m[2])}</span></span>` : mk(m[1] + m[2], "hm");
    // an empty heading: a <br> after the margin's unit gives the caret its place at the line's start (WebKit draws a caret
    // right after a non-editable unit that ends the line at the line's far end)
    return { cls: `h h${n}`, html: marks + (m[3] ? inline(m[3], look, {}, at + m[1].length + m[2].length, ends) : margin ? "<br>" : "") };
  }
  if (RULE.test(line)) return { cls: "hr", html: mk(line) };
  if ((m = FOOTDEF.exec(line))) return { cls: "fndef", html: mk(m[1]) + esc(m[2]) + inline(m[3], look, {}, at + m[1].length + m[2].length, ends) };
  if ((m = EMBED_ONLY.exec(line)) && targetOf(wikiParts(m[1])[0]).module === "notes") {
    const target = wikiParts(m[1])[0].trim();
    return { cls: "embline", html: span("mk embsrc", line) + pv(noteCardHtml(look.thing(target), look.sticker), `class="emb" data-emb="${esc(target)}"`, "div") };
  }
  if ((m = IMAGE_ONLY.exec(line))) {
    const name = decodeName(m[2]);
    const src = look.image(name);
    if (src) return { cls: "imgline", html: span("mk imgsrc", line) + pv(`<img src="${esc(src)}" alt="${esc(m[1])}" draggable="false">`, 'class="img"') };
  }
  if ((m = LIST.exec(line))) {
    const level = Math.floor(indentOf(line) / 2);
    const ordered = /\d/.test(m[2]);
    const hang = level * 1.5 + (m[4] ? 1.5 : ordered ? 1.6 + Math.max(0, m[2].length - 2) * 0.5 : 1.2);
    const ind = m[1] ? `<span class="ind" style="width:${level * 1.5}em">${esc(m[1])}</span>` : "";
    const marker = m[4] ? `<span class="bul-hide">${esc(m[2] + m[3])}</span>` : ordered ? span("num", m[2] + m[3]) : `<span class="bul">${esc(m[2])}</span>${esc(m[3])}`;
    const box = m[4]
      ? span("mk task-src", m[4]) + pv(`<span class="box${/x/i.test(m[4]) ? " on" : ""}"></span>`, `class="task" data-task="1" role="checkbox" aria-checked="${/x/i.test(m[4])}"`)
      : "";
    const done = m[4] && /x/i.test(m[4]);
    return { cls: `li${done ? " done" : ""}`, style: `--hang:${hang}em`, html: ind + marker + box + inline(m[5], look, {}, at + line.length - m[5].length, ends) };
  }
  return { cls: "p", html: inline(line, look, {}, at, ends) };
}

const gap = (i: number, pending: number | null) => (i === pending ? `<div class="ln"><br></div>` : `<div class="ln gap"><br></div>`);

/** The whole text, one HTML string per line; `active`: the caret's line (its block shows its marks). */
export function drawDoc(text: string, pending: number | null, active: number | null | undefined, look: Look, sel?: [number, number] | null): string[] {
  const lines = text.split("\n");
  // each line's start, and the selection's ends in the lines holding them (groups there show their marks)
  const startOf: number[] = [];
  for (let i = 0, p = 0; i < lines.length; p += lines[i].length + 1, i++) startOf.push(p);
  const endsOf = (i: number): Ends => (sel && active != null ? [sel[0], sel[1]].filter((e) => e >= startOf[i] && e <= startOf[i] + lines[i].length).map((e) => e - startOf[i]) : NO_ENDS);
  const bs = blocks(lines);
  const blockOf = new Map<number, Block>();
  for (const b of bs) for (let i = b.from; i <= b.to; i++) blockOf.set(i, b);
  // fold keys in one pass (foldKey's rule); a block's source, rendering and widths once per block
  const seen = new Map<string, number>();
  const keys = lines.map((l) => {
    const n = seen.get(l) ?? 0;
    seen.set(l, n + 1);
    return `${n}:${l}`;
  });
  const per = new Map<Block, { code?: string; hl?: string[] | null; math?: string; html?: string | null; widths?: number[] }>();
  const of = (b: Block) => per.get(b) ?? (per.set(b, {}), per.get(b)!);
  const fenced = new Set<number>(bs.filter((b) => b.kind === "fence" || b.kind === "math").flatMap((b) => Array.from({ length: b.to - b.from + 1 }, (_, k) => b.from + k)));
  const curBlock = active != null ? blockOf.get(active) : undefined;
  const isCur = (i: number) => active === i || (!!curBlock && i >= curBlock.from && i <= curBlock.to);
  // folds
  const hidden = new Set<number>();
  const folded = new Set<number>();
  const foldable = new Set<number>();
  for (let i = 0; i < lines.length; i++) {
    if (hidden.has(i)) continue;
    const r = foldRange(lines, i, fenced);
    if (!r) continue;
    foldable.add(i);
    if (look.folds.has(keys[i]) && !(active != null && active > r[0] - 1 && active <= r[1])) {
      folded.add(i);
      for (let j = r[0]; j <= r[1]; j++) hidden.add(j);
    }
  }
  const out: string[] = [];
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    const cur = isCur(i) ? " cur" : "";
    const hid = hidden.has(i) ? " hid" : "";
    const b = blockOf.get(i);
    const fold = foldable.has(i) ? pv("", `class="fold${folded.has(i) ? " shut" : ""}" data-fold="${esc(keys[i])}" aria-label="${folded.has(i) ? "展开" : "折叠"}"`) : "";
    const more = folded.has(i) ? pv("…", 'class="folded"') : "";
    if (b?.kind === "fence") {
      const edge = i === b.from || i === b.to;
      const c = of(b);
      const code = (c.code ??= lines.slice(b.from + 1, b.to).join("\n"));
      if (b.lang === "mermaid" && !isCur(i)) {
        if (i === b.from) {
          const r = look.mermaid(code);
          const body = r && "svg" in r ? r.svg : r ? `<span class="pv-err">${esc(r.error)}</span>` : `<span class="pv-wait">正在画图…</span>`;
          out.push(`<div class="ln mm${hid}">${span("srchid", l)}${pv(body, `class="diagram" data-at="${b.from}"`, "div")}</div>`);
        } else out.push(`<div class="ln srcline${hid}">${esc(l) || "<br>"}</div>`);
        continue;
      }
      if (edge) {
        const r = b.lang === "mermaid" && i === b.to && isCur(i) ? look.mermaid(code) : null;
        const after = r && "svg" in r ? pv(r.svg, 'class="diagram under"', "div") : "";
        out.push(`<div class="ln fence${cur}${hid}${i === b.from ? " top" : " bottom"}">${mk(l) || "<br>"}${after}</div>`);
        continue;
      }
      const hl = c.hl === undefined ? (c.hl = look.code(code, b.lang ?? "")) : c.hl;
      const html = hl?.[i - b.from - 1] ?? esc(l);
      out.push(`<div class="ln code${cur}${hid}">${html || "<br>"}</div>`);
      continue;
    }
    if (b?.kind === "math") {
      const c = of(b);
      const src = (c.math ??= lines.slice(b.from, b.to + 1).join("\n").trim().replace(/^\$\$|\$\$/g, ""));
      const html = c.html === undefined ? (c.html = look.math(src, true)) : c.html;
      if (!isCur(i)) {
        if (i === b.from) out.push(`<div class="ln mm${hid}">${span("srchid", l)}${pv(html ?? `<span class="pv-err">${esc(src)}</span>`, `class="formula" data-at="${b.from}"`, "div")}</div>`);
        else out.push(`<div class="ln srcline${hid}">${esc(l) || "<br>"}</div>`);
        continue;
      }
      const after = i === b.to && html ? pv(html, 'class="formula under"', "div") : "";
      out.push(`<div class="ln mathsrc cur${hid}">${esc(l) || "<br>"}${after}</div>`);
      continue;
    }
    if (b?.kind === "table") {
      const t = of(b);
      if (!t.widths) {
        const rows = lines.slice(b.from, b.to + 1).map(cells);
        const n = Math.max(...rows.map((r) => r.length));
        t.widths = Array.from({ length: n }, (_, c) => Math.max(3, ...rows.map((r, k) => (k === 1 || !r[c] ? 0 : wide(lines[b.from + k].slice(r[c][0], r[c][1]).trim())))));
      }
      const widths = t.widths;
      if (i === b.from + 1) {
        out.push(`<div class="ln tsep${cur}${hid}">${mk(l)}</div>`);
        continue;
      }
      const cs = cells(l);
      const ends = endsOf(i);
      let html = "";
      let last = 0;
      cs.forEach(([s, e], c) => {
        html += mk(l.slice(last, s));
        html += `<span class="td" style="min-width:${(widths[c] ?? 3) * 0.62 + 1.2}em">${inline(l.slice(s, e), look, {}, s, ends)}</span>`;
        last = e;
      });
      html += mk(l.slice(last));
      out.push(`<div class="ln tr${i === b.from ? " th" : ""}${i === b.to ? " tlast" : ""}${cur}${hid}">${html}</div>`);
      continue;
    }
    if (b?.kind === "quote") {
      const q = QUOTE.exec(l)!;
      const c = b.callout ? CALLOUTS[b.callout] : null;
      const tint = c ? ` co co-${c.tint}` : "";
      // a callout's first line with nothing after `[!TIP]` is only its mark: away from the caret it is not drawn and
      // the icon heads the next line (the box starts there); with the caret in the box it shows, the icon on it
      const bare = !!c && b.to > b.from && !isCur(b.from) && !QUOTE.exec(lines[b.from])![2].replace(CALLOUT, "").trim();
      const first = bare ? b.from + 1 : b.from;
      const edge = `${i === first ? " qtop" : ""}${i === b.to ? " qbottom" : ""}`;
      const head = c && i === first ? calloutHead(c) : "";
      if (c && i === b.from) {
        const cm = CALLOUT.exec(q[2])!;
        out.push(`<div class="ln q${tint}${bare ? " cohid" : edge}${cur}${hid}">${mk(q[1])}${mk(cm[0], "comark")}${head}${inline(q[2].slice(cm[0].length), look, {}, q[1].length + cm[0].length, endsOf(i))}</div>`);
        continue;
      }
      if (!q[2].trim()) {
        out.push(`<div class="ln q${tint}${edge}${cur}${hid}">${mk(q[1])}${head || (q[1] ? "" : "<br>")}</div>`);
        continue;
      }
      const { cls: c2, html, style } = lineHtml(q[2], look, false, q[1].length, endsOf(i));
      out.push(`<div class="ln q ${c2}${tint}${edge}${cur}${hid}"${style ? ` style="${style}"` : ""}>${mk(q[1])}${head}${html}</div>`);
      continue;
    }
    if (!l.trim()) {
      out.push(hidden.has(i) ? `<div class="ln hid"><br></div>` : gap(i, pending));
      continue;
    }
    const fm = FOLD_MARK.exec(l);
    const { cls: c2, html, style } = lineHtml(fm ? l.slice(0, fm.index) : l, look, true, 0, endsOf(i));
    out.push(`<div class="ln ${c2}${cur}${hid}"${style ? ` style="${style}"` : ""}>${fold}${html}${fm ? mk(fm[0], "foldmk") : ""}${more}</div>`);
  }
  return out;
}

// ---------- writing ----------

/** ↩ continues lists (a task's next one is open), quotes and callouts; an empty one ends. */
function lineKind(line: string): LineKind | null {
  const li = LIST.exec(line);
  if (li) {
    const marker = /\d/.test(li[2]) ? `${Number(li[2].slice(0, -1)) + 1}${li[2].slice(-1)}` : li[2];
    return { kind: "list", indent: li[1].length, empty: !li[5].trim(), next: li[1] + marker + li[3] + (li[4] ? "[ ] " : "") };
  }
  const q = QUOTE.exec(line);
  return q ? { kind: "quote", indent: 0, empty: !q[2].trim(), next: q[1].endsWith(" ") ? q[1] : `${q[1]} ` } : null;
}

export const LINE_PREFIX = /^ *(?:#{1,6} +|[-*+] +(?:\[[ xX]\] +)?|\d+[.)] +|>+ ?)/;

/** The right-click list: Bear's marks, 念念不忘's keys where the two differ. In layers (MdMenu's groups) — the page's
 * own on top (`lead`: 记到念念不忘, 柚子链接…), then 文字 / 段落 / 插入 and what the page adds to them or after them (公式,
 * 提示框, a table's rows and columns, 柚子). */
export function formats(extra: Format[]): Format[] {
  const lead = extra.filter((f) => f !== "-" && f.lead);
  const rest = extra.filter((f) => f === "-" || !f.lead);
  return [...lead, ...(lead.length ? ["-" as const] : []), ...BASE, ...rest];
}

const BASE: Format[] = [
  { label: "加粗", keys: "⌘B", wrap: "**", group: "文字" },
  { label: "斜体", keys: "⌘I", wrap: "*", group: "文字" },
  { label: "下划线", keys: "⌘U", wrap: "~", group: "文字" },
  { label: "删除线", keys: "⌘⇧E", wrap: "~~", group: "文字" },
  { label: "高亮", keys: "⌘⇧H", wrap: "==", group: "文字" },
  { label: "行内代码", keys: "⌘⇧C", wrap: "`", group: "文字" },
  { label: "链接", keys: "⌘⇧K", around: ["[", "](https://)"], group: "文字" },
  { label: "大标题", keys: "⌥⌘1", line: "# ", group: "段落" },
  { label: "中标题", keys: "⌥⌘2", line: "## ", group: "段落" },
  { label: "小标题", keys: "⌥⌘3", line: "### ", group: "段落" },
  { label: "列表", line: "- ", group: "段落" },
  { label: "编号列表", line: "1. ", group: "段落" },
  { label: "待办", keys: "⌘⇧L", line: "- [ ] ", group: "段落" },
  { label: "引用", line: "> ", group: "段落" },
  { label: "代码块", around: ["```\n", "\n```"], block: true, group: "插入" },
  { label: "表格", insert: "| 列 1 | 列 2 |\n| --- | --- |\n|  |  |", group: "插入" },
  { label: "分隔线", insert: "---", group: "插入" },
];

/** 见字如面's flavor; `look` is read at every draw (folds, pictures, renders change without a new flavor). */
export function bearFlavor(look: () => Look, extra: () => Format[]): Flavor {
  return {
    caretAware: true,
    draw: (text, pending, active, sel) => drawDoc(text, pending, active, look(), sel),
    onCaret: showGroups,
    line: lineKind,
    wraps: [
      { key: "b", mark: "**" },
      { key: "i", mark: "*" },
      { key: "u", mark: "~" },
      { key: "e", shift: true, mark: "~~" },
      { key: "h", shift: true, mark: "==" },
      { key: "c", shift: true, mark: "`" },
    ],
    get formats() {
      return formats(extra());
    },
    linePrefix: LINE_PREFIX,
  };
}
