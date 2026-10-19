// Renders a note's Markdown-lite text: lines = paragraphs, `- ` / `1. ` lists (2 spaces per level),
// **bold**, ==highlight==, ++underline++, #tags (clickable) and bare URLs (open via Rust).
// The tag rule mirrors notes.rs — keep them in step.
import type { ReactNode } from "react";
import { api } from "@/shared/ipc";

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

/** One-line preview, as notes.rs makes it: no tags, marks or list bullets. */
export function excerpt(content: string, max = 60): string {
  const lines = content.split("\n").map((l) => {
    let out = "";
    let last = 0;
    for (const t of tagSpans(l)) {
      out += l.slice(last, t.start);
      last = t.end;
    }
    return (out + l.slice(last)).replace(/\*\*|==|\+\+/g, "").replace(/^\s*(?:[-*]|\d+[.)]) /, "");
  });
  let s = lines.join(" ").split(/\s+/).filter(Boolean).join(" ");
  if (!s) s = [...new Set(content.split("\n").flatMap((l) => tagSpans(l).map((t) => `#${t.value}`)))].join(" ");
  return [...s].length > max ? [...s].slice(0, max).join("") + "…" : s;
}

const URL_RE = /(?:https?:\/\/|www\.)[^\s<>"'，。；！？、）】」』]+/giu;

export function atoms(line: string): Span[] {
  const spans = tagSpans(line);
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

function Inline(props: { line: string; onTag?: (tag: string) => void }) {
  const { line } = props;
  const spans = atoms(line);
  const marks = pairMarks(line, spans);
  const out: ReactNode[] = [];
  const on = { "**": false, "==": false, "++": false };
  let buf = "";
  const style = (node: ReactNode, key: number) => {
    let n = node;
    if (on["++"]) n = <u className="decoration-1 underline-offset-[3px]">{n}</u>;
    if (on["=="]) n = <mark className="rounded-[3px] bg-[var(--status-doing-bg)] px-0.5 text-foreground">{n}</mark>;
    if (on["**"]) n = <strong className="font-bold">{n}</strong>;
    return <span key={key}>{n}</span>;
  };
  const flush = () => {
    if (buf) out.push(style(buf, out.length));
    buf = "";
  };
  for (let i = 0; i < line.length; ) {
    const mark = marks.get(i);
    if (mark) {
      flush();
      on[mark] = !on[mark];
      i += 2;
      continue;
    }
    const a = spans.find((s) => s.start === i);
    if (a) {
      flush();
      const text = line.slice(a.start, a.end);
      out.push(
        style(
          a.kind === "tag" ? (
            <button type="button" onClick={() => props.onTag?.(a.value)} className="text-primary hover:underline">
              {text}
            </button>
          ) : (
            <button type="button" onClick={() => void api.openLink(a.value).catch(() => {})} className="text-left break-all text-primary underline underline-offset-2">
              {text}
            </button>
          ),
          out.length,
        ),
      );
      i = a.end;
      continue;
    }
    buf += line[i];
    i++;
  }
  flush();
  return <>{out}</>;
}

const LIST = /^( *)(?:([-*])|(\d+)[.)]) (.*)$/;
const BULLETS = ["•", "◦", "▪"];

/** Each line is a block: paragraphs sit 0.75em apart (clearly more than the line gap), list items in a
 * row 0.25rem; a blank line adds a little more room. */
export function NoteBody(props: { content: string; onTag?: (tag: string) => void }) {
  const lines = props.content.split("\n");
  const gap = (i: number, isItem: boolean) => (i === 0 || !lines[i - 1].trim() ? "" : isItem && LIST.test(lines[i - 1]) ? "mt-1" : "mt-[0.75em]");
  return (
    <div className="selectable leading-relaxed break-words">
      {lines.map((line, i) => {
        if (!line.trim()) return i === 0 ? null : <div key={i} className="h-[0.9em]" />;
        const li = LIST.exec(line);
        if (li) {
          const level = Math.floor(li[1].length / 2);
          return (
            <div key={i} className={`flex ${gap(i, true)}`} style={{ paddingLeft: `${level * 1.25}em` }}>
              <span className="w-[1.25em] shrink-0 text-muted-foreground tabular-nums">{li[3] ? `${li[3]}.` : BULLETS[Math.min(level, 2)]}</span>
              <div className="min-w-0 flex-1">
                <Inline line={li[4]} onTag={props.onTag} />
              </div>
            </div>
          );
        }
        const indent = /^ */.exec(line)![0].length;
        return (
          <p key={i} className={gap(i, false)} style={indent ? { paddingLeft: `${Math.floor(indent / 2) * 1.25}em` } : undefined}>
            <Inline line={line.trimStart()} onTag={props.onTag} />
          </p>
        );
      })}
    </div>
  );
}
