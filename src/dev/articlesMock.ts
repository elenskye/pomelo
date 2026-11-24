// 见字如面 in the browser mock (`?mock`): made-up articles and the same rules as modules/articles.rs, simplified
// (titles from the first line with words, tags outside code, the six lists, the trash, locking with 芝麻开门 open,
// [[links]] by title). mockIpc wraps the writes in its undo and keeps `ar` in its snapshots.
type Args = Record<string, unknown>;
type FileInfo = { id: number; name: string; kind: "image" | "file"; bytes: number; missing: boolean };
type Meta = { pin: number | null; icon: string | null; order: number | null };
export type ARow = { id: number; content: string; title: string; locked: boolean; pinned: boolean; archived: boolean; trashedAt: string | null; createdAt: string; updatedAt: string; files: number[] };

export const ar: { rows: ARow[]; meta: Map<string, Meta>; next: number } = { rows: [], meta: new Map(), next: 1 };

type Deps = { aAdd: (name: string, bytes: number) => FileInfo; aOf: (owner: string, id: number) => FileInfo[]; aSet: (owner: string, id: number, ids: number[] | undefined) => void; vaultOpen: () => boolean };
let deps: Deps;

const now = () => new Date().toISOString();
const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString();

// ---------- text ----------
const fencedLines = (lines: string[]) => {
  const out = new Set<number>();
  let on = false;
  lines.forEach((l, i) => {
    if (/^\s*(```|~~~)/.test(l)) {
      out.add(i);
      on = !on;
    } else if (on) out.add(i);
  });
  return out;
};
export function tagsIn(text: string): string[] {
  const lines = text.split("\n");
  const fenced = fencedLines(lines);
  const out: string[] = [];
  lines.forEach((l, i) => {
    if (fenced.has(i)) return;
    const plain = l.replace(/`[^`]*`/g, (m) => " ".repeat(m.length));
    for (const m of plain.matchAll(/(?<=^|\s)#([^\s#]+)/gu)) {
      const t = m[1].replace(/\/+$/, "");
      if (t && !t.startsWith("/") && !t.includes("//") && [...t].length <= 50 && !out.includes(t)) out.push(t);
    }
  });
  return out;
}
const marksOff = (s: string) =>
  s
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[\[([^\]|]*\|)?([^\]]*)\]\]/g, "$2")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__|==|~~|`)/g, "")
    .replace(/[🟢🔴🔵🟡🟣]/gu, "")
    .replace(/(^|\s)[*_~]|[*_~](\s|$)/g, "$1$2")
    .replace(/:pm-[a-z0-9-]+:/g, "")
    .trim();
const lineText = (l: string) =>
  marksOff(
    l
      .replace(/\s*<!--\s*\{"fold":true\}\s*-->\s*$/, "")
      .replace(/^\s*(>\s*)+/, "")
      .replace(/^#{1,6}\s+/, "")
      .replace(/^\s*([-*+]|\d+[.)])\s+(\[[ xX]\]\s+)?/, "")
      .replace(/^\[![A-Z]+\]/i, "")
      .replace(/(?<=^|\s)#[^\s#]+/gu, ""),
  ).replace(/\s+/g, " ");
function titleLine(content: string): [number, string] {
  const lines = content.split("\n");
  const fenced = fencedLines(lines);
  for (let i = 0; i < lines.length; i++) {
    if (fenced.has(i) || !lines[i].trim()) continue;
    const t = lineText(lines[i]);
    if (t) return [i, [...t].slice(0, 200).join("")];
  }
  return [-1, ""];
}
export const titleOf = (c: string) => titleLine(c)[1];
function previewOf(c: string) {
  const [at] = titleLine(c);
  const lines = c.split("\n");
  const words: string[] = [];
  let n = 0;
  for (let i = 0; i < lines.length && n <= 140; i++) {
    if (i === at || /^\s*(```|~~~)/.test(lines[i]) || !lines[i].replace(/[-*_ ]/g, "")) continue;
    const t = lineText(lines[i]);
    if (!t) continue;
    words.push(t);
    n += t.length + 1;
  }
  const s = words.join(" ");
  return s.length > 140 ? `${s.slice(0, 140)}…` : s;
}
const linksIn = (c: string) => [...c.matchAll(/\[\[([^\]\n]+)\]\]/g)].map((m) => m[1].split("|")[0].trim());
const names = (target: string, title: string) => {
  const t = target.toLowerCase();
  const x = title.toLowerCase();
  return !!x && (t === x || t.split("#")[0].trim() === x || t.slice(0, t.lastIndexOf("/")).trim() === x);
};
const under = (t: string, root: string) => t === root || t.startsWith(`${root}/`);
const live = (r: ARow) => !r.archived && !r.trashedAt;
const thumb = (r: ARow) => {
  if (r.locked) return null;
  let best: [number, number] | null = null;
  for (const f of deps.aOf("article", r.id)) {
    if (f.kind !== "image") continue;
    const at = r.content.indexOf(`](${encodeURI(f.name).replace(/[()#?]/g, (c) => encodeURIComponent(c))})`);
    const at2 = at >= 0 ? at : r.content.indexOf(`](${f.name})`);
    if (at2 >= 0 && (!best || at2 < best[0])) best = [at2, f.id];
  }
  return best?.[1] ?? null;
};
const summary = (r: ARow) => ({
  id: r.id,
  title: r.title,
  preview: r.locked ? "" : previewOf(r.content),
  pinned: r.pinned,
  archived: r.archived,
  trashed: !!r.trashedAt,
  locked: r.locked,
  tags: tagsIn(r.content),
  thumb: thumb(r),
  createdAt: r.createdAt,
  updatedAt: r.updatedAt,
});
const get = (id: number) => {
  const r = ar.rows.find((x) => x.id === id);
  if (!r) throw "找不到这篇笔记，可能已经被删除了";
  return r;
};
const full = (r: ARow) => {
  const open = !r.locked || deps.vaultOpen();
  return {
    id: r.id,
    title: r.title,
    content: open ? r.content : "",
    pinned: r.pinned,
    archived: r.archived,
    trashedAt: r.trashedAt,
    locked: r.locked,
    open,
    tags: tagsIn(r.content),
    attachments: deps.aOf("article", r.id),
    backlinks: ar.rows.filter((x) => x.id !== r.id && !x.locked && !x.trashedAt && linksIn(x.content).some((t) => names(t, r.title))).map((x) => ({ id: x.id, title: x.title })),
    createdAt: r.createdAt,
    updatedAt: r.updatedAt,
  };
};
function put(r: ARow, content: string) {
  if (r.locked && !deps.vaultOpen()) throw "这篇加密了：先用 Touch ID 打开芝麻开门";
  r.content = content;
  r.title = titleOf(content);
  r.updatedAt = now();
}

// ---------- tags ----------
function tree() {
  const by = new Map<string, Set<number>>();
  for (const r of ar.rows.filter(live))
    for (const t of tagsIn(r.content)) {
      const segs = t.split("/");
      for (let i = 1; i <= segs.length; i++) {
        const p = segs.slice(0, i).join("/");
        if (!by.has(p)) by.set(p, new Set());
        by.get(p)!.add(r.id);
      }
    }
  const tags = [...by].sort(([a], [b]) => a.localeCompare(b)).map(([tag, ids]) => ({ tag, count: ids.size, icon: ar.meta.get(tag)?.icon ?? null, order: ar.meta.get(tag)?.order ?? null }));
  const pinned = [...ar.meta].filter(([t, m]) => m.pin !== null && !t.includes("/") && by.has(t)).sort((a, b) => a[1].pin! - b[1].pin!).map(([t]) => t);
  return { tags, pinned };
}
const using = (tag: string) => {
  const rows = ar.rows.filter((r) => tagsIn(r.content).some((t) => under(t, tag)));
  if (!rows.length) throw `没有笔记用到「#${tag}」`;
  return rows;
};
const replaceTag = (text: string, from: string, to: string) => text.replace(/(^|\s)#([^\s#]+)/gu, (all, pre: string, t: string) => (under(t.replace(/\/+$/, ""), from) ? `${pre}#${to}${t.slice(from.length)}` : all));
function renameTag(from: string, to: string) {
  to = to.trim().replace(/^#/, "").replace(/\/+$/, "");
  if (!to) throw "标签不能为空";
  if (/[\s#]/.test(to)) throw "标签里不能有空格或 #";
  if (to === from) return 0;
  if (under(to, from)) throw "不能改成它自己下面的标签";
  const rows = using(from);
  rows.forEach((r) => put(r, replaceTag(r.content, from, to)));
  for (const [t, m] of [...ar.meta]) if (under(t, from)) {
    ar.meta.delete(t);
    ar.meta.set(to + t.slice(from.length), { ...m, pin: to.includes("/") ? null : m.pin });
  }
  return rows.length;
}

// ---------- sample (made up) ----------
function seed() {
  const pic = deps.aAdd("谷中银座.jpg", 380_000);
  const shelf = deps.aAdd("旧书店.png", 220_000);
  const add = (content: string, o: Partial<ARow> & { ago: number }) => {
    const r: ARow = { id: ar.next++, content, title: titleOf(content), locked: false, pinned: false, archived: false, trashedAt: null, createdAt: daysAgo(o.ago + 3), updatedAt: daysAgo(o.ago), files: [], ...o };
    ar.rows.push(r);
    return r;
  };
  add("# 十一月的计划\n#生活\n\n读完两本书，把东京的照片整理好，给小林寄生日礼物，学会做一道新菜。\n\n- [x] 读完《夜航西飞》\n- [ ] 整理 [[东京的第三天]] 的照片\n- [ ] 给小林寄礼物\n  - [ ] 挑一张明信片\n- [ ] 学做番茄牛腩", { pinned: true, ago: 12 });
  const tokyo3 = add(`# 东京的第三天\n#旅行/2026秋天的东京\n\n早上被楼下面包店的香味叫醒。今天的计划是谷中银座和根津，走累了就找一家咖啡店坐下来。\n\n![](${pic.name})\n\n## 下午\n\n下了一阵雨，躲进一家==🟢旧书店==，买到一本 1987 年的旅行手册。晚上去 [[东京的第二天]] 提到的那家拉面店。\n\n> 旅行的意义，是把日常换一个地方过。`, { ago: 7 });
  deps.aSet("article", tokyo3.id, [pic.id]);
  const tokyo2 = add(`# 东京的第二天\n#旅行/2026秋天的东京\n\n雨下了一整天，躲进神保町的旧书店，买到一本 1987 年的旅行手册。\n\n![](${shelf.name})\n\n晚上在小巷里吃了一碗 **很浓的** 拉面。`, { ago: 8 });
  deps.aSet("article", tokyo2.id, [shelf.id]);
  add(
    [
      "# Rust 生命周期笔记",
      "#学习和编程/rust",
      "",
      "生命周期不是让值活得更久，而是告诉编译器几个引用之间==谁比谁活得长==。[^1]",
      "",
      "> [!TIP]",
      "> 先写出来，编译器会告诉你缺哪个标注。",
      "",
      "## 例子",
      "",
      "```rust",
      "fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {",
      "    if x.len() > y.len() { x } else { y }",
      "}",
      "```",
      "",
      "| 写法 | 意思 |",
      "| --- | --- |",
      "| `'a` | 一个生命周期参数 |",
      "| `'static` | 整个程序都活着 |",
      "",
      "借用检查大致是在比区间：$\\text{life}(r) \\subseteq \\text{life}(x)$。",
      "",
      "$$",
      "\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}",
      "$$",
      "",
      "```mermaid",
      "flowchart LR",
      "  借用 --> 检查 --> 通过",
      "```",
      "",
      "- 函数签名里写清楚",
      "- 结构体里的引用也要标",
      "  - 和结构体本身一样长",
      "",
      "---",
      "",
      "[^1]: 摘自一份编的课堂笔记。",
    ].join("\n"),
    { ago: 11 },
  );
  // inline code in a paragraph and in a table's cells (1.0.5: words drawn over each other)
  add(
    [
      "# Pebble 的设计",
      "#学习和编程",
      "",
      "### 1. 插件按需加载是给谁的",
      "**只给 Pebble Loop。** 命令行的插件由命令行自己管，比如 `shell` 本来就会按需加载。",
      "",
      "### 2. 功能清单",
      "依据 `docs/design.md`，分三类。",
      "",
      "**一、两边都给（作为外壳提供的）**",
      "",
      "| 功能 | 循环 | 命令行 |",
      "| --- | --- | --- |",
      "| 一句话一个任务，前后各拍一份快照 | ✓ | ✓ |",
      "| 回退（`restore` / `undo` snapshot） | ✓ | ✓ |",
      "| 同一项目一次只跑一个任务 | ✓ |  |",
    ].join("\n"),
    { ago: 13 },
  );
  // headings folded in Bear (its marks at the line ends): they open shut
  add(
    [
      "# PyTorch 入门",
      "#学习和编程/ml",
      "",
      '### 什么是 Tensor <!-- {"fold":true} -->',
      "多维数组，能放到 GPU 上算。",
      "",
      '### 代码实现：创建 Tensor <!-- {"fold":true} -->',
      "```python",
      "x = torch.zeros(2, 3)",
      "```",
      "",
      '### 代码实现：广播 <!-- {"fold":true} -->',
      "形状不同的两个 Tensor 也能相加。",
      "",
      "### 小结",
      "先会建、会算，再学自动求导。",
    ].join("\n"),
    { ago: 12 },
  );
  add("# 读《夜航西飞》\n#读书/摘抄\n\n「我学会了，一个人可以在不完美里活得很好。」这句抄在书签上。\n\n==🔵飞行== 那几章最好看，~夜里的机场~ 写得像一首诗。", { ago: 16 });
  add("# 给小林的生日信\n#生活\n\n小林：生日快乐！这一年辛苦啦。", { locked: true, ago: 13 });
  add("# 京都的寺\n#旅行/京都\n\n清水寺、南禅寺、哲学之道。", { ago: 40 });
  add("# 周会记录 10/30\n#工作\n\n- 下个月的目标\n- 谁负责哪一块", { archived: true, ago: 20 });
  add("随手记一句：今天的云很好看。", { ago: 3 });
  add("# 不要了的草稿\n#随笔\n\n一些没写完的话。", { trashedAt: daysAgo(1), ago: 30 });
  ar.meta.set("旅行", { pin: 1, icon: "i:plane", order: null });
  ar.meta.set("读书", { pin: null, icon: "i:book-open", order: null });
}

export function initArticles(d: Deps) {
  deps = d;
  seed();
}

// ---------- handlers ----------
const VIEW: Record<string, (r: ARow) => boolean> = {
  all: live,
  pinned: (r) => live(r) && r.pinned,
  untagged: (r) => live(r) && tagsIn(r.content).length === 0,
  archived: (r) => r.archived && !r.trashedAt,
  trash: (r) => !!r.trashedAt,
  locked: (r) => r.locked && !r.trashedAt,
};
type F = { view?: string; tag?: string; q?: string; id?: number };
function listOf(f: F) {
  const words = (f.q ?? "").toLowerCase().split(/\s+/).filter(Boolean);
  const rows = ar.rows.filter(
    (r) =>
      (f.view === "any" || (VIEW[f.view ?? "all"] ?? live)(r)) &&
      (!f.tag || tagsIn(r.content).some((t) => under(t, f.tag!))) &&
      words.every((w) => r.title.toLowerCase().includes(w) || (!r.locked && r.content.toLowerCase().includes(w))) &&
      (f.id === undefined || r.id === f.id),
  );
  rows.sort((a, b) => (f.view === "trash" ? (b.trashedAt ?? "").localeCompare(a.trashedAt ?? "") : Number(b.pinned) - Number(a.pinned) || b.updatedAt.localeCompare(a.updatedAt)));
  return rows.map(summary);
}

export const articleHandlers: Record<string, (a: Args) => unknown> = {
  articles_list: (a) => listOf(a.filter as F),
  search_articles_similar: async (a) => {
    await new Promise((r) => setTimeout(r, 300));
    const f = a.filter as F;
    const chars = [...(f.q ?? "")].filter((c) => c.trim());
    return listOf({ ...f, q: undefined }).filter((s) => chars.some((c) => s.title.includes(c) || s.preview.includes(c))).slice(0, 5);
  },
  articles_overview: () => {
    const n = (k: string) => ar.rows.filter(VIEW[k]).length;
    return { counts: { all: n("all"), pinned: n("pinned"), untagged: n("untagged"), archived: n("archived"), trash: n("trash"), locked: n("locked") }, ...tree() };
  },
  articles_titles: () => ar.rows.filter((r) => !r.trashedAt && r.title).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).map((r) => ({ id: r.id, title: r.title })),
  article_get: (a) => full(get(Number(a.id))),
  article_create: (a) => {
    const p = a.patch as { content?: string; attachments?: number[]; pinned?: boolean };
    const t = now();
    const r: ARow = { id: ar.next++, content: p.content ?? "", title: titleOf(p.content ?? ""), locked: false, pinned: !!p.pinned, archived: false, trashedAt: null, createdAt: t, updatedAt: t, files: [] };
    ar.rows.push(r);
    deps.aSet("article", r.id, p.attachments);
    return full(r);
  },
  article_update: (a) => {
    const r = get(Number(a.id));
    const p = a.patch as { content?: string; pinned?: boolean; archived?: boolean; attachments?: number[] };
    if (p.content !== undefined && p.content !== r.content) put(r, p.content);
    if (p.attachments) deps.aSet("article", r.id, p.attachments);
    if (p.pinned !== undefined) r.pinned = p.pinned;
    if (p.archived !== undefined) r.archived = p.archived;
    return full(r);
  },
  article_trash: (a) => void (get(Number(a.id)).trashedAt = now()),
  article_restore: (a) => void (get(Number(a.id)).trashedAt = null),
  article_delete: (a) => {
    get(Number(a.id));
    ar.rows = ar.rows.filter((r) => r.id !== Number(a.id));
  },
  articles_empty_trash: () => {
    const n = ar.rows.filter((r) => r.trashedAt).length;
    ar.rows = ar.rows.filter((r) => !r.trashedAt);
    return n;
  },
  article_lock: (a) => {
    if (!deps.vaultOpen()) throw "先用 Touch ID 打开芝麻开门";
    const r = get(Number(a.id));
    r.locked = !!a.lock;
    return full(r);
  },
  articles_tag_set: (a) => {
    const tag = String(a.tag);
    const p = a.patch as { pinned?: boolean; icon?: string | null };
    using(tag);
    if (p.pinned && tag.includes("/")) throw "只能置顶第一级标签";
    const m = ar.meta.get(tag) ?? { pin: null, icon: null, order: null };
    if (p.pinned === true && m.pin === null) m.pin = Math.max(0, ...[...ar.meta.values()].map((x) => x.pin ?? 0)) + 1;
    if (p.pinned === false) m.pin = null;
    if (p.icon !== undefined) m.icon = p.icon;
    ar.meta.set(tag, m);
  },
  articles_tag_rename: (a) => renameTag(String(a.from), String(a.to)),
  articles_tag_place: (a) => {
    const tag = String(a.tag);
    const parent = (a.parent as string | null) || null;
    const leaf = tag.slice(tag.lastIndexOf("/") + 1);
    const to = parent ? `${parent}/${leaf}` : leaf;
    const moved = to === tag ? 0 : renameTag(tag, to);
    (a.order as string[]).forEach((t, i) => ar.meta.set(t, { ...(ar.meta.get(t) ?? { pin: null, icon: null, order: null }), order: i + 1 }));
    return moved;
  },
  articles_tag_pins: (a) => (a.order as string[]).forEach((t, i) => ar.meta.set(t, { ...(ar.meta.get(t) ?? { pin: null, icon: null, order: null }), pin: i + 1 })),
  articles_tag_delete: (a) => {
    const tag = String(a.tag);
    const rows = using(tag);
    if (a.articles) rows.forEach((r) => (r.trashedAt = now()));
    else rows.forEach((r) => put(r, r.content.replace(/(^|\s)#([^\s#]+)/gu, (all, pre: string, t: string) => (under(t.replace(/\/+$/, ""), tag) ? pre : all)).replace(/[ \t]+$/gm, "").replace(/\n{3,}/g, "\n\n")));
    return [rows.length, 0];
  },
};
