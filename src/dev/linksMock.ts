// 柚子链接 for ?mock (links.rs, loosely): names → things over the mock's notes, articles and works; lookup, the `[[`
// list, 柚子链接 (backlinks), a thing; 玩物有志's move from a 五花八门 list.
import { escapeName, foundIn, shown, targetOf, type Backlink, type LinkModule, type Thing } from "@/shared/links/links";
import { pad } from "@/shared/dates";

type Args = Record<string, unknown>;
type Note = { id: number; content: string; refs: number[]; createdAt: string };
type Art = { id: number; title: string; content: string; locked: boolean; trashedAt: string | null; createdAt: string; updatedAt: string };
type Work = { id: number; title: string; author?: string | null; director?: string | null; platformId?: number | null; status: string; updatedAt: string };
type LinkDeps = {
  notes: () => Note[];
  articles: () => Art[];
  works: (module: "books" | "films" | "games") => Work[];
  platform: (id: number | null | undefined) => string | null;
};

/** Local `YYYY-MM-DD HH:MM` (`sec`: `:SS` too). */
function local(iso: string, sec = false) {
  const d = new Date(iso);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}${sec ? `:${pad(d.getSeconds())}` : ""}`;
}
const excerptOf = (c: string) => {
  const s = shown(c).replace(/(?<=^|\s)#[^\s#]*/gu, "").replace(/\*\*|==|\+\+/g, "").replace(/^\s*(?:[-*]|\d+[.)]) /gm, "").split(/\s+/).filter(Boolean).join(" ");
  return [...s].length > 60 ? [...s].slice(0, 60).join("") + "…" : s;
};
const STATUS: Record<string, string> = { todo: "想玩", doing: "在玩", done: "通关" };
const LABEL: Record<string, string> = { notes: "念念不忘", books: "摸鱼读读", films: "好奇看看", games: "玩物有志" };

export function linkHandlers(d: LinkDeps) {
  const things = () => {
    const out: (Thing & { names: string[]; updated: string })[] = [];
    for (const a of d.articles().filter((x) => !x.trashedAt).sort((x, y) => x.createdAt.localeCompare(y.createdAt)))
      out.push({ module: "articles", id: a.id, title: a.title, meta: null, date: local(a.updatedAt), text: null, locked: a.locked, target: escapeName(a.title), names: [a.title], updated: a.updatedAt });
    const notes = [...d.notes()].sort((x, y) => x.createdAt.localeCompare(y.createdAt));
    const minutes = new Map<string, number>();
    notes.forEach((n) => minutes.set(local(n.createdAt), (minutes.get(local(n.createdAt)) ?? 0) + 1));
    for (const n of notes) {
      const m = local(n.createdAt);
      const stamp = minutes.get(m) === 1 ? m : local(n.createdAt, true);
      out.push({ module: "notes", id: n.id, title: excerptOf(n.content), meta: null, date: m, text: n.content, locked: false, target: `念念不忘/${stamp}`, names: [m, local(n.createdAt, true)], updated: n.createdAt });
    }
    for (const module of ["books", "films", "games"] as const)
      for (const w of d.works(module)) {
        const meta = module === "books" ? (w.author ?? null) : module === "films" ? (w.director ?? null) : [d.platform(w.platformId), STATUS[w.status]].filter(Boolean).join(" · ") || null;
        out.push({ module, id: w.id, title: w.title, meta, date: null, text: null, locked: false, target: `${LABEL[module]}/${escapeName(w.title)}`, names: [w.title], updated: w.updatedAt });
      }
    return out;
  };
  const resolve = (all: ReturnType<typeof things>, target: string) => {
    const { module, name } = targetOf(target);
    const find = (n: string) => all.find((t) => t.module === module && t.names.some((x) => x.toLowerCase() === n.trim().toLowerCase()));
    return find(name) ?? (module === "articles" ? (find(name.split("#")[0]) ?? find(name.replace(/\/[^/]*$/, ""))) : undefined) ?? null;
  };
  const clean = (t: ReturnType<typeof things>[number] | null): Thing | null => {
    if (!t) return null;
    const { names: _n, updated: _u, ...rest } = t;
    return rest;
  };
  const backlinks = (module: string, id: number): Backlink[] => {
    const all = things();
    const out: (Backlink & { at: string })[] = [];
    const add = (b: Backlink, at: string, text: string) => {
      const hit = foundIn(text).some((f) => {
        const t = resolve(all, f.target);
        return t && t.module === module && t.id === id;
      });
      if (hit && !(b.module === module && b.id === id)) out.push({ ...b, at });
    };
    for (const a of d.articles().filter((x) => !x.locked && !x.trashedAt)) add({ module: "articles", id: a.id, title: a.title, date: local(a.updatedAt).slice(0, 10) }, a.updatedAt, a.content);
    for (const n of d.notes()) add({ module: "notes", id: n.id, title: excerptOf(n.content), date: local(n.createdAt).slice(0, 10) }, n.createdAt, n.content);
    if (module === "notes")
      for (const n of d.notes().filter((x) => x.refs.includes(id) && !out.some((o) => o.module === "notes" && o.id === x.id)))
        out.push({ module: "notes", id: n.id, title: excerptOf(n.content), date: local(n.createdAt).slice(0, 10), at: n.createdAt });
    return out.sort((a, b) => b.at.localeCompare(a.at)).map(({ at: _a, ...b }) => b);
  };
  return {
    backlinks,
    handlers: {
      links_lookup: (a: Args) => {
        const all = things();
        return (a.targets as string[]).map((t) => clean(resolve(all, t)));
      },
      links_suggest: (a: Args) => {
        const q = String(a.q ?? "").trim().toLowerCase();
        const hidden = (a.hidden as string[]) ?? [];
        const except = a.except as [string, number] | null;
        const rank = (t: Thing) => {
          if (!q) return 0;
          const n = t.title.toLowerCase();
          return n === q ? 0 : n.startsWith(q) ? 1 : n.includes(q) ? 2 : `${t.meta ?? ""} ${t.text ?? ""}`.toLowerCase().includes(q) ? 3 : -1;
        };
        return things()
          .filter((t) => !hidden.includes(t.module) && !(except && t.module === except[0] && t.id === except[1]))
          .map((t) => [rank(t), t] as const)
          .filter(([r]) => r >= 0)
          .sort((x, y) => x[0] - y[0] || y[1].updated.localeCompare(x[1].updated))
          .slice(0, 6)
          .map(([, t]) => clean(t));
      },
      links_backlinks: (a: Args) => backlinks(String(a.module), Number(a.id)),
      links_thing: (a: Args) => clean(things().find((t) => t.module === (a.module as LinkModule) && t.id === Number(a.id)) ?? null),
    } as Record<string, (a: Args) => unknown>,
  };
}
