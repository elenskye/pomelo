// Small hand-drawn pieces of the frame, the same as visual-atelier's frame prototype (canvases/pomelo-frame):
// one-stroke icons (drawn by shared/hand.ts from data-hand="icon"), the tooltip hanging on a string (Design System
// Tooltip; it swings, motion part tooltip), and a strip of tape (mint or yuzu stripes, zigzag ends).
// The frame's icons stay these until Dawn Plan · Phase 8 draws all of the UI icons.
import { useMemo, type CSSProperties, type ReactNode } from "react";

const f = (n: number) => +n.toFixed(2);
const gear = Array.from({ length: 8 }, (_, i) => {
  const a = (i * Math.PI) / 4;
  return `M${f(12 + 7 * Math.cos(a))} ${f(12 + 7 * Math.sin(a))} L${f(12 + 9.6 * Math.cos(a))} ${f(12 + 9.6 * Math.sin(a))}`;
}).join(" ");

/** 24-unit one-stroke paths; `o` adds rings (cx cy r). */
const ICONS = {
  back: { d: "M15 5.5 L8.5 12 L15 18.5" },
  fwd: { d: "M9 5.5 L15.5 12 L9 18.5" },
  plus: { d: "M12 5 L12 19 M5 12 L19 12" },
  x: { d: "M6.5 6.5 L17.5 17.5 M17.5 6.5 L6.5 17.5" },
  search: { d: "M16 16 L20.5 20.5", o: "10.5 10.5 6" },
  gear: { d: gear, o: "12 12 3, 12 12 6.6" },
  back2: { d: "M9.5 6 L4.5 11 L9.5 16 M4.5 11 L14.5 11 C18 11 20 13 20 16 L20 19" },
  sparkle: { d: "M12 4 Q12 12 20 12 Q12 12 12 20 Q12 12 4 12 Q12 12 12 4 Z" },
  grid: { d: "M5 5 L10.5 5 L10.5 10.5 L5 10.5 Z M13.5 5 L19 5 L19 10.5 L13.5 10.5 Z M5 13.5 L10.5 13.5 L10.5 19 L5 19 Z M13.5 13.5 L19 13.5 L19 19 L13.5 19 Z" },
  down: { d: "M7 10 L12 15 L17 10" },
  check: { d: "M5.5 12.5 L10 17 L18.5 7" },
} as const;
export type HandIconName = keyof typeof ICONS;

export function HandIcon(props: { name: HandIconName; size?: number; seed?: number; className?: string }) {
  const ic = ICONS[props.name] as { d: string; o?: string };
  return <span aria-hidden className={`inline-flex shrink-0 ${props.className ?? ""}`} data-hand="icon" data-size={props.size ?? 20} data-seed={props.seed ?? 1} data-d={ic.d} data-o={ic.o} />;
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
