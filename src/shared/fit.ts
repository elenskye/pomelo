// A box floating by a piece of text or a button (柚子续写 / 润色, 记到念念不忘, 柚子链接…): placed after it is measured and
// again whenever it changes size, so it stays inside the window however long it grows — under its anchor when it fits,
// else above it, else against the bottom edge; pushed in from the sides; never taller than the window (`.pm-float`'s
// inner part scrolls). `useDragFit`: a card (柚子续写 / 润色, 记到念念不忘) that can also be moved by its title or any
// empty part of it; once moved it stays where it was put (pushed back inside the window when it or the window changes).
import { useLayoutEffect, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent, type RefObject } from "react";

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

type Pos = { left: number; top: number };

/** The box's `left` / `top` (hidden until measured); `pinned`: put there instead of by the anchor. */
export function useFit(box: RefObject<HTMLElement | null>, at: Anchor, pinned?: Pos | null): CSSProperties {
  const [pos, setPos] = useState<Pos | null>(null);
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const place = () => {
      const [w, h, vw, vh] = [el.offsetWidth, el.offsetHeight, window.innerWidth, window.innerHeight];
      const inX = (x: number) => Math.max(EDGE, Math.min(x, vw - EDGE - w));
      const left = inX(pinned ? pinned.left : at.end ? at.left - w : at.left);
      const top = pinned
        ? Math.max(EDGE, Math.min(pinned.top, vh - EDGE - h))
        : at.bottom + GAP + h <= vh - EDGE ? at.bottom + GAP : at.top - GAP - h >= EDGE ? at.top - GAP - h : Math.max(EDGE, vh - EDGE - h);
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
  }, [box, at.left, at.top, at.bottom, at.end, pinned?.left, pinned?.top]);
  return pos ?? { left: at.left, top: at.bottom + GAP, visibility: "hidden" };
}

/** What a press on the card starts no drag from: its controls, its text boxes, text meant to be selected. */
const KEEP = "button, a, input, textarea, select, label, [contenteditable=\"\"], [contenteditable=true], .selectable, [data-no-drag]";

/** useFit, and `grab` (the card's onMouseDown) moves the card: from its title or any empty part, not from a control,
 *  a text box, selectable text or a scrollbar. Mouse events (like sortable), the text under it stays unselected. */
export function useDragFit(box: RefObject<HTMLElement | null>, at: Anchor) {
  const [pinned, setPinned] = useState<Pos | null>(null);
  const style = useFit(box, at, pinned);
  const live = useRef<(() => void) | null>(null);
  const grab = (e: ReactMouseEvent) => {
    e.stopPropagation(); // a press in the card is not a press on the page under it (the editor keeps its selection)
    const t = e.target as HTMLElement;
    const el = box.current;
    if (e.button !== 0 || !el || t.closest(KEEP)) return;
    // the scrollbar of a box that scrolls inside the card
    if (t.scrollHeight > t.clientHeight && e.nativeEvent.offsetX >= t.clientWidth) return;
    e.preventDefault();
    const r = el.getBoundingClientRect();
    const [x0, y0] = [e.clientX, e.clientY];
    el.classList.add("is-dragging");
    const move = (m: MouseEvent) => setPinned({ left: r.left + m.clientX - x0, top: r.top + m.clientY - y0 });
    const up = () => {
      el.classList.remove("is-dragging");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      live.current = null;
    };
    live.current?.();
    live.current = up;
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  useLayoutEffect(() => () => live.current?.(), []);
  return { style, grab };
}
