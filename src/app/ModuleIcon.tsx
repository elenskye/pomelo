// A module's icon where it is shown big enough for a sticker (模块坞, 柚子瓣, dragging): the sticker it wears in
// its color (moduleLooks.ts: what was picked, no two modules alike). Before a sync brings the stickers, a colored
// square with its Lucide icon.
import { HandIcon, type HandIconName } from "@/shared/handParts";
import { Mark, markSvg, stickerById } from "@/shared/marks";
import { ownMark, ownTint, resolveLook, type Look } from "./looks";
import { useModuleLooks } from "./moduleLooks";
import type { ModuleDef } from "./modules";

/** Places that aren't modules (设置) have no sticker: a hand-drawn round with their hand-drawn icon, like the
 *  desk's 贴纸簿 button (visual-atelier's frame prototype). */
const PLACE_ICONS: Record<string, HandIconName> = { settings: "gear", calendar: "cal" };

function PlaceIcon(props: { icon: HandIconName; size: number; className?: string }) {
  const d = Math.round(props.size * 0.84);
  return (
    <span aria-hidden className={`inline-grid shrink-0 place-items-center ${props.className ?? ""}`} style={{ width: props.size, height: props.size }}>
      <span className="relative grid place-items-center" style={{ width: d, height: d, color: "var(--color-pen)" }} data-hand="round" data-seed={751} data-fill="card">
        <HandIcon name={props.icon} size={Math.round(d * 0.6)} seed={18} className="relative" />
      </span>
    </span>
  );
}

export function ModuleIcon(props: { def: ModuleDef; size: number; /** Show this look instead of the saved one (a preview). */ look?: Look; className?: string }) {
  const { eff } = useModuleLooks();
  const place = PLACE_ICONS[props.def.id];
  if (place) return <PlaceIcon icon={place} size={props.size} className={props.className} />;
  let svg: string | null, tint;
  if (props.look) ({ svg, tint } = resolveLook(props.def, props.look));
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
