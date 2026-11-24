// A branch's children in a side column (标签树、文件夹、分组): opened or closed by hand they slide in one after another /
// go back up, and the rows below follow (motion part fold: .pm-fold + is-open / is-closing, --pm-i / --pm-r on each
// child, set here; the height is measured here). Fold is the children's own box (no wrappers), so it can also be a
// sortable list (`boxRef`). Only a hand's toggle moves: the toggle calls foldByHand() first, so a branch opening as
// settings or data load (or following the picked tag) just shows. Reduced motion: no motion.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { reducedMotion } from "./hand";

const OPEN_MS = 600;
const CLOSE_MS = 320;
let handAt = -Infinity;

/** The next open / close (now) is by hand: it moves. */
export const foldByHand = () => {
  handAt = performance.now();
};
const byHand = () => performance.now() - handAt < 400 && !reducedMotion();

export function Fold(props: { open: boolean; children: ReactNode; className?: string; boxRef?: RefObject<HTMLDivElement | null> }) {
  const own = useRef<HTMLDivElement>(null);
  const ref = props.boxRef ?? own;
  const timers = useRef<number[]>([]);
  const [prev, setPrev] = useState(props.open);
  const [leaving, setLeaving] = useState(false);
  if (prev !== props.open) {
    setPrev(props.open);
    setLeaving(!props.open && byHand());
  }
  const mounted = useRef(false);

  useLayoutEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    timers.current.forEach(clearTimeout);
    timers.current = [];
    const el = ref.current;
    if (!el) return;
    const moving = el.classList.contains("is-open") || el.classList.contains("is-closing");
    if (!moving && !byHand()) return;
    const later = (ms: number, f: () => void) => timers.current.push(window.setTimeout(f, ms));
    const rows = [...el.children] as HTMLElement[];
    rows.forEach((r, i) => {
      r.style.setProperty("--pm-i", String(i));
      r.style.setProperty("--pm-r", String(rows.length - 1 - i));
    });
    if (props.open) {
      const from = el.classList.contains("is-closing") ? el.getBoundingClientRect().height : 0;
      el.classList.remove("is-open", "is-closing");
      el.style.height = "auto";
      const to = el.scrollHeight;
      el.style.height = `${from}px`;
      void el.offsetWidth;
      el.classList.add("is-open");
      el.style.height = `${to}px`;
      later(OPEN_MS, () => {
        el.classList.remove("is-open");
        el.style.height = "";
      });
    } else {
      el.style.height = `${el.getBoundingClientRect().height}px`;
      el.classList.remove("is-open");
      void el.offsetWidth;
      el.classList.add("is-closing");
      const lag = Math.min(rows.length - 1, 9) * 10;
      later(lag, () => (el.style.height = "0px"));
      later(lag + CLOSE_MS, () => {
        el.classList.remove("is-closing");
        el.style.height = "";
        setLeaving(false);
      });
    }
  }, [props.open]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => timers.current.forEach(clearTimeout), []);

  if (!props.open && !leaving) return null;
  return (
    <div ref={ref} className={`pm-fold ${props.className ?? ""}`}>
      {props.children}
    </div>
  );
}
