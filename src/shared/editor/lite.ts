// Markdown-lite, 念念不忘's format (also the 速记小窗's): lines = paragraphs, `- ` / `1. ` lists (2 spaces
// per level), **bold**, ==highlight==, ++underline++, bare URLs and (optionally) #tags. The tag rule
// mirrors notes.rs — keep them in step. `liteFlavor` draws it in LiveText the way NoteBody shows it
// (paragraphs 0.75em apart, list items 4px, a blank line 0.9em), marks kept and dimmed.
import type { Flavor } from "./LiveText";

const TAG_MAX = 50;
const validTag = (t: string) => t.length > 0 && !t.startsWith("/") && !t.includes("//") && [...t].length <= TAG_MAX;

export type Span = { start: number; end: number; kind: "tag" | "url"; value: string };

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

/** Tags (unless `tags` is false) and URLs in a line, in order. */
export function atoms(line: string, tags = true): Span[] {
  const spans = tags ? tagSpans(line) : [];
  for (const m of line.matchAll(URL_RE)) {
    const url = m[0].replace(/[.,;:!?)\]]+$/, "");
    const s = { start: m.index, end: m.index + url.length, kind: "url" as const, value: url };
    if (!spans.some((t) => t.start < s.end && s.start < t.end)) spans.push(s);
  }
  return spans.sort((a, b) => a.start - b.start);
}

export type Mark = "**" | "==" | "++";
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

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const span = (cls: string, text: string) => (cls ? `<span class="${cls}">${esc(text)}</span>` : `<span>${esc(text)}</span>`);

/** One line's inside: every character of it, marks dimmed, styled like NoteBody's. */
function lineHtml(line: string, tags: boolean): string {
  const li = LIST.exec(line);
  const head = li ? line.slice(0, line.length - li[5].length) : "";
  const text = line.slice(head.length);
  const spans = atoms(text, tags);
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

/** NoteBody's gaps: a blank line is a 0.9em spacer; a line after text sits 0.75em below it, a list item
 * after a list item 4px. The pending line counts as text. */
function gap(lines: string[], i: number, pending: number | null): string {
  const blank = (j: number) => j !== pending && !lines[j].trim();
  if (blank(i)) return lines.length > 1 ? "h-[0.9em] leading-[0.9em]" : "";
  if (i === 0 || blank(i - 1)) return "";
  return LIST.test(lines[i]) && LIST.test(lines[i - 1]) ? "mt-1" : "mt-[0.75em]";
}

export function liteFlavor(opts: { tags: boolean }): Flavor {
  return {
    draw: (text, pending) => {
      const lines = text.split("\n");
      return lines.map((l, i) => `<div class="${gap(lines, i, pending)}">${l ? lineHtml(l, opts.tags) : "<br>"}</div>`).join("");
    },
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
  };
}
