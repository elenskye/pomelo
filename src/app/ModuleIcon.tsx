// A module's icon where it is shown big enough for a sticker (模块坞, 柚子瓣, dragging): the sticker it wears in
// its color (moduleLooks.ts: what was picked, no two modules alike). Before a sync brings the stickers, a colored
// square with its Lucide icon.
import { Mark, markSvg, stickerById } from "@/shared/marks";
import { ownMark, ownTint, resolveLook, type Look } from "./looks";
import { useModuleLooks } from "./moduleLooks";
import type { ModuleDef } from "./modules";

/** The pages that aren't modules (设置, 日历) have a sticker of their own, drawn like the modules' (visual-atelier
 *  marks/places/); no「图标和颜色」for them. */
const PLACES = ["settings", "calendar"];

export function ModuleIcon(props: { def: ModuleDef; size: number; /** Show this look instead of the saved one (a preview). */ look?: Look; className?: string }) {
  const { eff } = useModuleLooks();
  let svg: string | null, tint;
  if (PLACES.includes(props.def.id)) [svg, tint] = [markSvg(`places/${props.def.id}`), null];
  else if (props.look) ({ svg, tint } = resolveLook(props.def, props.look));
  else if (props.def.id === "youzi") [svg, tint] = [markSvg("pomelo-normal"), null];
  else {
    const e = eff.get(props.def.id);
    svg = ((e && stickerById(e.mark)) ?? stickerById(ownMark(props.def)))?.svg ?? null;
    tint = e ? e.tint : ownTint(props.def);
  }
  if (svg) return <Mark svg={svg} size={props.size} tint={tint} className={props.className} />;
  const Icon = props.def.icon;
  return (
    <span className={`grid shrink-0 place-items-center text-white ${props.className ?? ""}`} style={{ width: props.size, height: props.size, borderRadius: Math.round(props.size * 0.3), background: props.def.color }}>
      <Icon size={props.size >= 36 ? 20 : props.size >= 24 ? 15 : 12} strokeWidth={1.75} />
    </span>
  );
}
