// The drawn marks synced from visual-atelier (src/vendor/pomelo/marks/, never edited here): 柚子's 8 faces,
// the AI button, the stickers a module can wear (modules/: the built-in modules' own and the general ones).
// Colors are var(--pm-…), so they follow the palette and dark mode; a sticker's main color can be swapped
// for another of the 5 category colors (`tint`: --mark-color on the wrapper, which the sticker reads).
// Each use gets its own ids (the sticker's shadow filter), so one sticker can be on a page many times.
// Before a sync brings them there is no mark and callers keep their Lucide icon.
import { useId, useMemo, type CSSProperties } from "react";

const FILES = import.meta.glob<string>("/src/vendor/pomelo/marks/**/*.svg", { query: "?raw", import: "default", eager: true });
const BY_NAME = new Map(Object.entries(FILES).map(([p, svg]) => [p.replace("/src/vendor/pomelo/marks/", "").replace(/\.svg$/, ""), svg]));

/** A mark by its path under marks/ without .svg: "pomelo-normal", "ai-button", "modules/todo". */
export const markSvg = (name: string): string | null => BY_NAME.get(name) ?? null;

/** A module's own sticker (柚子's own module is 柚子 itself); null for made modules and the pages that aren't modules. */
export const moduleMark = (id: string): string | null => (id === "youzi" ? markSvg("pomelo-normal") : markSvg(`modules/${id}`));

/** The 5 category colors a sticker (and its module) can take. */
export const TINTS = ["mint", "pink", "grape", "orange", "sky"] as const;
export type Tint = (typeof TINTS)[number];
export const TINT_NAMES: Record<Tint, string> = { mint: "薄荷", pink: "樱粉", grape: "葡萄紫", orange: "橙", sky: "天蓝" };
export const isTint = (v: unknown): v is Tint => TINTS.includes(v as Tint);
export const tintColor = (t: Tint) => `var(--pm-cat-${t})`;

export type Sticker = { id: string; name: string; /** its own color */ tint: Tint | null; svg: string };
// the order of the picker: the built-in modules' stickers as the 模块坞 lists them, then the general ones
const OWN = ["todo", "notes", "days", "finance", "packing", "books", "films", "drawing", "site", "canvas", "jobs", "seeker", "rater", "translate", "pub2md", "importer", "atlas", "vault", "bookmarks", "janitor"];
const MORE = ["shapes", "checklist", "game", "music", "headphones", "camera", "tv", "coffee", "bowl", "bag", "gift", "shirt", "plane", "mountain", "leaf", "sprout", "dumbbell", "heart", "star", "grad", "code", "puzzle", "trophy", "cat", "flower", "bulb", "house", "pill", "letter", "piggy", "dice", "moon"];
const rank = (id: string) => {
  const i = OWN.indexOf(id), j = MORE.indexOf(id);
  return i >= 0 ? i : j >= 0 ? 100 + j : 1000;
};
const toSticker = (id: string, svg: string): Sticker => {
  const tint = /--pm-cat-(mint|pink|grape|orange|sky),/.exec(svg)?.[1];
  return { id, name: /aria-label="([^"]*)"/.exec(svg)?.[1] ?? id, tint: isTint(tint) ? tint : null, svg };
};
/** Every sticker a module can wear, in the picker's order; `own` = a built-in module's. */
export const STICKERS: (Sticker & { own: boolean })[] = [...BY_NAME]
  .filter(([n]) => n.startsWith("modules/"))
  .map(([n, svg]) => ({ ...toSticker(n.slice(8), svg), own: OWN.includes(n.slice(8)) }))
  .sort((a, b) => rank(a.id) - rank(b.id) || a.id.localeCompare(b.id));
export const stickerById = (id: string) => STICKERS.find((s) => s.id === id) ?? null;

/** A mark's SVG at `size` px square, with its ids made unique by `uid` (so the same mark can be on a page many times). */
export function sizedMark(svg: string, size: number, uid: string, height = size) {
  let out = svg
    .replace(/^<svg([^>]*?) width="[\d.]+" height="[\d.]+"/, `<svg$1 width="${size}" height="${height}" style="display:block;overflow:visible" focusable="false"`)
    .replace(/ role="img" aria-label="[^"]*"/, ' aria-hidden="true"');
  for (const [, id] of svg.matchAll(/id="([^"]+)"/g)) out = out.split(`id="${id}"`).join(`id="${id}-${uid}"`).split(`url(#${id})`).join(`url(#${id}-${uid})`);
  return out;
}

/** One mark, `size` px square (the sticker's white edge and shadow sit inside it). Decorative unless labelled.
 *  `tint` swaps a sticker's main color (and its lighter one) for another category color. */
export function Mark(props: { svg: string; size: number; label?: string; className?: string; tint?: Tint | null }) {
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const html = useMemo(() => sizedMark(props.svg, props.size, uid), [props.svg, props.size, uid]);
  const tint = props.tint ? ({ "--mark-color": tintColor(props.tint), "--mark-color-soft": `var(--pm-cat-${props.tint}-sticker)` } as CSSProperties) : undefined;
  return (
    <span
      className={`inline-block shrink-0 ${props.className ?? ""}`}
      style={{ width: props.size, height: props.size, ...tint }}
      role={props.label ? "img" : undefined}
      aria-label={props.label}
      aria-hidden={props.label ? undefined : true}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

/** 柚子's small badge on its 助手 (was a bird). */
export function YouziBadge(props: { size?: number; className?: string }) {
  const svg = markSvg("pomelo-normal");
  return svg ? <Mark svg={svg} size={props.size ?? 14} className={props.className} label="柚子的助手" /> : null;
}
