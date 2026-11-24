// 柚子链接 (v1.0 Phase 5): links between 见字如面, 念念不忘 and 摸鱼读读 / 好奇看看 / 玩物有志. The rules are Rust's
// (src-tauri/src/modules/links.rs: what a target names, 柚子链接, `[[` suggestions, renames); this mirrors only
// the parsing, to draw the text: `[[目标]]` a chip (the module's sticker + the name), `[[目标|字]]` the words as a
// Pomelo link, `![[目标]]` (articles) a note set in as a card. 目标 = `<模块名>/<名字>`, or a bare article title.
import { invoke } from "@tauri-apps/api/core";
import { useQuery, type QueryClient } from "@tanstack/react-query";

export const KINDS = [
  ["articles", "见字如面"],
  ["notes", "念念不忘"],
  ["books", "摸鱼读读"],
  ["films", "好奇看看"],
  ["games", "玩物有志"],
] as const;
export type LinkModule = (typeof KINDS)[number][0];

/** The record kind each module's page opens (全局搜索's Hit.kind). */
export const RECORD: Record<LinkModule, string> = { articles: "article", notes: "note", books: "book", films: "film", games: "game" };

/** A thing a link names (links.rs `Thing`). */
export type Thing = {
  module: LinkModule;
  id: number;
  /** An article's title, a note's opening words, a work's name. */
  title: string;
  /** 作者 / 导演 / 平台 · 状态. */
  meta: string | null;
  /** Local `YYYY-MM-DD HH:MM` (notes: written; articles: last edited). */
  date: string | null;
  /** A note's whole text. */
  text: string | null;
  locked: boolean;
  /** What a link to it is written as. */
  target: string;
};

/** An article or note linking to a thing (links.rs `Backlink`). */
export type Backlink = { module: LinkModule; id: number; title: string; date: string };

/** A target's module and name. */
export function targetOf(raw: string): { module: LinkModule; name: string } {
  const t = raw.trim();
  for (const [k, l] of KINDS) {
    if (t.startsWith(`${l}/`) && t.slice(l.length + 1).trim()) return { module: k, name: t.slice(l.length + 1).trim() };
  }
  return { module: "articles", name: t };
}

export const escapeName = (name: string) => name.replace(/[\\|[\]]/g, (c) => `\\${c}`);

/** One `[[…]]` in a line. */
type Found = {
  /** The whole thing, `!` included: [start, end). */
  start: number;
  end: number;
  embed: boolean;
  /** Escapes undone. */
  target: string;
  alias: string | null;
  /** Where `[[`, the target, `|` + alias and `]]` are: [start of target, end of target, end of alias part (before `]]`)]. */
  inner: [number, number, number];
};

/** The links in a line (or a text; a link never spans lines). */
export function foundIn(text: string): Found[] {
  const out: Found[] = [];
  let at = 0;
  for (;;) {
    const i = text.indexOf("[[", at);
    if (i < 0) break;
    const j = text.indexOf("]]", i + 2);
    if (j < 0) break;
    const inner = text.slice(i + 2, j);
    if (!inner.includes("\n")) {
      let target = "";
      let cut = inner.length;
      for (let k = 0; k < inner.length; ) {
        const c = inner[k];
        if (c === "\\" && k + 1 < inner.length && /[!-/:-@[-`{-~]/.test(inner[k + 1])) {
          target += inner[k + 1];
          k += 2;
          continue;
        }
        if (c === "|") {
          cut = k;
          break;
        }
        target += c;
        k++;
      }
      const alias = cut < inner.length ? inner.slice(cut + 1).trim() || null : null;
      if (target.trim()) {
        const embed = i > 0 && text[i - 1] === "!";
        out.push({ start: embed ? i - 1 : i, end: j + 2, embed, target: target.trim(), alias, inner: [i + 2, i + 2 + cut, j] });
      }
    }
    at = j + 2;
  }
  return out;
}

/** A text with its links as they read (`[[摸鱼读读/夜航西飞]]` → 夜航西飞, `[[目标|字]]` → 字), like links.rs `shown`. */
export function shown(text: string): string {
  let out = "";
  let last = 0;
  for (const f of foundIn(text)) {
    out += text.slice(last, f.start) + (f.alias ?? targetOf(f.target).name);
    last = f.end;
  }
  return out + text.slice(last);
}

/** `M 月 D 日` of a local `YYYY-MM-DD…`. */
export const dayText = (d: string | null) => (d ? `${Number(d.slice(5, 7))} 月 ${Number(d.slice(8, 10))} 日` : "");
/** `MM / DD` of a local `YYYY-MM-DD…` (this year), else `YYYY / MM / DD`. */
export function shortDay(d: string): string {
  const [y, m, day] = [d.slice(0, 4), d.slice(5, 7), d.slice(8, 10)];
  return y === String(new Date().getFullYear()) ? `${m} / ${day}` : `${y} / ${m} / ${day}`;
}

export const LINKS_KEY = ["links"];

/** An article, a note or a work changed: what links name and who links to whom are asked again — and 念念不忘's lists,
 *  whose notes carry their 柚子链接 (an article that came to link to a note has to show under it in every list). */
export function linksChanged(qc: QueryClient) {
  void qc.invalidateQueries({ queryKey: LINKS_KEY });
  void qc.invalidateQueries({ queryKey: ["notes"] });
}

export const linksApi = {
  lookup: (targets: string[]) => invoke<(Thing | null)[]>("links_lookup", { targets }),
  suggest: (q: string, hidden: string[], except: [string, number] | null) => invoke<Thing[]>("links_suggest", { q, hidden, except }),
  backlinks: (module: string, id: number) => invoke<Backlink[]>("links_backlinks", { module, id }),
  thing: (module: string, id: number) => invoke<Thing | null>("links_thing", { module, id }),
};

/** What the targets name (null: 找不到), refreshed when anything a link can name changes (LINKS_KEY). */
export function useLookup(targets: string[]): Map<string, Thing | null> {
  const keys = [...new Set(targets)].sort();
  const q = useQuery({
    queryKey: [...LINKS_KEY, "lookup", ...keys],
    queryFn: () => linksApi.lookup(keys),
    enabled: keys.length > 0,
    staleTime: 0,
    placeholderData: (prev) => prev,
  });
  const out = new Map<string, Thing | null>();
  if (q.data && q.data.length === keys.length) keys.forEach((k, i) => out.set(k, q.data![i]));
  return out;
}

export function useBacklinks(module: string, id: number | null) {
  return useQuery({ queryKey: [...LINKS_KEY, "back", module, id], queryFn: () => linksApi.backlinks(module, id!), enabled: id !== null, staleTime: 0 });
}
