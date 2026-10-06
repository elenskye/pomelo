// One-off: a Flomo export (设置 → 账号详情 → 导出全部数据: `<name>的笔记.html` + `file/`) → Pomelo data
// file with the `notes` module (default prototype/notes-import.json, gitignored). Import it in the app:
// 设置 → 导入 (replaces only 念念不忘).
// Usage: pnpm import:flomo [export dir or .html] [output.json]   (default: newest prototype/flomo-*)
// Mapping: each <div class="memo"> → one note. Time (local) → createdAt (UTC ms). Content HTML →
// Markdown-lite: <p>/<br> → lines, <strong> → **, <mark> → ==, <u> → ++, <ul>/<ol> → `- ` / `1. `
// (2 spaces per level). Tags are already inline (#a/b). Images / audio are not imported (attachments
// come in v0.3; createdAt is unique, so they can be matched later) — listed at the end instead. A
// voice memo's transcript is appended when it isn't already the note's text. Flomo memo links
// (v.flomoapp.com/mine/?memo_id=…) stay as links: the export has no memo ids to resolve them.
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");
const output = process.argv[3] ?? resolve(root, "prototype/notes-import.json");

function findHtml(arg: string | undefined): string {
  let p = arg ? resolve(arg) : null;
  if (!p) {
    const dirs = readdirSync(resolve(root, "prototype"))
      .filter((d) => d.startsWith("flomo-") && statSync(resolve(root, "prototype", d)).isDirectory())
      .sort();
    if (!dirs.length) throw new Error("prototype/ 里没有 flomo-* 文件夹");
    p = resolve(root, "prototype", dirs[dirs.length - 1]);
  }
  if (statSync(p).isFile()) return p;
  const html = readdirSync(p).filter((f) => f.endsWith(".html"));
  if (html.length !== 1) throw new Error(`${p} 里应该正好有一个 .html 文件`);
  return resolve(p, html[0]);
}

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " " };
const decode = (s: string) =>
  s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e: string) =>
    e[0] === "#" ? String.fromCodePoint(e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)) : (ENTITIES[e.toLowerCase()] ?? m),
  );

const MARKS: Record<string, string> = { strong: "**", b: "**", mark: "==", u: "++" };

/** Flomo content HTML → Markdown-lite lines. Inline marks left open at a line break are closed
 * there and reopened on the next line; spaces just inside a mark move outside it. */
function toMarkdown(html: string): string {
  const lines: string[] = [];
  let cur: string | null = null; // the line being built (marks included, indent not)
  let indent = "";
  let fresh: string | null = null; // a mark just opened, nothing written after it yet
  const lists: { ordered: boolean; n: number }[] = [];
  let itemStarted = true; // inside an <li>: has its marker been written?
  const open: string[] = [];

  const flush = () => {
    if (cur === null) return;
    let s = cur + [...open].reverse().join("");
    for (const m of new Set(Object.values(MARKS))) s = s.split(m + m).join(""); // empty marks
    lines.push(s.replace(/[*=+]/g, "").trim() ? indent + s.trim() : "");
    cur = null;
    fresh = null;
  };
  const startLine = () => {
    flush();
    const depth = lists.length;
    if (depth === 0) indent = "";
    else if (!itemStarted) {
      const l = lists[depth - 1];
      indent = "  ".repeat(depth - 1) + (l.ordered ? `${++l.n}. ` : "- ");
      itemStarted = true;
    } else indent = "  ".repeat(depth); // continuation inside the item
    cur = open.join("");
  };
  const text = (t: string) => {
    if (cur === null) {
      if (!t.trim()) return;
      startLine();
    }
    if (fresh && /^\s/.test(t)) {
      const ws = /^\s+/.exec(t)![0];
      cur = cur!.slice(0, -fresh.length) + ws + fresh;
      t = t.slice(ws.length);
    }
    if (t) fresh = null;
    cur += t;
  };
  const closeMark = (m: string) => {
    const i = open.lastIndexOf(m);
    if (i < 0) return;
    open.splice(i, 1);
    if (cur === null) return;
    if (cur.endsWith(m) && fresh === m) {
      cur = cur.slice(0, -m.length); // nothing inside
    } else {
      const trail = /\s*$/.exec(cur)![0];
      cur = cur.slice(0, cur.length - trail.length) + m + trail;
    }
    fresh = null;
  };

  for (const tok of html.split(/(<[^>]+>)/)) {
    if (!tok) continue;
    const tag = /^<(\/?)([a-z0-9]+)/i.exec(tok);
    if (!tag) {
      text(decode(tok.replace(/\n/g, "")));
      continue;
    }
    const close = tag[1] === "/";
    const name = tag[2].toLowerCase();
    if (name in MARKS) {
      const m = MARKS[name];
      if (close) closeMark(m);
      else {
        if (cur === null) startLine();
        cur += m;
        open.push(m);
        fresh = m;
      }
    } else if (name === "p") {
      if (close) flush();
      else startLine();
    } else if (name === "br") {
      startLine();
    } else if (name === "ul" || name === "ol") {
      flush();
      if (close) lists.pop();
      else lists.push({ ordered: name === "ol", n: Number(/start="(\d+)"/.exec(tok)?.[1] ?? 1) - 1 });
      itemStarted = true; // text right after a list continues the enclosing item
    } else if (name === "li") {
      flush();
      itemStarted = close;
    }
  }
  flush();
  // blank lines: none at the ends, at most one in a row
  const out: string[] = [];
  for (const l of lines) if (l || (out.length && out[out.length - 1])) out.push(l);
  while (out.length && !out[out.length - 1]) out.pop();
  return out.join("\n");
}

/** "2026-02-23 14:39:11" (local) → "2026-02-23T06:39:11.000Z". */
const toStamp = (t: string) => {
  const d = new Date(t.trim().replace(" ", "T"));
  if (Number.isNaN(d.getTime())) throw new Error(`时间格式不对：${t}`);
  return d.toISOString();
};

const plain = (s: string) => s.replace(/[\s*=+#/\-，。、！？,.!?：:；;“”"'（）()]/g, "");

const htmlPath = findHtml(process.argv[2]);
const src = readFileSync(htmlPath, "utf8");
// Split on memo starts: content holds no <div>, files (images, audio + transcript) run to the next memo.
const chunks = src.split('<div class="memo">').slice(1);
const memos = chunks.map((c, i) => {
  const time = /<div class="time">([^<]*)<\/div>/.exec(c)?.[1];
  const body = /<div class="content">([\s\S]*?)<\/div>\s*<div class="files">([\s\S]*)$/.exec(c);
  if (!time || !body) throw new Error(`第 ${i + 1} 条认不出来，导出格式可能变了`);
  return [c, time, body[1], body[2]] as const;
});

const withFiles: string[] = [];
let transcripts = 0;
const notes = memos.map(([, time, content, files]) => {
  let md = toMarkdown(content);
  const imgs = (files.match(/<img /g) ?? []).length;
  const audio = /<audio /.test(files);
  const transcript = /<div class="audio-player__content">([\s\S]*?)<\/div>/.exec(files)?.[1];
  if (transcript && decode(transcript).trim()) {
    const t = decode(transcript).trim();
    // Flomo keeps the raw transcript next to the (usually edited) text; append it only if it adds something
    const a = plain(md);
    const b = plain(t);
    const shared = [...b].filter((ch) => a.includes(ch)).length / Math.max(1, b.length);
    if (!a.includes(b) && shared < 0.9) {
      md += `\n\n🎙️ ${t}`;
      transcripts++;
    }
  }
  if (!md.trim()) md = "（空白笔记）";
  if (imgs || audio) withFiles.push(`  ${time.trim()}  ${[imgs && `${imgs} 张图片`, audio && "1 段录音"].filter(Boolean).join("、")}`);
  return { content: md, createdAt: toStamp(time) };
});
notes.sort((a, b) => a.createdAt.localeCompare(b.createdAt));
const dup = notes.find((n, i) => i > 0 && n.createdAt === notes[i - 1].createdAt);
if (dup) throw new Error(`有两条笔记的时间相同：${dup.createdAt}`);

writeFileSync(
  output,
  JSON.stringify({ format: "pomelo-export", formatVersion: 7, exportedAt: new Date().toISOString(), source: htmlPath.slice(root.length + 1), modules: { notes: { notes } } }, null, 2) + "\n",
);

const tags = new Set(
  notes.flatMap((n) =>
    [...n.content.matchAll(/(?:^|\s)#([^\s#]+)/g)].flatMap((m) => {
      const segs = m[1].replace(/\/+$/, "").split("/");
      return segs.map((_, i) => segs.slice(0, i + 1).join("/")); // with parents, as the app counts
    }),
  ),
);
const flomoLinks = notes.reduce((k, n) => k + (n.content.match(/flomoapp\.com\/mine\/\?memo_id=/g) ?? []).length, 0);
console.log(`${notes.length} 条笔记、${tags.size} 个标签 → ${output}`);
console.log(`时间：${notes[0]?.createdAt.slice(0, 10)} ～ ${notes[notes.length - 1]?.createdAt.slice(0, 10)}`);
if (transcripts) console.log(`${transcripts} 条录音的文字稿接在了笔记末尾（🎙️）`);
if (flomoLinks) console.log(`${flomoLinks} 处 Flomo 笔记链接原样保留`);
if (withFiles.length) console.log(`${withFiles.length} 条带附件，附件这次没导入（v0.3 再补）：\n${withFiles.join("\n")}`);
