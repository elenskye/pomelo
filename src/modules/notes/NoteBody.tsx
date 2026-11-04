// Renders a note's Markdown-lite text (shared/editor/lite.ts): lines = paragraphs, `- ` / `1. ` lists
// (2 spaces per level), **bold**, ==highlight==, ++underline++, #tags (clickable) and bare URLs (open via Rust).
import type { ReactNode } from "react";
import { api } from "@/shared/ipc";
import { atoms, LIST, pairMarks, tagSpans } from "@/shared/editor/lite";

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
                <Inline line={li[5]} onTag={props.onTag} />
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
