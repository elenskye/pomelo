// 见字如面's PDF pages and Word blocks (the print page; print.rs). An article is drawn by the writing box's own renderer
// (bear.ts drawDoc: no caret, nothing folded, every mark hidden, the tags left out), the title, 标签和日期 and 目录 on
// top of the first page, then cut into A4 pages (1 CSS px = 1 pt) line by line: a heading never ends a page, a gap
// never starts one, a picture / formula / diagram stays whole (scaled to fit the page). Two looks (print.css):
// 柚儿园 and 印刷. Word gets the same lines as blocks and runs (docx.rs); formulas and diagrams as pictures.
import { CALLOUTS, LIST, blocks, drawDoc, fencedLines, type Look } from "@/modules/articles/bear";
import { diagramReady, renderCode, renderMath } from "@/modules/articles/render";
import { fileSrc, type FileInfo } from "@/shared/attachments/api";
import { esc } from "@/shared/editor/markdown";
import { moduleMark, sizedMark } from "@/shared/marks";

type Style = "pomelo" | "print";
export type Options = { format: string; style: Style; toc: boolean; header: boolean; meta: boolean; dest?: string };
export type Item = { id: number; title: string; content: string; tags: string[]; createdAt: string; updatedAt: string; files: FileInfo[] };
type PdfLink = { page: number; x: number; y: number; w: number; h: number; toPage?: number; toY?: number; url?: string };
type OutlineMark = { level: number; title: string; page: number; y: number };
type Laid = { pages: HTMLElement[]; links: PdfLink[]; outline: OutlineMark[] };

const PAGE = { w: 595, h: 842 };
const BOX: Record<Style, { top: number; right: number; bottom: number; left: number }> = {
  pomelo: { top: 64, right: 62, bottom: 64, left: 66 },
  print: { top: 76, right: 72, bottom: 76, left: 76 },
};
const inner = (s: Style) => ({ w: PAGE.w - BOX[s].left - BOX[s].right, h: PAGE.h - BOX[s].top - BOX[s].bottom });

const el = (tag: string, cls: string, html = "") => {
  const e = document.createElement(tag);
  e.className = cls;
  e.innerHTML = html;
  return e;
};

/** 2026 年 11 月 6 日 · 周四 */
export function longDate(stamp: string) {
  const d = new Date(stamp);
  return `${d.getFullYear()} 年 ${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${"日一二三四五六"[d.getDay()]}`;
}

// ---------- the text ----------

const TAG = /(^|\s)#[^\s#]+/g;
/** The title's line (articles.rs `title_line`): the first line outside code with words of its own. */
function titleIndex(lines: string[]): number {
  const fenced = fencedLines(blocks(lines));
  for (let i = 0; i < lines.length; i++) {
    if (fenced.has(i)) continue;
    const t = lines[i]
      .replace(/<!--.*?-->\s*$/, "")
      .replace(/^\s*(>\s*)*/, "")
      .replace(/^#{1,6}\s+/, "")
      .replace(/^([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?/, "")
      .replace(TAG, " ")
      .replace(/[*_~=`]/g, "")
      .trim();
    if (t) return i;
  }
  return -1;
}

/** Each article's diagrams drawn first (light), then read by drawDoc. */
async function lookOf(item: Item, text: string): Promise<Look> {
  const lines = text.split("\n");
  const diagrams = new Map<string, { svg: string } | { error: string }>();
  for (const b of blocks(lines).filter((b) => b.kind === "fence" && b.lang === "mermaid")) {
    const src = lines.slice(b.from + 1, b.to).join("\n");
    diagrams.set(src, await diagramReady(src, false));
  }
  const byName = new Map(item.files.map((f) => [f.name, f]));
  return {
    image: (name) => {
      const f = byName.get(name);
      return f && f.kind === "image" && !f.missing ? fileSrc(f.id) : null;
    },
    file: (name) => byName.has(name),
    thing: () => undefined,
    sticker: () => "",
    math: renderMath,
    mermaid: (src) => diagrams.get(src) ?? { error: "画不出来" },
    code: renderCode,
    folds: new Set(),
  };
}

const VISIBLE_PARTS = "img, svg, .box, .formula, .diagram, .katex, .co-head";
/** The article's lines (without its title line and its tags) in `flow`, each `data-i` its line in the text. */
async function drawLines(item: Item, flow: HTMLElement) {
  const lines = item.content.split("\n");
  const t = titleIndex(lines);
  const text = lines.map((l, i) => (i === t ? "" : l)).join("\n");
  const look = await lookOf(item, text);
  flow.innerHTML = drawDoc(text, null, null, look)
    .map((h, i) => h.replace(/^<div class="ln/, `<div data-i="${i}" class="ln`))
    .join("");
  // the tags, and the source lines a diagram / formula / table's rule stands for, go
  for (const x of flow.querySelectorAll(".pm-hashtag, :scope > .srcline, :scope > .tsep, :scope > .cohid")) x.remove();
  // lines left empty (the title's, tags only) go; gaps don't repeat, nor start
  let gap = true;
  for (const ln of [...flow.children] as HTMLElement[]) {
    const kind = ln.classList;
    if (kind.contains("gap") || (kind.contains("p") && !ln.textContent?.replace(/\s/g, "") && !ln.querySelector(VISIBLE_PARTS))) {
      if (gap || !kind.contains("gap")) ln.remove();
      else gap = true;
      continue;
    }
    if (kind.contains("hid")) kind.remove("hid");
    gap = false;
  }
  // tables: the columns shared out over the width (the box's widths, scaled)
  let row: HTMLElement[] = [];
  const flush = () => {
    if (!row.length) return;
    const head = row[0].querySelectorAll<HTMLElement>(".td");
    const ws = [...head].map((td) => parseFloat(td.style.minWidth) || 3);
    const sum = ws.reduce((a, b) => a + b, 0) || 1;
    for (const r of row) r.querySelectorAll<HTMLElement>(".td").forEach((td, c) => ((td.style.minWidth = "0"), (td.style.width = `${((ws[c] ?? 3) / sum) * 100}%`)));
    row = [];
  };
  for (const ln of flow.children) {
    if (ln.classList.contains("tr")) row.push(ln as HTMLElement);
    else if (!ln.classList.contains("tsep")) flush();
  }
  flush();
  await Promise.all([...flow.querySelectorAll("img")].map((i) => i.decode().catch(() => {})));
  return lines;
}

// ---------- the top of the first page ----------

function headOf(item: Item, o: Options): HTMLElement {
  const box = el("div", "pv-top");
  const marker = o.style === "pomelo" ? `<span class="pv-tm" data-hand="marker" data-seed="61"></span>` : "";
  box.append(el("h1", "pv-title", `${marker}${esc(item.title || "无标题")}`));
  if (o.meta) {
    const tags = item.tags.map((t) => (o.style === "pomelo" ? `<span class="pv-tag">#${esc(t)}</span>` : `#${esc(t)}`));
    box.append(el("div", "pv-meta", [esc(longDate(item.createdAt)), ...tags].join(o.style === "pomelo" ? " " : " · ")));
  }
  return box;
}

function tocOf(heads: HTMLElement[]): HTMLElement | null {
  if (!heads.length) return null;
  const top = Math.min(...heads.map(level));
  const box = el("div", "pv-toc", `<div class="pv-toct">目录</div>`);
  for (const h of heads) {
    const row = el("div", `pv-tocl${level(h) > top ? " sub" : ""}`, `<span class="t"></span><i></i><span class="n"></span>`);
    row.style.paddingLeft = `${(level(h) - top) * 14}px`;
    row.querySelector(".t")!.textContent = h.innerText.trim();
    box.append(row);
  }
  return box;
}

const level = (h: HTMLElement) => Number(/\bh([1-6])\b/.exec(h.className)?.[1] ?? 1);

// ---------- pages ----------

/** Lays the article out as pages in `host` (all shown, one under another), painted; their links and outline. */
export async function layout(item: Item, o: Options, host: HTMLElement): Promise<Laid> {
  const size = inner(o.style);
  host.innerHTML = "";
  host.className = `pv-host pv-${o.style}`;
  const measure = el("div", "pv-measure");
  measure.style.width = `${size.w}px`;
  const top = headOf(item, o);
  const flow = el("div", "ar-text pv-text");
  measure.append(top, flow);
  host.append(measure);
  await drawLines(item, flow);
  const heads = [...flow.querySelectorAll<HTMLElement>(":scope > .ln.h")].filter((h) => h.innerText.trim());
  const toc = o.toc ? tocOf(heads) : null;
  if (toc) top.append(toc);
  await document.fonts.ready;

  // cut by line: the space each takes is the distance to the next one
  const lines = [...flow.children] as HTMLElement[];
  const tops = lines.map((l) => l.offsetTop);
  const end = flow.scrollHeight;
  const tall = (i: number) => (i + 1 < lines.length ? tops[i + 1] : end) - tops[i];
  const headH = top.offsetHeight + 10;
  const pages: HTMLElement[][] = [[]];
  let used = 0;
  let room = size.h - headH - 2;
  for (let i = 0; i < lines.length; i++) {
    const ln = lines[i];
    const cur = pages[pages.length - 1];
    const gap = ln.classList.contains("gap");
    if (gap && cur.length === 0) continue;
    let need = tall(i);
    if (ln.classList.contains("h")) {
      let j = i + 1;
      while (j < lines.length && lines[j].classList.contains("gap")) j++;
      if (j < lines.length) need += Math.min(tall(j), size.h / 3);
    }
    if (used + need > room && cur.length > 0) {
      pages.push([]);
      used = 0;
      room = size.h - 2;
      if (gap) continue;
    }
    pages[pages.length - 1].push(ln);
    used += tall(i);
  }
  if (pages.length > 1 && pages[pages.length - 1].length === 0) pages.pop();

  // 柚儿园's grid: the palette's grid colour, a 16 pt square tiled
  const grid = getComputedStyle(host).getPropertyValue("--color-grid").trim() || "#E2E8F5";
  const tile = `url("data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><path d="M0 .5H16M.5 0V16" stroke="${grid}" stroke-opacity=".6" fill="none"/></svg>`)}")`;
  const made: HTMLElement[] = [];
  pages.forEach((lns, n) => {
    const pg = el("div", `pv-page pv-${o.style}`);
    if (o.style === "pomelo") {
      pg.style.backgroundImage = tile;
      // the paper's grain, as the app's papers have it (the desk preset: a tiled picture under everything)
      pg.dataset.hand = "desk";
    }
    if (o.header && n > 0) {
      const h = el("div", "pv-head", `<span class="t"></span><span>见字如面</span>`);
      h.querySelector(".t")!.textContent = item.title || "无标题";
      pg.append(h);
    }
    const box = el("div", "pv-in");
    if (n === 0) {
      box.append(top);
      if (o.style === "pomelo") {
        const svg = moduleMark("articles");
        if (svg) pg.append(el("span", "pv-slap", sizedMark(svg, 46, `pv${item.id}`)));
      }
    }
    const text = el("div", "ar-text pv-text");
    text.append(...lns);
    box.append(text);
    pg.append(box);
    if (o.header) pg.append(el("div", "pv-foot", o.style === "pomelo" ? `<span class="pv-num" data-hand="frame" data-r="half" data-sw="1.3" data-seed="${70 + n}">${n + 1}</span>` : `— ${n + 1} —`));
    made.push(pg);
  });
  measure.remove();
  host.append(...made);
  window.Pomelo?.paint(host);

  // where things are, page by page
  const pageOf = (e: HTMLElement) => made.findIndex((p) => p.contains(e));
  const at = (e: Element, page: HTMLElement) => {
    const r = e.getBoundingClientRect();
    const p = page.getBoundingClientRect();
    return { x: r.left - p.left, y: r.top - p.top, w: r.width, h: r.height };
  };
  const links: PdfLink[] = [];
  const outline: OutlineMark[] = [];
  const topLevel = heads.length ? Math.min(...heads.map(level)) : 1;
  heads.forEach((h, k) => {
    const p = pageOf(h);
    if (p < 0) return;
    const y = Math.max(0, at(h, made[p]).y - 4);
    outline.push({ level: level(h) - topLevel + 1, title: h.innerText.trim(), page: p, y });
    const row = toc?.children[k + 1];
    if (toc && row) {
      row.querySelector(".n")!.textContent = String(p + 1);
      links.push({ page: 0, ...at(row, made[0]), toPage: p, toY: y });
    }
  });
  made.forEach((pg, p) => {
    for (const a of pg.querySelectorAll<HTMLElement>(".pv-text [data-href]")) {
      const href = a.dataset.href!;
      const url = /^www\./i.test(href) ? `https://${href}` : href;
      if (!/^(https?:|mailto:)/i.test(url)) continue;
      const base = pg.getBoundingClientRect();
      for (const r of a.getClientRects()) links.push({ page: p, x: r.left - base.left, y: r.top - base.top, w: r.width, h: r.height, url });
    }
  });
  return { pages: made, links, outline };
}

// ---------- Word ----------

type Run = { t?: string; b?: boolean; i?: boolean; u?: boolean; s?: boolean; code?: boolean; sup?: boolean; hl?: string; href?: string; pic?: number; file?: number; w?: number; h?: number };
type Block =
  | { k: "h"; level: number; runs: Run[] }
  | { k: "p"; runs: Run[] }
  | { k: "li"; level: number; mark: string; done: boolean; runs: Run[] }
  | { k: "quote"; tint?: string; label?: string; runs: Run[] }
  | { k: "code"; lines: string[] }
  | { k: "table"; rows: Run[][][] }
  | { k: "hr" }
  | { k: "pic"; pic?: number; file?: number; w: number; h: number }
  | { k: "note"; runs: Run[] };
/** Draws a box as a picture (print_picture): its number. */
export type Snap = (e: HTMLElement) => Promise<{ pic: number; w: number; h: number } | null>;

const SKIP = ".mk, .hg, .fold, .folded, .msrc, .srchid, .ind, .bul, .bul-hide, .num, .task, .co-head, .pv-err, .pv-wait";

async function runsOf(root: Element, item: Item, snap: Snap): Promise<Run[]> {
  const out: Run[] = [];
  const fileOf = (src: string) => item.files.find((f) => fileSrc(f.id) === src)?.id;
  const walk = async (n: Node, st: Run) => {
    if (n.nodeType === Node.TEXT_NODE) {
      const t = n.textContent ?? "";
      if (t) out.push({ ...st, t });
      return;
    }
    if (!(n instanceof HTMLElement)) return;
    if (n.matches(SKIP)) return;
    if (n.classList.contains("mrender")) {
      const p = await snap(n);
      if (p) out.push(p);
      return;
    }
    if (n instanceof HTMLImageElement) {
      const file = fileOf(n.getAttribute("src") ?? "");
      if (file !== undefined) out.push({ file, w: n.offsetWidth || 200, h: n.offsetHeight || 150 });
      return;
    }
    const c = n.classList;
    const next: Run = { ...st };
    if (c.contains("b")) next.b = true;
    if (c.contains("i")) next.i = true;
    if (c.contains("u")) next.u = true;
    if (c.contains("s")) next.s = true;
    if (c.contains("cd")) next.code = true;
    if (c.contains("hl")) next.hl = [...c].find((x) => x.startsWith("hl-"))?.slice(3) ?? "def";
    if (n.dataset.href) next.href = /^www\./i.test(n.dataset.href) ? `https://${n.dataset.href}` : n.dataset.href;
    if (n.tagName === "SUP") next.sup = true;
    for (const k of n.childNodes) await walk(k, next);
  };
  for (const k of root.childNodes) await walk(k, {});
  // runs side by side with the same look become one
  const merged: Run[] = [];
  for (const r of out) {
    const last = merged[merged.length - 1];
    const same = (a: Run, b: Run) => a.pic === undefined && b.pic === undefined && a.file === undefined && b.file === undefined && (["b", "i", "u", "s", "code", "sup", "hl", "href"] as const).every((k) => a[k] === b[k]);
    if (last && same(last, r)) last.t = (last.t ?? "") + (r.t ?? "");
    else merged.push({ ...r });
  }
  // a block's text starts and ends with its words (the marks' spaces gone)
  const first = merged[0];
  if (first?.t !== undefined) first.t = first.t.replace(/^\s+/, "");
  const last = merged[merged.length - 1];
  if (last?.t !== undefined) last.t = last.t.replace(/\s+$/, "");
  return merged.filter((r) => r.t !== "" || r.pic !== undefined || r.file !== undefined);
}

/** The article's lines as Word blocks (drawn in `host` first, the page's width). */
export async function wordBlocks(item: Item, o: Options, host: HTMLElement, snap: Snap): Promise<Block[]> {
  host.innerHTML = "";
  host.className = `pv-host pv-${o.style}`;
  const measure = el("div", "pv-measure");
  measure.style.width = `${inner(o.style).w}px`;
  const flow = el("div", "ar-text pv-text");
  measure.append(flow);
  host.append(measure);
  const source = await drawLines(item, flow);
  await document.fonts.ready;
  const out: Block[] = [];
  let code: string[] | null = null;
  let table: Run[][][] | null = null;
  /** A callout's name on a line of its own goes with the next line. */
  let label: string | undefined;
  const close = () => {
    if (code) out.push({ k: "code", lines: code });
    if (table) out.push({ k: "table", rows: table });
    code = null;
    table = null;
  };
  for (const ln of [...flow.children] as HTMLElement[]) {
    const c = ln.classList;
    const src = source[Number(ln.dataset.i)] ?? "";
    if (c.contains("code")) {
      if (table) close();
      (code ??= []).push(ln.textContent ?? "");
      continue;
    }
    if (c.contains("fence")) {
      if (c.contains("bottom") || c.contains("top")) close();
      continue;
    }
    if (c.contains("tsep")) continue;
    if (c.contains("tr")) {
      if (code) close();
      const cells: Run[][] = [];
      for (const td of ln.querySelectorAll(":scope > .td")) cells.push(await runsOf(td, item, snap));
      (table ??= []).push(cells);
      continue;
    }
    close();
    if (c.contains("gap")) continue;
    if (c.contains("hr")) out.push({ k: "hr" });
    else if (c.contains("mm")) {
      const box = ln.querySelector<HTMLElement>(".diagram, .formula");
      const p = box ? await snap(box) : null;
      if (p) out.push({ k: "pic", ...p });
    } else if (c.contains("imgline")) {
      const r = await runsOf(ln, item, snap);
      const pic = r.find((x) => x.file !== undefined);
      if (pic) out.push({ k: "pic", file: pic.file, w: pic.w ?? 200, h: pic.h ?? 150 });
    } else if (c.contains("q")) {
      const tint = [...c].find((x) => x.startsWith("co-"))?.slice(3);
      const name = ln.querySelector<HTMLElement>(".co-head")?.dataset.label || label;
      const runs = await runsOf(ln, item, snap);
      if (!runs.length && name) {
        label = name;
        continue;
      }
      label = undefined;
      out.push({ k: "quote", tint: tint ? (Object.values(CALLOUTS).find((x) => x.tint === tint)?.tint ?? tint) : undefined, label: name, runs });
    } else if (c.contains("h")) out.push({ k: "h", level: level(ln), runs: await runsOf(ln, item, snap) });
    else if (c.contains("li")) {
      const m = LIST.exec(src);
      const indent = (m?.[1] ?? "").replace(/\t/g, "    ").length;
      const mark = m?.[4] ? (/x/i.test(m[4]) ? "☑" : "☐") : m && /\d/.test(m[2]) ? m[2] : "•";
      out.push({ k: "li", level: Math.floor(indent / 2), mark, done: c.contains("done"), runs: await runsOf(ln, item, snap) });
    } else if (c.contains("fndef")) out.push({ k: "note", runs: [{ t: (/^\[\^([^\]]+)\]:/.exec(src)?.[1] ?? "") + ". " }, ...(await runsOf(ln, item, snap))] });
    else out.push({ k: "p", runs: await runsOf(ln, item, snap) });
  }
  close();
  return out;
}

/** The colours Word uses (RRGGBB): the palette's for 柚儿园, fixed greys for 印刷. */
export function themeOf(style: Style, at: HTMLElement) {
  const cs = getComputedStyle(at);
  const ctx = document.createElement("canvas").getContext("2d")!;
  const hex = (v: string, fallback: string) => {
    if (!v.trim()) return fallback;
    ctx.fillStyle = fallback;
    ctx.fillStyle = v.trim();
    const s = String(ctx.fillStyle);
    if (s.startsWith("#")) return s;
    const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)/.exec(s);
    return m ? `#${[m[1], m[2], m[3]].map((x) => Math.round(Number(x)).toString(16).padStart(2, "0")).join("")}` : fallback;
  };
  const v = (name: string, fallback: string) => hex(cs.getPropertyValue(name), fallback);
  if (style === "print")
    return {
      ink: "#22261F", soft: "#5D615A", link: "#3A4A8A", pen: "#555555", line: "#C9C9C2", code: "#EEEEEA",
      hl: { def: "#F3E7A3", mint: "#DCEFE3", pink: "#F6DDE3", sky: "#DDE9F6", yuzu: "#F6ECC4", grape: "#E8E0F3" },
      tint: { sky: "#EEF2F7", mint: "#EEF4EF", grape: "#F1EEF5", orange: "#F7F0E8", pink: "#F6EEF0" },
    };
  return {
    ink: v("--color-ink", "#1F2328"), soft: v("--color-ink-soft", "#5D615A"), link: v("--color-link", "#3A4A8A"), pen: v("--color-pen", "#3A4A8A"), line: v("--color-line", "#C9C9C2"), paper: v("--color-paper", "#FBFBF5"), code: v("--color-desk", "#EEEEEA"),
    hl: { def: v("--color-marker", "#F3E7A3"), mint: v("--color-cat-mint-bg", "#DCEFE3"), pink: v("--color-cat-pink-bg", "#F6DDE3"), sky: v("--color-cat-sky-bg", "#DDE9F6"), yuzu: v("--color-yuzu-bg", "#F6ECC4"), grape: v("--color-cat-grape-bg", "#E8E0F3") },
    tint: { sky: v("--color-cat-sky-tint", "#EEF2F7"), mint: v("--color-cat-mint-tint", "#EEF4EF"), grape: v("--color-cat-grape-tint", "#F1EEF5"), orange: v("--color-cat-orange-tint", "#F7F0E8"), pink: v("--color-cat-pink-tint", "#F6EEF0") },
  };
}
