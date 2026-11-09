// Small hand-drawn pieces of the frame, the same as visual-atelier's frame prototype (canvases/pomelo-frame):
// one-stroke icons, the tooltip hanging on a string (Design System
// Tooltip; it swings, motion part tooltip), and a strip of tape (mint or yuzu stripes, zigzag ends).
// The frame's icons are the UI icons now (shared/icons.tsx, drawn in visual-atelier), under the short names the frame
// has always used; nothing is painted at run time for them.
import { useMemo, type CSSProperties, type ReactNode } from "react";
import { BackDesk, CalendarDays, Check, ChevronDown, ChevronLeft, ChevronRight, Flip, LayoutGrid, Plus, Search, Settings, Sparkle, X, type IconType } from "./icons";

const f = (n: number) => +n.toFixed(2);

const ICONS = { back: ChevronLeft, fwd: ChevronRight, plus: Plus, x: X, search: Search, gear: Settings, back2: BackDesk, sparkle: Sparkle, grid: LayoutGrid, down: ChevronDown, check: Check, flip: Flip, cal: CalendarDays } satisfies Record<string, IconType>;
export type HandIconName = keyof typeof ICONS;

/** `seed` is from when each icon was drawn in the page (kept so callers needn't change): the line is the icon's own now. */
export function HandIcon(props: { name: HandIconName; size?: number; seed?: number; className?: string }) {
  const Icon = ICONS[props.name];
  return (
    <span aria-hidden className={`inline-flex shrink-0 ${props.className ?? ""}`}>
      <Icon size={props.size ?? 20} />
    </span>
  );
}

/** The tooltip on a string under an icon-only button or a sticker: put it last inside an element with `pm-tip-host`. */
export function Tip(props: { children: ReactNode; style?: CSSProperties }) {
  return (
    <span className="pm-tip" role="tooltip" style={props.style}>
      <span className="pm-tip-string" />
      <span className="pm-tip-tag">{props.children}</span>
    </span>
  );
}

/** A strip of tape filling its (positioned) parent: only the strip tilts, put the words in a layer above it. */
export function Tape(props: { kind?: "mint" | "yuzu"; rot?: number }) {
  const zz: string[] = [];
  for (let i = 0; i <= 8; i++) zz.push(`${i % 2 ? 6 : 0}px ${f((i / 8) * 100)}%`);
  const right = zz.map((p) => { const [x, y] = p.split(" "); return `calc(100% - ${x}) ${y}`; }).reverse();
  const [a, b] = props.kind === "yuzu" ? ["tape-yuzu-a", "tape-yuzu-b"] : ["tape-mint-a", "tape-mint-b"];
  return (
    <span
      aria-hidden
      className="pointer-events-none absolute inset-0"
      style={{ transform: `rotate(${props.rot ?? -2}deg)`, opacity: 0.88, clipPath: `polygon(${[...zz, ...right].join(",")})`, background: `repeating-linear-gradient(45deg,var(--color-${a}) 0 7px,var(--color-${b}) 7px 14px)` }}
    />
  );
}

type Hand = { P: (d: string) => unknown; handStroke: (c: unknown, o: object) => string };
/** 「柚子在想…」: a little curl drawn, held, wiped and drawn again, 1.6 s round, in step with 柚子's 在想 face
 *  (motion part think-line; the same stroke as the prototype's). */
export function ThinkLine(props: { children?: ReactNode; className?: string }) {
  const d = useMemo(() => {
    const raw = "M10 30 C4 22 12 12 20 18 C28 24 22 34 16 28 C10 20 20 6 32 10 C40 13 40 22 34 22";
    const h = (window as unknown as { Pomelo?: { hand?: Hand } }).Pomelo?.hand;
    return h ? h.handStroke(h.P(raw), { amp: 0.6, wave: 10, seed: 3, sp: 1 }) : raw;
  }, []);
  return (
    <span className={`inline-flex items-center gap-2 text-sm text-muted-foreground ${props.className ?? ""}`}>
      <svg className="pm-think-line" width="34" height="31" viewBox="0 0 44 40" style={{ overflow: "visible" }} aria-hidden>
        <path pathLength={1} d={d} fill="none" stroke="var(--color-pen)" strokeWidth="2.4" strokeLinecap="round" />
      </svg>
      {props.children}
    </span>
  );
}
