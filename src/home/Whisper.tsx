// 小彩蛋 (Phase 2): the pomelo sticker on the 今日简报 card's corner can be peeled. Click it and it flips up, showing its
// white back and, underneath, the mark it left (marks/pomelo-ghost.svg) with a whisper written in it; after a moment it
// falls back like a dropped sticker and pats flat, and 柚子 winks. Drag it and it peels the way you pull; let go and it
// falls back. Motion: ds.css (sticker-whisper).
import { useId, useMemo, useRef } from "react";
import { flash, reducedMotion } from "@/shared/hand";
import { markSvg, sizedMark } from "@/shared/marks";

/** Ask the top bar's pomelo for a face for a moment (App listens). */
export const pomeloFace = (face: string, ms: number) => window.dispatchEvent(new CustomEvent("pomelo:face", { detail: { face, ms } }));

function useMark(name: string, size: number) {
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const svg = markSvg(name);
  return useMemo(() => (svg ? sizedMark(svg, size, uid) : ""), [svg, size, uid]);
}

export function Whisper(props: { size?: number; className?: string }) {
  const size = props.size ?? 72;
  const ghost = useMark("pomelo-ghost", size);
  const front = useMark("pomelo-normal", size);
  const back = useMark("pomelo-normal", size);
  const peel = useRef<HTMLDivElement>(null);
  const upT = useRef(0);
  if (!ghost || !front) return null;

  const fall = (ax: number, ay: number, a: number, origin: string) => {
    const el = peel.current;
    if (!el) return;
    clearTimeout(upT.current);
    el.classList.remove("is-up");
    el.style.transform = "";
    el.style.transformOrigin = "";
    if (reducedMotion() || a < 1) return el.classList.remove("is-falling");
    el.style.setProperty("--pm-ax", ax.toFixed(3));
    el.style.setProperty("--pm-ay", ay.toFixed(3));
    el.style.setProperty("--pm-a", a.toFixed(1));
    el.style.setProperty("--pm-origin", origin);
    const ms = 380 + a * 2.4;
    flash(el, "is-falling", ms + 40);
    setTimeout(() => pomeloFace("wink", 900), Math.round(ms * 0.58));
  };
  const up = () => {
    const el = peel.current;
    if (!el) return;
    clearTimeout(upT.current);
    el.classList.remove("is-falling");
    flash(el, "is-up");
    upT.current = window.setTimeout(() => fall(1, 0, 150, "50% 0"), 2400);
  };
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const x0 = e.clientX, y0 = e.clientY;
    let pose: { ax: number; ay: number; a: number; origin: string } | null = null;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0, dy = ev.clientY - y0, len = Math.hypot(dx, dy);
      if (len < 4) return;
      clearTimeout(upT.current);
      el.classList.remove("is-up", "is-falling");
      pose = { ax: -dy / len, ay: dx / len, a: Math.min(165, len * 1.7), origin: `${dx < 0 ? "0%" : "100%"} ${dy < 0 ? "0%" : "100%"}` };
      el.style.transformOrigin = pose.origin;
      el.style.transform = `perspective(420px) rotate3d(${pose.ax.toFixed(3)}, ${pose.ay.toFixed(3)}, 0, ${pose.a.toFixed(1)}deg)`;
    };
    const done = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", done);
      el.removeEventListener("pointercancel", done);
      if (pose) fall(pose.ax, pose.ay, pose.a, pose.origin);
      else if (el.classList.contains("is-up")) fall(1, 0, 150, "50% 0");
      else up();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", done);
    el.addEventListener("pointercancel", done);
  };
  return (
    <div className={`pm-whisper ${props.className ?? ""}`} style={{ width: size, height: size }}>
      <div className="pm-whisper-under">
        <span dangerouslySetInnerHTML={{ __html: ghost }} />
        <div className="pm-whisper-text">
          今天也
          <br />
          辛苦啦
        </div>
      </div>
      <div
        ref={peel}
        className="pm-whisper-peel"
        role="button"
        tabIndex={0}
        aria-label="撕起柚子贴纸"
        onPointerDown={onPointerDown}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            up();
          }
        }}
      >
        <div className="pm-whisper-front" dangerouslySetInnerHTML={{ __html: front }} />
        <div className="pm-whisper-back" dangerouslySetInnerHTML={{ __html: back }} />
      </div>
    </div>
  );
}
