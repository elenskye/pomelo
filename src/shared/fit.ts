// A box floating by a piece of text or a button (柚子续写 / 润色, 记到念念不忘, 柚子链接…): placed after it is measured and
// again whenever it changes size, so it stays inside the window however long it grows — under its anchor when it fits,
// else above it, else against the bottom edge; pushed in from the sides; never taller than the window (`.pm-float`'s
// inner part scrolls).
import { useLayoutEffect, useState, type CSSProperties, type RefObject } from "react";

/** Where a floating box hangs: the line it belongs to (`top`–`bottom`) and its left edge (`end`: its right edge). */
export type Anchor = { left: number; top: number; bottom: number; end?: boolean };

const EDGE = 12;
const GAP = 8;

/** The selection's (or the caret's) line. */
export function selectionAnchor(): Anchor {
  const sel = window.getSelection();
  const r = sel && sel.rangeCount ? sel.getRangeAt(0).getBoundingClientRect() : null;
  return r && (r.width || r.height) ? { left: r.left - 8, top: r.top, bottom: r.bottom } : { left: 200, top: 190, bottom: 210 };
}

/** Under / over a button, its right edges lined up with the box's. */
export const buttonAnchor = (el: Element): Anchor => {
  const r = el.getBoundingClientRect();
  return { left: r.right, top: r.top, bottom: r.bottom, end: true };
};

/** The box's `left` / `top` (hidden until measured). */
export function useFit(box: RefObject<HTMLElement | null>, at: Anchor): CSSProperties {
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const place = () => {
      const [w, h, vw, vh] = [el.offsetWidth, el.offsetHeight, window.innerWidth, window.innerHeight];
      const left = Math.max(EDGE, Math.min(at.end ? at.left - w : at.left, vw - EDGE - w));
      const top = at.bottom + GAP + h <= vh - EDGE ? at.bottom + GAP : at.top - GAP - h >= EDGE ? at.top - GAP - h : Math.max(EDGE, vh - EDGE - h);
      setPos((p) => (p && p.left === left && p.top === top ? p : { left, top }));
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(el);
    window.addEventListener("resize", place);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", place);
    };
  }, [box, at.left, at.top, at.bottom, at.end]);
  return pos ?? { left: at.left, top: at.bottom + GAP, visibility: "hidden" };
}
