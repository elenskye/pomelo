// The 模块坞's layout (requirements「界面外框」), one copy for the 模块坞 page, the 柚子瓣 and the 索引签:
// categories (each with its modules in order), the pinned row and the hidden ones. Kept in settings
// (`ui.dock` = { cats, pinned }; hidden stays `ui.module_hidden`, which search and 小拾 read), never undone.
// A module missing from every category (new, or made since) joins its default one; made modules go to
// 兴趣 unless made from a category's「＋」. Hiding 小拾 hides its 助手 too.
import { useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useAllModules } from "@/modules/custom/api";
import { api } from "@/shared/ipc";
import { useHiddenModules } from "./hidden";
import { ASSISTANT_MODULES, type ModuleDef } from "./modules";

const KEY = "ui.dock";
const QUERY = ["setting", KEY];

export type Cat = { id: string; name: string; tone: number; mods: string[] };
export type Layout = { cats: Cat[]; pinned: string[] };

/** Category colors, by `tone`. */
export const TONES = ["var(--module-todo)", "var(--module-finance)", "var(--module-books)", "var(--module-jobs)", "var(--module-site)", "var(--module-picky)", "var(--module-vault)", "var(--module-days)", "var(--module-films)", "var(--module-packing)"];
export const toneColor = (tone: number) => TONES[((tone % TONES.length) + TONES.length) % TONES.length];

const INTEREST = "c3";
export const DEFAULT: Layout = {
  cats: [
    { id: "c1", name: "日常", tone: 0, mods: ["todo", "notes", "days"] },
    { id: "c2", name: "生活", tone: 1, mods: ["finance", "packing"] },
    { id: INTEREST, name: "兴趣", tone: 2, mods: ["books", "films", "drawing"] },
    { id: "c4", name: "求职", tone: 3, mods: ["jobs", "seeker", "rater"] },
    { id: "c5", name: "创作", tone: 4, mods: ["site"] },
    { id: "c6", name: "小拾", tone: 5, mods: ["picky", "translate", "pub2md"] },
    { id: "c7", name: "工具", tone: 6, mods: ["vault"] },
  ],
  pinned: ["todo", "notes", "jobs", "seeker", "picky"],
};
const HOME_OF: Record<string, string> = Object.fromEntries(DEFAULT.cats.flatMap((c) => c.mods.map((m) => [m, c.id])));

const isLayout = (v: unknown): v is Layout => {
  const l = v as Layout;
  return (
    !!l &&
    Array.isArray(l.cats) &&
    Array.isArray(l.pinned) &&
    l.pinned.every((p) => typeof p === "string") &&
    l.cats.every((c) => c && typeof c.id === "string" && typeof c.name === "string" && typeof c.tone === "number" && Array.isArray(c.mods) && c.mods.every((m) => typeof m === "string"))
  );
};

/** Every known module in exactly one category; unknown ones dropped. */
export function normalize(saved: Layout, known: string[]): Layout {
  const seen = new Set<string>();
  const cats = saved.cats.map((c) => ({ ...c, mods: c.mods.filter((m) => known.includes(m) && !seen.has(m) && (seen.add(m), true)) }));
  for (const m of known.filter((k) => !seen.has(k))) {
    const home = cats.find((c) => c.id === (HOME_OF[m] ?? INTEREST)) ?? cats.find((c) => c.id === INTEREST) ?? cats[cats.length - 1];
    if (home) home.mods.push(m);
    else cats.push({ id: "c1", name: "模块", tone: 0, mods: [m] });
  }
  return { cats, pinned: [...new Set(saved.pinned.filter((p) => known.includes(p)))] };
}

export type Dock = {
  /** Every module the dock shows (data, made, 助手), by id. */
  defs: Map<string, ModuleDef>;
  layout: Layout;
  hidden: string[];
  /** Hidden, or a 助手 while 小拾 is. */
  isHidden: (id: string) => boolean;
  pinned: string[];
  catOf: (id: string) => Cat | undefined;
  /** Drop `mod` into a zone ("pinned" | "hidden" | "cat:<id>"), before `before` (else at the end). */
  move: (mod: string, zone: string, before?: string, from?: string) => void;
  togglePin: (mod: string) => void;
  toggleHide: (mod: string) => void;
  addCat: () => string;
  renameCat: (id: string, name: string) => void;
  deleteCat: (id: string) => void;
  /** A module just made from a category's「＋」. */
  place: (mod: string, catId: string) => void;
};

export function useDock(): Dock {
  const qc = useQueryClient();
  const { data: saved } = useQuery({ queryKey: QUERY, queryFn: () => api.settingsGet<unknown>(KEY).then((v) => (isLayout(v) ? v : null)) });
  const [hidden, setHidden] = useHiddenModules();
  const all = useAllModules();
  const defs = useMemo(() => new Map([...all, ...ASSISTANT_MODULES].map((m) => [m.id, m])), [all]);
  const known = useMemo(() => [...defs.keys()], [defs]);
  const layout = useMemo(() => normalize(saved ?? DEFAULT, known), [saved, known]);
  const isHidden = (id: string) => hidden.includes(id) || (!!defs.get(id)?.assistant && hidden.includes("picky"));
  const save = (l: Layout) => {
    qc.setQueryData(QUERY, l);
    api.settingsSet(KEY, l).catch(() => {});
  };
  const clone = (): Layout => ({ cats: layout.cats.map((c) => ({ ...c, mods: [...c.mods] })), pinned: [...layout.pinned] });
  const insert = (list: string[], mod: string, before?: string) => {
    const i = before ? list.indexOf(before) : -1;
    list.splice(i < 0 ? list.length : i, 0, mod);
  };
  const unhide = (mod: string) => hidden.includes(mod) && setHidden(hidden.filter((h) => h !== mod));
  return {
    defs,
    layout,
    hidden,
    isHidden,
    pinned: layout.pinned.filter((m) => !isHidden(m)),
    catOf: (id) => layout.cats.find((c) => c.mods.includes(id)),
    move: (mod, zone, before, from) => {
      const l = clone();
      if (zone === "pinned") {
        l.pinned = l.pinned.filter((m) => m !== mod);
        insert(l.pinned, mod, before !== mod ? before : undefined);
        unhide(mod);
        return save(l);
      }
      if (zone === "hidden") {
        l.pinned = l.pinned.filter((m) => m !== mod);
        save(l);
        if (!hidden.includes(mod)) setHidden([...hidden, mod]);
        return;
      }
      if (!zone.startsWith("cat:")) return;
      const c = l.cats.find((x) => x.id === zone.slice(4));
      if (!c) return;
      // dragged out of the pinned row into a category: it leaves the row
      if (from === "pinned") l.pinned = l.pinned.filter((m) => m !== mod);
      l.cats.forEach((x) => (x.mods = x.mods.filter((m) => m !== mod)));
      insert(c.mods, mod, before !== mod ? before : undefined);
      save(l);
      unhide(mod);
    },
    togglePin: (mod) => {
      const l = clone();
      if (l.pinned.includes(mod)) l.pinned = l.pinned.filter((m) => m !== mod);
      else {
        l.pinned.push(mod);
        unhide(mod);
      }
      save(l);
    },
    toggleHide: (mod) => {
      if (hidden.includes(mod)) return unhide(mod);
      const l = clone();
      l.pinned = l.pinned.filter((m) => m !== mod);
      save(l);
      setHidden([...hidden, mod]);
    },
    addCat: () => {
      const l = clone();
      const n = Math.max(0, ...l.cats.map((c) => Number(c.id.slice(1)) || 0)) + 1;
      const tone = Math.max(-1, ...l.cats.map((c) => c.tone)) + 1;
      const id = `c${n}`;
      l.cats.push({ id, name: "新分类", tone, mods: [] });
      save(l);
      return id;
    },
    renameCat: (id, name) => {
      const n = name.trim().slice(0, 12);
      const l = clone();
      const c = l.cats.find((x) => x.id === id);
      if (!c || !n || c.name === n) return;
      c.name = n;
      save(l);
    },
    deleteCat: (id) => {
      const l = clone();
      if (l.cats.find((c) => c.id === id)?.mods.length) return;
      l.cats = l.cats.filter((c) => c.id !== id);
      save(l);
    },
    place: (mod, catId) => {
      const l = clone();
      const c = l.cats.find((x) => x.id === catId);
      if (!c) return;
      l.cats.forEach((x) => (x.mods = x.mods.filter((m) => m !== mod)));
      c.mods.push(mod);
      save(l);
    },
  };
}
