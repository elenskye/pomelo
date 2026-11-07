// The drawn marks synced from visual-atelier (src/vendor/pomelo/marks/, never edited here): 柚子's 8 faces,
// the AI button, the module stickers. Colors are var(--pm-…), so they follow the palette and dark mode.
// Each use gets its own ids (the sticker's shadow filter), so one sticker can be on a page many times.
// Before a sync brings them (or for made modules) there is no mark and callers keep their Lucide icon.
import { useId, useMemo } from "react";

const FILES = import.meta.glob<string>("/src/vendor/pomelo/marks/**/*.svg", { query: "?raw", import: "default", eager: true });
const BY_NAME = new Map(Object.entries(FILES).map(([p, svg]) => [p.replace("/src/vendor/pomelo/marks/", "").replace(/\.svg$/, ""), svg]));

/** A mark by its path under marks/ without .svg: "pomelo-normal", "ai-button", "modules/todo". */
export const markSvg = (name: string): string | null => BY_NAME.get(name) ?? null;

/** A module's sticker (柚子's own module is 柚子 itself); null for made modules and the pages that aren't modules. */
export const moduleMark = (id: string): string | null => (id === "youzi" ? markSvg("pomelo-normal") : markSvg(`modules/${id}`));

function sized(svg: string, size: number, uid: string) {
  let out = svg
    .replace(/^<svg([^>]*?) width="[\d.]+" height="[\d.]+"/, `<svg$1 width="${size}" height="${size}" style="display:block;overflow:visible" focusable="false"`)
    .replace(/ role="img" aria-label="[^"]*"/, ' aria-hidden="true"');
  for (const [, id] of svg.matchAll(/id="([^"]+)"/g)) out = out.split(`id="${id}"`).join(`id="${id}-${uid}"`).split(`url(#${id})`).join(`url(#${id}-${uid})`);
  return out;
}

/** One mark, `size` px square (the sticker's white edge and shadow sit inside it). Decorative unless labelled. */
export function Mark(props: { svg: string; size: number; label?: string; className?: string }) {
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const html = useMemo(() => sized(props.svg, props.size, uid), [props.svg, props.size, uid]);
  return (
    <span
      className={`inline-block shrink-0 ${props.className ?? ""}`}
      style={{ width: props.size, height: props.size }}
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
