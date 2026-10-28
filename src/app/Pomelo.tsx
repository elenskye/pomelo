// The pomelo in the top bar (requirements「界面外框 · 柚子」): the user's four faces (平常、在想、开心、出错)
// and four more (眨眼、哇、困了、担心), with small animations (none with 减少动态效果).
// Which face: a status first (小拾 writing → 在想; done → 开心 and a line; something failed → 出错 and a
// line, until the pomelo is clicked), then the pointer over it (眨眼 or 哇), then the day's mood (the
// 今日简报's; without one, by the hour; late at night 困了).
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useLocation } from "react-router";
import { useBrief, usePickyStatus } from "@/modules/picky/api";

const PC = { rind: "#ebb36e", flesh: "#f8d694", core: "#f0c17c", leaf: "#8da872", gold: "#dcaa52", red: "#cd7865" };
type FaceDef = { core?: boolean; eyes: keyof typeof EYES; mouth: keyof typeof MOUTH; extra?: keyof typeof EXTRA };
export type Face = "idle" | "thinking" | "success" | "error" | "wink" | "wow" | "sleepy" | "worried";
const FACES: Record<Face, FaceDef> = {
  idle: { core: true, eyes: "dots", mouth: "smile" },
  thinking: { core: true, eyes: "pensive", mouth: "small", extra: "dots" },
  success: { eyes: "happy", mouth: "grin", extra: "sparkle" },
  error: { eyes: "worry", mouth: "frown", extra: "badge" },
  wink: { core: true, eyes: "wink", mouth: "smile" },
  wow: { eyes: "big", mouth: "o" },
  sleepy: { core: true, eyes: "closed", mouth: "dot", extra: "z" },
  worried: { eyes: "worry", mouth: "frown" },
};
const EYES = {
  dots: (
    <>
      <circle className="fill" cx="-13" cy="-7" r="4.9" />
      <circle className="fill" cx="13" cy="-7" r="4.9" />
    </>
  ),
  pensive: (
    <>
      <path d="M-18.5 -5 L-7.5 -8.8" />
      <path d="M7.5 -8.8 L18.5 -5" />
    </>
  ),
  happy: (
    <>
      <path d="M-20 -4 Q-13 -15 -6 -4" />
      <path d="M6 -4 Q13 -15 20 -4" />
    </>
  ),
  worry: (
    <>
      <path d="M-20 -5 L-7 -12.5" />
      <path d="M7 -12.5 L20 -5" />
    </>
  ),
  wink: (
    <>
      <circle className="fill" cx="-13" cy="-7" r="4.9" />
      <path d="M6 -5 Q13 -14 20 -5" />
    </>
  ),
  big: (
    <>
      <circle className="fill" cx="-13" cy="-7" r="6.2" />
      <circle className="fill" cx="13" cy="-7" r="6.2" />
      <circle className="hl" cx="-11" cy="-9.4" r="1.9" />
      <circle className="hl" cx="15" cy="-9.4" r="1.9" />
    </>
  ),
  closed: (
    <>
      <path d="M-19 -7 Q-13 -2 -7 -7" />
      <path d="M7 -7 Q13 -2 19 -7" />
    </>
  ),
};
const MOUTH = {
  smile: <path d="M-14 12 Q0 25 14 12" />,
  small: <path d="M-9 13 Q0 19.5 9 13" />,
  grin: <path d="M-13 11 Q0 37 13 11" />,
  frown: <path d="M-13 19 Q0 4 13 19" />,
  o: <ellipse className="fill" cx="0" cy="15" rx="5.6" ry="6.8" />,
  dot: <circle cx="0" cy="15" r="3.4" />,
};
const EXTRA = {
  dots: (
    <g className="x-dots" fill={PC.gold}>
      <circle cx="66" cy="-57" r="5.5" />
      <circle cx="79" cy="-76" r="3.6" />
    </g>
  ),
  sparkle: (
    <g className="x-spark" stroke={PC.gold} strokeWidth="4.4" strokeLinecap="round">
      <path d="M-62 -77.5V-51.5M-75 -64.5H-49" />
      <path d="M63.5 -25V-1M51.5 -13H75.5" />
    </g>
  ),
  badge: (
    <g className="x-badge">
      <circle cx="52" cy="-49" r="16" fill={PC.red} />
      <path d="M52 -58.5V-46" stroke="#fff" strokeWidth="5" strokeLinecap="round" />
      <circle cx="52" cy="-38.5" r="2.9" fill="#fff" />
    </g>
  ),
  z: (
    <g className="x-z" fill={PC.gold} fontWeight="700" fontFamily="ui-rounded, -apple-system, sans-serif">
      <text x="56" y="-48" fontSize="22">
        z
      </text>
      <text x="71" y="-67" fontSize="15">
        z
      </text>
    </g>
  ),
};

export function PomeloFace(props: { face: Face; size?: number }) {
  const f = FACES[props.face];
  const size = props.size ?? 38;
  return (
    // key: a new face starts its animation over
    <svg key={props.face} className={`pm f-${props.face}`} viewBox="-84 -84 168 168" width={size} height={size} aria-hidden>
      <g className="pb">
        <g className="leaf">
          <ellipse cx="19.5" cy="-58" rx="23" ry="10" transform="rotate(-22 19.5 -58)" fill={PC.leaf} />
        </g>
        <circle r="56" fill={PC.rind} />
        <circle r="46" fill={PC.flesh} />
        {f.core && <circle r="33" fill={PC.core} />}
        <g className="face">
          <g className="eyes">{EYES[f.eyes]}</g>
          {MOUTH[f.mouth]}
        </g>
      </g>
      {f.extra && EXTRA[f.extra]}
    </svg>
  );
}

// ---------- what went wrong in the background (backup, 发布, Pub2MD…): the pomelo says it ----------

let problem: string | null = null;
const listeners = new Set<() => void>();
/** Something failed out of sight: the pomelo shows 出错 and says it, until clicked. */
export function reportProblem(text: string) {
  problem = text;
  listeners.forEach((l) => l());
}
export function clearProblem() {
  if (problem == null) return;
  problem = null;
  listeners.forEach((l) => l());
}
function useProblem() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => problem,
  );
}

/** Work that goes on after its page is left (发布, Pub2MD): `check` says null while it runs, "" when it
 * ended fine, or what went wrong. Checked every 2 seconds from the frame until it ends. */
const watches = new Map<string, () => Promise<string | null>>();
export function watchJob(key: string, check: () => Promise<string | null>) {
  watches.set(key, check);
}
export function useJobWatch() {
  useEffect(() => {
    const t = setInterval(() => {
      watches.forEach((check, key) =>
        void check()
          .then((r) => {
            if (r == null) return;
            watches.delete(key);
            if (r) reportProblem(r);
          })
          .catch(() => watches.delete(key)),
      );
    }, 2000);
    return () => clearInterval(t);
  }, []);
}

const BRIEF_FACE: Record<string, Face> = { calm: "idle", busy: "thinking", good: "success", worry: "worried" };
const BY_HOUR: Record<string, Face[]> = { am: ["idle", "wow", "wink"], pm: ["idle", "thinking"], eve: ["idle", "wink"] };
const pick = <T,>(a: T[]) => a[Math.floor(Math.random() * a.length)];
const partOfDay = (h: number) => (h >= 23 || h < 6 ? "night" : h < 12 ? "am" : h < 18 ? "pm" : "eve");

/** The day's mood: today's brief, else a pick for this part of the day (late at night: sleepy). */
function useMood(): Face {
  const brief = useBrief(true).data;
  const [part, setPart] = useState(() => partOfDay(new Date().getHours()));
  const [byHour, setByHour] = useState<Face>(() => (part === "night" ? "sleepy" : pick(BY_HOUR[part])));
  useEffect(() => {
    const t = setInterval(() => setPart(partOfDay(new Date().getHours())), 60_000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => setByHour(part === "night" ? "sleepy" : pick(BY_HOUR[part])), [part]);
  if (part === "night") return "sleepy";
  return (brief?.mood && BRIEF_FACE[brief.mood]) || byHour;
}

/** 小拾 writing → thinking; when it stops → success for a moment (with a line when not on 小拾's page). */
function useWriting(): { face: Face | null; line: string | null } {
  const generating = !!usePickyStatus().data?.generating;
  const onPicky = useLocation().pathname.startsWith("/picky");
  const [done, setDone] = useState<string | null>(null);
  const was = useRef(false);
  useEffect(() => {
    if (generating) {
      setDone(null);
    } else if (was.current) {
      setDone(onPicky ? "" : "小拾写好了");
      const t = setTimeout(() => setDone(null), 2800);
      was.current = false;
      return () => clearTimeout(t);
    }
    was.current = generating;
  }, [generating]); // eslint-disable-line react-hooks/exhaustive-deps
  if (generating) return { face: "thinking", line: null };
  if (done != null) return { face: "success", line: done || null };
  return { face: null, line: null };
}

export function PomeloButton(props: { open: boolean; onClick: (el: HTMLElement) => void }) {
  const mood = useMood();
  const writing = useWriting();
  const trouble = useProblem();
  const [hover, setHover] = useState<Face | null>(null);
  const status: { face: Face; line: string | null; error?: boolean } | null = trouble ? { face: "error", line: trouble, error: true } : writing.face ? { face: writing.face, line: writing.line } : null;
  const face = status?.face ?? hover ?? mood;
  // a line pops up for a while (an error's stays while hovered)
  const [said, setSaid] = useState<string | null>(null);
  useEffect(() => {
    if (!status?.line) return setSaid(null);
    setSaid(status.line);
    const t = setTimeout(() => setSaid(null), status.error ? 4000 : 2800);
    return () => clearTimeout(t);
  }, [status?.line]); // eslint-disable-line react-hooks/exhaustive-deps
  const line = said ?? (hover && trouble ? trouble : null);
  return (
    <button
      type="button"
      aria-label="柚子瓣"
      aria-expanded={props.open}
      onClick={(e) => {
        clearProblem();
        props.onClick(e.currentTarget);
      }}
      onMouseEnter={() => setHover(Math.random() < 0.65 ? "wink" : "wow")}
      onMouseLeave={() => setHover(null)}
      className={`relative grid size-10 shrink-0 place-items-center rounded-[10px] outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring ${props.open ? "bg-black/[0.045] dark:bg-white/[0.06]" : "hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"}`}
    >
      <PomeloFace face={face} />
      {line && (
        <span
          role="status"
          className={`pm-bubble pointer-events-none absolute top-[42px] left-0.5 z-50 rounded-[4px_12px_12px_12px] border bg-card px-3 py-1 text-[13px] whitespace-nowrap text-foreground shadow-lg ${status?.error ? "border-[color-mix(in_srgb,#cd7865_55%,var(--border))]" : ""}`}
        >
          {line}
        </span>
      )}
    </button>
  );
}
