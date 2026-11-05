// 一网打尽 in the browser mock (`?mock`): a made-up tree, the same rules as modules/bookmarks.rs (http(s)
// only, no name = the host, never a folder into itself, deleting a folder deletes what's in it). No icons
// (the mock never goes online): the page shows first letters. mockIpc wraps the writes in its undo.
import type { Bookmark, NewBookmark } from "@/modules/bookmarks/api";

type Args = Record<string, unknown>;

export const bm: { rows: Bookmark[]; next: number } = { rows: [], next: 1 };

const now = () => new Date().toISOString();

function host(url: string) {
  const rest = url.includes("://") ? url.slice(url.indexOf("://") + 3) : url;
  const authority = rest.split(/[/?#]/)[0] ?? "";
  return authority.slice(authority.lastIndexOf("@") + 1).split(":")[0].toLowerCase();
}

function normalize(raw: string) {
  const s = raw.trim();
  if (!s) throw "网址不能是空的";
  if (/\s/.test(s)) throw "网址里不能有空格";
  const url = /^https?:\/\//i.test(s) ? s : s.includes("://") || /^(javascript|data|file):/i.test(s) ? null : `https://${s}`;
  if (!url) throw "只收 http 和 https 的网址";
  if (!host(url)) throw "网址不对：没有网站的地址";
  return url;
}

const kids = (parent: number | null) => bm.rows.filter((b) => b.parentId === parent).sort((a, b) => a.sortOrder - b.sortOrder || a.id - b.id);

function add(parentId: number | null, kind: Bookmark["kind"], title: string, url = "", onHome = false) {
  const t = now();
  const b: Bookmark = { id: bm.next++, parentId, kind, title, url, onHome, sortOrder: kids(parentId).length + 1, createdAt: t, updatedAt: t };
  bm.rows.push(b);
  return b;
}

// made-up sample: folders in folders, a link on 首页
{
  const work = add(null, "folder", "工作");
  const mail = add(work.id, "folder", "邮箱");
  add(mail.id, "link", "示例邮箱", "https://mail.example.com/");
  add(work.id, "link", "GitHub", "https://github.com/", true);
  add(work.id, "link", "Vercel", "https://vercel.com/new");
  const api = add(null, "folder", "API");
  add(api.id, "link", "DeepSeek Platform", "https://platform.deepseek.com/usage", true);
  add(api.id, "link", "OpenRouter", "https://openrouter.ai/");
  const cn = add(api.id, "folder", "国内");
  add(cn.id, "link", "百炼控制台", "https://bailian.console.aliyun.com/");
  const study = add(null, "folder", "学习");
  add(study.id, "link", "Google Scholar", "https://scholar.google.com/");
  add(study.id, "link", "中国知网", "https://kns.cnki.net/");
  add(null, "folder", "空的文件夹");
  add(null, "link", "柚儿园的说明", "https://example.com/pomelo");
}

const get = (id: number) => {
  const b = bm.rows.find((x) => x.id === id);
  if (!b) throw "找不到这一条，可能已经被删除了";
  return b;
};
const checkParent = (p: number | null) => {
  if (p == null) return;
  const f = bm.rows.find((x) => x.id === p);
  if (!f) throw "这个文件夹已经不在了";
  if (f.kind !== "folder") throw "只能放进文件夹";
};

export const bookmarkHandlers: Record<string, (a: Args) => unknown> = {
  bookmarks_list: () => structuredClone([...bm.rows].sort((a, b) => Number(a.parentId != null) - Number(b.parentId != null) || (a.parentId ?? 0) - (b.parentId ?? 0) || a.sortOrder - b.sortOrder || a.id - b.id)),
  bookmarks_create: (a) => {
    const i = a.input as NewBookmark;
    checkParent(i.parentId);
    if (i.kind === "link") {
      const url = normalize(i.url ?? "");
      return add(i.parentId, "link", i.title.trim() || host(url), url, !!i.onHome);
    }
    if (!i.title.trim()) throw "文件夹要有名字";
    return add(i.parentId, "folder", i.title.trim());
  },
  bookmarks_update: (a) => {
    const b = get(a.id as number);
    const p = a.patch as { title?: string; url?: string; onHome?: boolean };
    if (p.url !== undefined && b.kind === "folder") throw "文件夹没有网址";
    const url = p.url !== undefined ? normalize(p.url) : b.url;
    const title = p.title !== undefined ? p.title.trim() || (b.kind === "link" ? host(url) : "") : b.title;
    if (!title) throw "文件夹要有名字";
    Object.assign(b, { title, url, onHome: b.kind === "link" && (p.onHome ?? b.onHome), updatedAt: now() });
    return structuredClone(b);
  },
  bookmarks_place: (a) => {
    const ids = [...new Set(a.ids as number[])];
    const parent = (a.parentId as number | null) ?? null;
    const before = (a.before as number | null) ?? null;
    checkParent(parent);
    ids.forEach(get);
    for (let at = parent == null ? undefined : bm.rows.find((x) => x.id === parent); at; at = at.parentId == null ? undefined : bm.rows.find((x) => x.id === at!.parentId))
      if (ids.includes(at.id)) throw "文件夹不能放进它自己或它里面的文件夹";
    const order = kids(parent).filter((b) => !ids.includes(b.id)).map((b) => b.id);
    const at = before != null && !ids.includes(before) && order.includes(before) ? order.indexOf(before) : order.length;
    order.splice(at, 0, ...ids);
    order.forEach((id, i) => Object.assign(get(id), { parentId: parent, sortOrder: i + 1 }));
    return null;
  },
  bookmarks_delete: (a) => {
    const gone = new Set<number>();
    const drop = (id: number) => {
      gone.add(id);
      bm.rows.filter((x) => x.parentId === id).forEach((x) => drop(x.id));
    };
    (a.ids as number[]).forEach(drop);
    const before = bm.rows.length;
    bm.rows = bm.rows.filter((x) => !gone.has(x.id));
    return before - bm.rows.length;
  },
  bookmarks_icons: () => ({}),
};

/** The links marked 放首页, in the tree's order. */
export function bookmarksHome(): Bookmark[] {
  const out: Bookmark[] = [];
  const walk = (p: number | null) => kids(p).forEach((b) => (b.kind === "folder" ? walk(b.id) : b.onHome && out.push(structuredClone(b))));
  walk(null);
  return out;
}
