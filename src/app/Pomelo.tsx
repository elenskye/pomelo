// The pomelo in the top bar (requirements「界面外框 · 柚子」): the user's four faces (平常、在想、开心、出错)
// and four more (眨眼、哇、困了、惊讶), drawn in visual-atelier, animated by ds.css (none with 减少动态效果).
// Which face: a status first (柚子 writing → 在想; done → 开心 and a line; something failed → 出错 and a
// line, until the pomelo is clicked), then the pointer over it (眨眼 or 哇), then the mood (the 今日简报's
// for an hour after it's done; otherwise a random one that isn't sad, picked each hour; late at night 困了).
import { useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { useLocation } from "react-router";
import { useBrief, useYouziStatus } from "@/modules/youzi/api";
import { Tip } from "@/shared/handParts";
import { markSvg, sizedMark } from "@/shared/marks";

export type Face = "idle" | "thinking" | "success" | "error" | "wink" | "wow" | "sleepy" | "worried" | "surprised";
/** Pomelo's faces → the drawn ones synced from visual-atelier (marks/pomelo-*.svg). */
const MARK_FACE: Record<Face, string> = { idle: "normal", thinking: "think", success: "happy", error: "error", wink: "wink", wow: "wow", sleepy: "sleepy", worried: "worried", surprised: "surprised" };

/**
 * 柚子 with a face (visual-atelier's 8 drawn faces) and its animation from ds.css (the Design System's motion parts
 * pomelo-*: 平常 breathes and looks around, 在想 and 困了 loop, the others play once when the face appears; with
 * 减弱动态效果 none moves). A new face is a new element, so its animation starts over. `dizzy`: the 连戳 easter egg.
 */
export function PomeloFace(props: { face: Face; size?: number; dizzy?: boolean; className?: string }) {
  const mark = MARK_FACE[props.face];
  const size = props.size ?? 38;
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const svg = markSvg(`pomelo-${mark}`);
  const html = useMemo(() => (svg ? sizedMark(svg, size, uid) : ""), [svg, size, uid]);
  return <span key={props.face} aria-hidden className={`pm-pomelo ${props.dizzy ? "is-dizzy" : ""} ${props.className ?? ""}`} data-face={mark} style={{ width: size, height: size }} dangerouslySetInnerHTML={{ __html: html }} />;
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

const BRIEF_FACE: Record<string, Face> = { calm: "idle", busy: "thinking", good: "success", worry: "worried", plain: "idle" };
/** The brief's face stays this long after it's done. */
const BRIEF_MS = 3600_000;
/** Picked each hour otherwise: none sad (and not 在想, which says 柚子 is writing). */
const RANDOM: Face[] = ["idle", "wink", "wow", "success"];
const pick = <T,>(a: T[]) => a[Math.floor(Math.random() * a.length)];
const isNight = (h: number) => h >= 23 || h < 6;

/** The mood: today's brief's for an hour after it's done, else a random one each hour (late at night: sleepy). */
function useMood(): Face {
  const brief = useBrief(true).data;
  const [now, setNow] = useState(() => Date.now());
  const hour = new Date(now).getHours();
  const [random, setRandom] = useState<Face>(() => pick(RANDOM));
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(t);
  }, []);
  useEffect(() => setRandom(pick(RANDOM)), [hour]);
  if (isNight(hour)) return "sleepy";
  const at = brief?.mood && brief.moodAt ? Date.parse(brief.moodAt) : NaN;
  return (now - at < BRIEF_MS && BRIEF_FACE[brief!.mood!]) || random;
}

/** 柚子 writing → thinking; when it stops → success for a moment (with a line when not on 柚子's page). */
function useWriting(): { face: Face | null; line: string | null } {
  const status = useYouziStatus().data;
  const generating = !!status?.generating;
  const asking = !!status?.active.some((a) => a.asking);
  const onYouzi = useLocation().pathname.startsWith("/youzi");
  const [done, setDone] = useState<string | null>(null);
  const was = useRef(false);
  useEffect(() => {
    if (generating) {
      setDone(null);
    } else if (was.current) {
      setDone(onYouzi ? "" : "柚子写好了");
      const t = setTimeout(() => setDone(null), 2800);
      was.current = false;
      return () => clearTimeout(t);
    }
    was.current = generating;
  }, [generating]); // eslint-disable-line react-hooks/exhaustive-deps
  if (asking) return { face: "wow", line: onYouzi ? null : "柚子在等你回答" };
  if (generating) return { face: "thinking", line: null };
  if (done != null) return { face: "success", line: done || null };
  return { face: null, line: null };
}

/** 小彩蛋 (Phase 2): 20 seconds without moving the mouse or typing, 柚子 dozes off; the next move wakes it with a start. */
function useDoze(): Face | null {
  const [face, setFace] = useState<Face | null>(null);
  const now = useRef(face);
  now.current = face;
  useEffect(() => {
    let last = Date.now();
    let wake = 0;
    const onMove = () => {
      last = Date.now();
      if (now.current !== "sleepy") return;
      setFace("surprised");
      clearTimeout(wake);
      wake = window.setTimeout(() => setFace(null), 900);
    };
    const tick = setInterval(() => {
      if (Date.now() - last > 20_000 && now.current == null && !document.querySelector("dialog[open]")) setFace("sleepy");
    }, 2000);
    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("keydown", onMove);
    return () => {
      clearInterval(tick);
      clearTimeout(wake);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("keydown", onMove);
    };
  }, []);
  return face;
}

export function PomeloButton(props: { open: boolean; onClick: (el: HTMLElement) => void; face?: Face | null }) {
  const mood = useMood();
  const writing = useWriting();
  const trouble = useProblem();
  const doze = useDoze();
  const [hover, setHover] = useState<Face | null>(null);
  const status: { face: Face; line: string | null; error?: boolean } | null = trouble ? { face: "error", line: trouble, error: true } : writing.face ? { face: writing.face, line: writing.line } : null;
  // what the frame asks for (哇 when the 柚子瓣 / 贴纸簿 opens, 开心 on a page, 眨眼 at the desk) comes after a status
  const face = status?.face ?? props.face ?? hover ?? doze ?? mood;
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
      aria-label="柚子瓣：去哪，或问柚子"
      aria-expanded={props.open}
      onClick={(e) => {
        clearProblem();
        props.onClick(e.currentTarget);
      }}
      onMouseEnter={() => setHover(Math.random() < 0.65 ? "wink" : "wow")}
      onMouseLeave={() => setHover(null)}
      className={`hub pm-press ${line ? "" : "pm-tip-host"} ${props.open ? "on" : ""}`}
    >
      <span style={{ transform: "rotate(-8deg)", display: "inline-flex" }}>
        <PomeloFace face={face} size={44} />
      </span>
      {!line && <Tip>柚子瓣</Tip>}
      {line && (
        <span
          role="status"
          className={`pm-bubble pointer-events-none absolute top-[48px] left-0.5 z-50 rounded-[4px_12px_12px_12px] border bg-card px-3 py-1 text-[13px] whitespace-nowrap text-foreground shadow-lg ${status?.error ? "border-[color-mix(in_srgb,var(--color-danger)_55%,var(--border))]" : ""}`}
        >
          {line}
        </span>
      )}
    </button>
  );
}
