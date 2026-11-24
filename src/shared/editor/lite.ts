// Markdown-lite, 念念不忘's format (also the 速记小窗's): lines = paragraphs, `- ` / `1. ` lists (2 spaces
// per level), **bold**, ==highlight==, ++underline++, bare URLs, (optionally) #tags, `:pm-<key>:` icons
// (inlineIcon.ts) and 柚子链接 `[[目标]]` / `[[目标|字]]` (shared/links: marks dimmed, the name drawn as a link). The tag rule mirrors notes.rs — keep them in step. `liteFlavor` draws it in LiveText the way NoteBody shows it,
// laid out as 见字如面 (index.css .lv-ln): every line a whole line, a blank one too, the same small gap after each; a
// `- ` item's marker drawn as a dot. Marks (** == ++, a link's brackets) show only where the caret is (`.fx.on`, as in
// 见字如面); the box not being edited shows none.
import { endsIn, fxOpen, NO_ENDS, showGroups, type Ends } from "./editable";
import { iconHtml, iconSpans } from "./inlineIcon";
import { foundIn, targetOf, KINDS } from "@/shared/links/links";
import "@/shared/links/links.css";
import type { Flavor, Format } from "./LiveText";

const TAG_MAX = 50;
const validTag = (t: string) => t.length > 0 && !t.startsWith("/") && !t.includes("//") && [...t].length <= TAG_MAX;

type Span = { start: number; end: number; kind: "tag" | "url" | "icon" | "link"; value: string };

/** Tag spans in a line (`start` is the `#`). */
export function tagSpans(line: string): Span[] {
  const out: Span[] = [];
  for (const m of line.matchAll(/(?<=^|\s)#([^\s#]*)/gu)) {
    const tag = m[1].replace(/\/+$/, "");
    if (validTag(tag)) out.push({ start: m.index, end: m.index + 1 + tag.length, kind: "tag", value: tag });
  }
  return out;
}

const URL_RE = /(?:https?:\/\/|www\.)[^\s<>"'，。；！？、）】」』]+/giu;

/** 柚子链接 (a `!` before one is just a character here), tags (unless `tags` is false), URLs and icons in a line, in order. */
export function atoms(line: string, tags = true): Span[] {
  const spans: Span[] = line.includes("[[") ? foundIn(line).map((f) => ({ start: f.embed ? f.start + 1 : f.start, end: f.end, kind: "link" as const, value: line.slice(f.embed ? f.start + 1 : f.start, f.end) })) : [];
  if (tags) for (const t of tagSpans(line)) if (!spans.some((x) => x.start < t.end && t.start < x.end)) spans.push(t);
  for (const [start, end, value] of iconSpans(line)) if (!spans.some((t) => t.start < end && start < t.end)) spans.push({ start, end, kind: "icon", value });
  for (const m of line.matchAll(URL_RE)) {
    const url = m[0].replace(/[.,;:!?)\]]+$/, "");
    const s = { start: m.index, end: m.index + url.length, kind: "url" as const, value: url };
    if (!spans.some((t) => t.start < s.end && s.start < t.end)) spans.push(s);
  }
  return spans.sort((a, b) => a.start - b.start);
}

type Mark = "**" | "==" | "++";
const ALNUM = /[A-Za-z0-9]/;

/** Paired mark positions (index → mark). `==` / `++` must not touch ASCII letters on the outside (C++). */
export function pairMarks(line: string, skip: Span[]): Map<number, Mark> {
  const inAtom = (i: number) => skip.some((s) => i >= s.start && i < s.end);
  const open = new Map<Mark, number>();
  const pairs = new Map<number, Mark>();
  for (let i = 0; i < line.length - 1; i++) {
    const m = line.slice(i, i + 2);
    if ((m !== "**" && m !== "==" && m !== "++") || inAtom(i)) continue;
    const before = line[i - 1] ?? "";
    const after = line[i + 2] ?? "";
    const strict = m !== "**";
    const o = open.get(m);
    if (o !== undefined && !/\s/.test(before) && i > o + 2 && !(strict && ALNUM.test(after))) {
      pairs.set(o, m);
      pairs.set(i, m);
      open.delete(m);
    } else if (o === undefined && after && !/\s/.test(after) && !(strict && (ALNUM.test(before) || before === m[0]))) {
      open.set(m, i);
    }
    i++; // marks are two chars
  }
  return pairs;
}

export const LIST = /^( *)(?:([-*])|(\d+)([.)])) (.*)$/;

/** The text as a Markdown file (速记小窗's 存为 .md): each line its own paragraph (list items stay together),
 * ++underline++ as <u>…</u>; **bold**, ==highlight==, lists, #tags and URLs as written. */
export function liteMarkdown(text: string): string {
  const lines = text.replace(/\s+$/, "").split("\n");
  const out: string[] = [];
  lines.forEach((l, i) => {
    const prev = lines[i - 1];
    if (i > 0 && l.trim() && prev.trim() && !(LIST.test(l) && LIST.test(prev))) out.push("");
    const li = LIST.exec(l);
    const head = li ? l.slice(0, l.length - li[5].length) : "";
    const body = l.slice(head.length);
    const marks = pairMarks(body, atoms(body));
    let line = head;
    let on = false;
    for (let j = 0; j < body.length; ) {
      if (marks.get(j) === "++") {
        line += on ? "</u>" : "<u>";
        on = !on;
        j += 2;
      } else line += body[j++];
    }
    out.push(line);
  });
  return `${out.join("\n")}\n`;
}

/** A file name from the text's first line (marks, list markers and #tags off), else `fallback`. */
export function liteFileName(text: string, fallback: string): string {
  const first = text.split("\n").find((l) => l.trim()) ?? "";
  const body = first.replace(LIST, "$5");
  const plain = tagSpans(body).reverse().reduce((s, t) => s.slice(0, t.start) + s.slice(t.end), body);
  const name = plain.replace(/\*\*|==|\+\+/g, "").replace(/[/\\:*?"<>|]/g, " ").replace(/\s+/g, " ").trim();
  return [...name].slice(0, 40).join("").trim() || fallback;
}
const MARK = "text-muted-foreground/45";
/** A format mark (** == ++, a link's brackets): shown only in the group holding the caret (index.css .md-mark, .fx). */
const FMT = `md-mark ${MARK}`;

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const span = (cls: string, text: string) => (cls ? `<span class="${cls}">${esc(text)}</span>` : `<span>${esc(text)}</span>`);

/** The list head's HTML: a `- ` / `* ` marker as a dot (见字如面's .bul), a number dimmed. */
function headHtml(head: string, li: RegExpExecArray): string {
  if (!li[2]) return span(MARK, head);
  return `${esc(li[1])}<span class="lv-bul">${esc(li[2])}</span>${esc(head.slice(li[1].length + 1))}`;
}

/** One line's inside: every character of it, styled like NoteBody's; each pair of marks and each link a group (`.fx`)
 * whose marks show while `ends` (the selection's ends in this line) touch it. */
function lineHtml(line: string, tags: boolean, ends: Ends = NO_ENDS): string {
  const li = LIST.exec(line);
  const head = li ? line.slice(0, line.length - li[5].length) : "";
  const at = head.length;
  const text = line.slice(at);
  const spans = atoms(text, tags);
  const marks = pairMarks(text, spans);
  // each opening mark's closing one (pairMarks pairs them in order)
  const closer = new Map<number, number>();
  const opened = new Map<Mark, number>();
  for (const [i, m] of [...marks].sort((x, y) => x[0] - y[0])) {
    const o = opened.get(m);
    if (o === undefined) opened.set(m, i);
    else (closer.set(o, i), opened.delete(m));
  }
  const groups: { mark: Mark; a: number; b: number }[] = [];
  const open = (g: { a: number; b: number }) => fxOpen(at + g.a, at + g.b, ends);
  const on: Record<Mark, boolean> = { "**": false, "==": false, "++": false };
  let out = li ? headHtml(head, li) : "";
  const classes = (cls = "") => [cls, on["**"] && "font-bold", on["=="] && "rounded-[3px] bg-[var(--status-doing-bg)] px-0.5 text-foreground", on["++"] && "underline decoration-1 underline-offset-[3px]"].filter(Boolean).join(" ");
  const styled = (s: string, cls = "") => span(classes(cls), s);
  // a #tag: its 「#」 on its own, so a tag with an icon shows the icon there (modules/notes/tagIcons.tsx)
  const tag = (seen: string, value: string) => `<span class="${classes("pm-hashtag")}" data-tags="notes" data-tag="${esc(value).replace(/"/g, "&quot;")}"><span class="pm-hash">#</span>${esc(seen.slice(1))}</span>`;
  let buf = "";
  const flush = () => {
    if (buf) out += styled(buf);
    buf = "";
  };
  for (let i = 0; i < text.length; ) {
    const mark = marks.get(i);
    if (mark) {
      flush();
      const end = closer.get(i);
      if (end !== undefined) {
        const g = { mark, a: i, b: end + 2 };
        out += open(g) + span(FMT, mark);
        groups.push(g);
        on[mark] = true;
      } else {
        // a pair crossing another (**a==b**c==): the ones opened inside close and open again around this end
        const k = groups.map((g) => g.mark).lastIndexOf(mark);
        const inner = groups.splice(k + 1);
        out += "</span>".repeat(inner.length) + span(FMT, mark) + "</span>";
        groups.pop();
        on[mark] = false;
        for (const g of inner) (out += open(g)), groups.push(g);
      }
      i += 2;
      continue;
    }
    const a = spans.find((s) => s.start === i);
    if (a) {
      flush();
      const seen = text.slice(a.start, a.end);
      out += a.kind === "link" ? open({ a: a.start, b: a.end }) + linkHtml(seen, styled) + "</span>" : a.kind === "icon" ? (iconHtml(a.value) ?? styled(seen)) : a.kind === "tag" ? tag(seen, a.value) : styled(seen, "text-primary underline underline-offset-2");
      i = a.end;
      continue;
    }
    buf += text[i];
    i++;
  }
  flush();
  return out;
}

/** A `[[…]]` being written: every character kept, `[[`, the module's name and `]]` dimmed, the name (or the words
 *  after `|`) in 柚子链接色 (links.css). */
function linkHtml(seen: string, styled: (s: string, cls?: string) => string): string {
  const f = foundIn(seen)[0];
  if (!f) return styled(seen);
  const [a, b, c] = f.inner;
  if (f.alias) return span(FMT, seen.slice(0, b + 1)) + styled(seen.slice(b + 1, c), "pm-lkw-edit") + span(FMT, seen.slice(c));
  const raw = seen.slice(a, b);
  const { module } = targetOf(f.target);
  const label = KINDS.find(([k]) => k === module)?.[1];
  const head = module !== "articles" && label && raw.startsWith(`${label}/`) ? label.length + 1 : 0;
  return span(FMT, seen.slice(0, a + head)) + styled(raw.slice(head), "pm-lk-edit") + span(FMT, seen.slice(b));
}

/** What the right-click list offers in this format. */
const FORMATS: Format[] = [
  { label: "加粗", keys: "⌘B", wrap: "**" },
  { label: "高亮", keys: "⌘⇧H", wrap: "==" },
  { label: "下划线", keys: "⌘U", wrap: "++" },
  { label: "列表", line: "- " },
  { label: "编号列表", line: "1. " },
];

export function liteFlavor(opts: { tags: boolean }): Flavor {
  return {
    caretAware: true,
    draw: (text, _pending, active, sel) => {
      let start = 0;
      return text.split("\n").map((l, i) => {
        const ends = i === active ? endsIn(sel, start, l.length) : NO_ENDS;
        start += l.length + 1;
        return `<div class="lv-ln">${l ? lineHtml(l, opts.tags, ends) : "<br>"}</div>`;
      });
    },
    onCaret: showGroups,
    line: (line) => {
      const li = LIST.exec(line);
      if (!li) return null;
      return { kind: "list", indent: li[1].length, empty: !li[5].trim(), next: li[1] + (li[2] ? `${li[2]} ` : `${Number(li[3]) + 1}${li[4]} `) };
    },
    wraps: [
      { key: "b", mark: "**" },
      { key: "u", mark: "++" },
      { key: "h", shift: true, mark: "==" },
    ],
    formats: FORMATS,
    linePrefix: /^ *(?:[-*]|\d+[.)]) /,
  };
}
