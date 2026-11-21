// A note set into an article (`![[念念不忘/…]]`, bear.ts) as HTML: a small white card — 念念不忘's sticker and the time,
// 「去念念不忘看这一条」 at the right, then the note's whole text the way its card reads (bold, highlight, underline,
// lists, #tags, links as names). Gone: a dashed box 「这条笔记不在了」.
import { markSvg } from "@/shared/marks";
import { atoms, LIST, pairMarks } from "@/shared/editor/lite";
import { esc } from "@/shared/editor/markdown";
import { foundIn, targetOf, type Thing } from "./links";

const WEEK = "日一二三四五六";

/** `11 月 2 日 · 周一 21:14` of a local `YYYY-MM-DD HH:MM`. */
export function noteWhen(d: string): string {
  const day = new Date(`${d.slice(0, 10)}T00:00:00`);
  const time = d.slice(11, 16);
  return `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8, 10))} 日 · 周${WEEK[day.getDay()]}${time ? ` ${time}` : ""}`;
}

function lineHtml(line: string, sticker: (module: string) => string): string {
  const spans = atoms(line);
  const marks = pairMarks(line, spans);
  const on = { "**": false, "==": false, "++": false };
  let out = "";
  let buf = "";
  const wrap = (h: string) => {
    let x = h;
    if (on["++"]) x = `<u>${x}</u>`;
    if (on["=="]) x = `<mark>${x}</mark>`;
    if (on["**"]) x = `<b>${x}</b>`;
    return x;
  };
  const flush = () => {
    if (buf) out += wrap(esc(buf));
    buf = "";
  };
  for (let i = 0; i < line.length; ) {
    const m = marks.get(i);
    if (m) {
      flush();
      on[m] = !on[m];
      i += 2;
      continue;
    }
    const a = spans.find((s) => s.start === i);
    if (a) {
      flush();
      const seen = line.slice(a.start, a.end);
      if (a.kind === "tag") out += wrap(`<span class="pm-hashtag">${esc(seen)}</span>`);
      else if (a.kind === "link") {
        const f = foundIn(seen)[0];
        const { module, name } = targetOf(f?.target ?? seen);
        out += wrap(f?.alias ? `<span class="pm-lkw">${esc(f.alias)}</span>` : `<span class="pm-lk"><span class="pm-lk-mk">${sticker(module)}</span>${esc(name)}</span>`);
      } else out += wrap(esc(seen));
      i = a.end;
      continue;
    }
    buf += line[i];
    i++;
  }
  flush();
  return out;
}

/** The card for a set-in note (`t` undefined: not looked up yet; null: gone). */
export function noteCardHtml(t: Thing | null | undefined, sticker: (module: string) => string): string {
  if (t === null) return `<div class="emb-card gone">这条笔记不在了</div>`;
  if (!t) return `<div class="emb-card wait"><div class="emb-h">${sticker("notes")}<span>…</span></div></div>`;
  const open = (markSvg("icons/external-link") ?? "").replace(/^<svg[^>]*>/, '<svg viewBox="0 0 24 24" width="15" height="15" fill="none" aria-hidden="true">');
  const body = (t.text ?? "")
    .split("\n")
    .map((l) => {
      if (!l.trim()) return `<div class="emb-gap"></div>`;
      const li = LIST.exec(l);
      return li ? `<div class="emb-li">${li[3] ? `${li[3]}.` : "•"} ${lineHtml(li[5], sticker)}</div>` : `<div>${lineHtml(l.trimStart(), sticker)}</div>`;
    })
    .join("");
  return `<div class="emb-card"><div class="emb-h">${sticker("notes")}<span>${esc(noteWhen(t.date ?? ""))}</span><span class="emb-op" data-open-note="${t.id}" title="去念念不忘看这一条">${open}</span></div><div class="emb-t">${body}</div></div>`;
}
