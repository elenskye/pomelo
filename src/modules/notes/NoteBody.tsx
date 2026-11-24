// Renders a note's Markdown-lite text (shared/editor/lite.ts): lines = paragraphs, `- ` / `1. ` lists
// (2 spaces per level), **bold**, ==highlight==, ++underline++, #tags (clickable), bare URLs (open via Rust),
// `:pm-<key>:` icons from the icon library and 柚子链接 (`[[目标]]` a chip, `[[目标|字]]` a Pomelo link: shared/links).
import type { ReactNode } from "react";
import { api } from "@/shared/ipc";
import { withoutIcons } from "@/shared/editor/inlineIcon";
import { atoms, LIST, pairMarks, tagSpans } from "@/shared/editor/lite";
import { LibIcon, libraryIcon } from "@/shared/iconLibrary";
import { foundIn, shown, type Thing } from "@/shared/links/links";
import { LinkChip, useLinkThings } from "@/shared/links/LinkParts";

/** One-line preview, as notes.rs makes it: no tags, marks, icons or list bullets. */
export function excerpt(content: string): string {
  const lines = shown(content).split("\n").map((l) => {
    let out = "";
    let last = 0;
    for (const t of tagSpans(l)) {
      out += l.slice(last, t.start);
      last = t.end;
    }
    return withoutIcons(out + l.slice(last)).replace(/\*\*|==|\+\+/g, "").replace(/^\s*(?:[-*]|\d+[.)]) /, "");
  });
  let s = lines.join(" ").split(/\s+/).filter(Boolean).join(" ");
  if (!s) s = [...new Set(content.split("\n").flatMap((l) => tagSpans(l).map((t) => `#${t.value}`)))].join(" ");
  return [...s].length > 60 ? [...s].slice(0, 60).join("") + "…" : s;
}

function Inline(props: { line: string; onTag?: (tag: string) => void; thing: (target: string) => Thing | null | undefined }) {
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
          a.kind === "link" ? (
            (() => {
              const f = foundIn(text)[0];
              return f ? <LinkChip target={f.target} alias={f.alias} thing={props.thing(f.target)} /> : text;
            })()
          ) : a.kind === "icon" ? (
            libraryIcon(a.value) ? <LibIcon name={a.value} size={18} className="inline-block align-[-0.2em]" /> : text
          ) : a.kind === "tag" ? (
            <button type="button" onClick={() => props.onTag?.(a.value)} className="pm-hashtag hover:underline" data-tags="notes" data-tag={a.value}>
              <span className="pm-hash">#</span>
              {text.slice(1)}
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

/** Each line is a block, laid out as the box writing it (index.css .lv-ln). */
export function NoteBody(props: { content: string; onTag?: (tag: string) => void }) {
  const lines = props.content.split("\n");
  const thing = useLinkThings(props.content);
  return (
    <div className="selectable leading-relaxed break-words">
      {lines.map((line, i) => {
        // as the box writing it (lite.ts, index.css .lv-ln): every line a whole line, a blank one too, a small gap after each
        if (!line.trim()) return <div key={i} className="lv-ln"><br /></div>;
        const li = LIST.exec(line);
        if (li) {
          const level = Math.floor(li[1].length / 2);
          return (
            <div key={i} className="lv-ln flex" style={{ paddingLeft: `${level * 1.25}em` }}>
              <span className="w-[1.25em] shrink-0 text-muted-foreground tabular-nums">{li[3] ? `${li[3]}.` : BULLETS[Math.min(level, 2)]}</span>
              <div className="min-w-0 flex-1">
                <Inline line={li[5]} onTag={props.onTag} thing={thing} />
              </div>
            </div>
          );
        }
        const indent = /^ */.exec(line)![0].length;
        return (
          <p key={i} className="lv-ln" style={indent ? { paddingLeft: `${Math.floor(indent / 2) * 1.25}em` } : undefined}>
            <Inline line={line.trimStart()} onTag={props.onTag} thing={thing} />
          </p>
        );
      })}
    </div>
  );
}
