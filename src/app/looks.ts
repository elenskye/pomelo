// Each module's sticker and color, picked in the 模块坞 (「图标和颜色…」) or when making a module: settings
// `ui.module_look` = { [module]: { mark?, tint? } }; left out = the module's own (built-in: its sticker and its
// category color; made: the sticker of the icon it was made with). The color reaches everything painted with the
// module's color (page header, 页签, 索引签, search) through its CSS variable (moduleLooks.ts sets it on <html>).
// No two modules may wear the same sticker in the same color (requirements「图标和颜色」, 2026-11-08): the 柚子瓣
// shows stickers only. effectiveLooks() settles what each one shows; the picker greys out what another one wears.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { isTint, markSvg, stickerById, TINTS, type Sticker, type Tint } from "@/shared/marks";
import type { ModuleDef } from "./modules";

export type Look = { mark?: string; tint?: Tint };
export type Looks = Record<string, Look>;

const KEY = "ui.module_look";
const QUERY = ["setting", KEY];

const clean = (v: unknown): Looks => {
  if (!v || typeof v !== "object" || Array.isArray(v)) return {};
  const out: Looks = {};
  for (const [id, l] of Object.entries(v as Record<string, unknown>)) {
    if (!l || typeof l !== "object") continue;
    const { mark, tint } = l as Record<string, unknown>;
    const look: Look = {};
    if (typeof mark === "string" && mark) look.mark = mark;
    if (isTint(tint)) look.tint = tint;
    if (look.mark || look.tint) out[id] = look;
  }
  return out;
};

/** A module's CSS color variable (`--module-todo`, made ones `--module-list-3`). */
export const colorVar = (id: string) => `--module-${id.replace(/[^a-zA-Z0-9]+/g, "-")}`;

export function useLooks() {
  const qc = useQueryClient();
  const { data: looks = {} } = useQuery({ queryKey: QUERY, queryFn: () => api.settingsGet<unknown>(KEY).then(clean) });
  /** Save one module's look (null or empty = back to its own). */
  const set = (id: string, look: Look | null) => {
    const next = { ...(qc.getQueryData<Looks>(QUERY) ?? looks) };
    if (look && (look.mark || look.tint)) next[id] = { ...(look.mark ? { mark: look.mark } : {}), ...(look.tint ? { tint: look.tint } : {}) };
    else delete next[id];
    qc.setQueryData(QUERY, next);
    api.settingsSet(KEY, next).catch(() => {});
  };
  /** A deleted module's look goes with it (a new one may get its key). */
  const forget = (ids: string[]) => {
    const cur = qc.getQueryData<Looks>(QUERY) ?? looks;
    if (!ids.some((id) => cur[id])) return;
    const next = { ...cur };
    for (const id of ids) delete next[id];
    qc.setQueryData(QUERY, next);
    api.settingsSet(KEY, next).catch(() => {});
  };
  return { looks, lookOf: (id: string): Look => looks[id] ?? {}, set, forget };
}

/** The sticker a module wears by default: its own (built-in), the one of its icon (made). */
export const ownMark = (def: ModuleDef): string => def.mark ?? def.id;
/** A module's own color: its own sticker's (= its category color for the built-in ones). */
export const ownTint = (def: ModuleDef): Tint | null => stickerById(ownMark(def))?.tint ?? null;

/** What a module shows: the sticker (null before a sync brings it, and for pages that aren't modules) and its color
 *  (picked, else the module's own: another sticker keeps the module's color unless one is picked). */
export function resolveLook(def: ModuleDef, look: Look): { sticker: Sticker | null; svg: string | null; tint: Tint | null } {
  if (def.id === "youzi") return { sticker: null, svg: markSvg("pomelo-normal"), tint: null };
  const sticker = (look.mark ? stickerById(look.mark) : null) ?? stickerById(ownMark(def));
  return { sticker, svg: sticker?.svg ?? null, tint: look.tint ?? ownTint(def) };
}

/** What to store for a module that should show `mark` in `tint`: only what differs from its own. */
export const lookFor = (own: { mark: string; tint: Tint | null }, mark: string, tint: Tint | null): Look => ({
  ...(mark !== own.mark ? { mark } : {}),
  ...(tint && tint !== own.tint ? { tint } : {}),
});

/** A sticker in a color, as one key. */
export const lookKey = (mark: string, tint: Tint | null) => `${mark}|${tint ?? ""}`;
export type Effective = { mark: string; tint: Tint | null };
const isMade = (id: string) => /^(list|progress)\//.test(id);

/**
 * What every module shows, no two alike: the built-in modules keep their own first, then what was picked, then
 * the made modules' own. One that would wear what another already does moves to the next free color (from its
 * own, in TINTS order); a pick is checked when it is saved, so only old settings or two made modules with the
 * same icon ever move. 柚子 is 柚子 and takes no part.
 */
export function effectiveLooks(defs: ModuleDef[], looks: Looks): Map<string, Effective> {
  const out = new Map<string, Effective>();
  const used = new Set<string>();
  const place = (def: ModuleDef) => {
    const l = looks[def.id] ?? {};
    let e: Effective = { mark: l.mark && stickerById(l.mark) ? l.mark : ownMark(def), tint: l.tint ?? ownTint(def) };
    if (used.has(lookKey(e.mark, e.tint))) {
      const free = freeTint(e.mark, e.tint, used);
      if (free) e = { ...e, tint: free };
    }
    used.add(lookKey(e.mark, e.tint));
    out.set(def.id, e);
  };
  const mods = defs.filter((d) => d.id !== "youzi");
  mods.filter((d) => !isMade(d.id) && !looks[d.id]).forEach(place);
  mods.filter((d) => looks[d.id]).forEach(place);
  mods.filter((d) => isMade(d.id) && !looks[d.id]).forEach(place);
  return out;
}

/** The first color, from `from` on, that `mark` isn't worn in (`taken` holds lookKey()s); null when all are. */
export function freeTint(mark: string, from: Tint | null, taken: { has: (k: string) => boolean }): Tint | null {
  const i = from ? TINTS.indexOf(from) : 0;
  return [...TINTS.slice(i), ...TINTS.slice(0, i)].find((t) => !taken.has(lookKey(mark, t))) ?? null;
}
