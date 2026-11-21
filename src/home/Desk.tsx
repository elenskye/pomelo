// 首页完整版的桌面 (docs/design.md「首页」, visual-atelier's canvases/pomelo-pages Desk / DeskRight): wider than
// the window (DESK_W). Drag the empty desk to slide it left and right (it coasts after letting go), or scroll sideways
// on the trackpad; when the window is too short for the desk it slides up and down too. Two dots at the bottom right
// jump to either half. The notes can be dragged and thrown: they coast and bounce off the desk's edges, and where
// each one rests is kept in settings `ui.desk` ({ notes: { id: [x, y] } }), so it's there next time. With 减少动态效果
// nothing coasts.
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { reducedMotion } from "@/shared/hand";
import "./desk.css";

export const DESK_W = 2560;
/** The desk's height when the window is shorter (the notes are laid out for it). */
export const DESK_H = 600;
const KEY = "ui.desk";
const QUERY = ["settings", KEY];
type Layout = { notes: Record<string, [number, number]> };
const isLayout = (v: unknown): v is Layout => !!v && typeof v === "object" && typeof (v as Layout).notes === "object";

type Ctx = { pos: Record<string, [number, number]>; place: (id: string, x: number, y: number) => void; size: () => [number, number]; top: () => number; moved: { current: boolean } };
const DeskCtx = createContext<Ctx | null>(null);

/** Elements inside a note that are used, not dragged. */
const LIVE = "button,a,input,textarea,select,label,[role=checkbox],[role=radio],[data-nodrag]";

export function Desk(props: { children: ReactNode }) {
  const qc = useQueryClient();
  const { data: saved } = useQuery({ queryKey: QUERY, queryFn: () => api.settingsGet<unknown>(KEY).then((v) => (isLayout(v) ? v : { notes: {} })) });
  const view = useRef<HTMLDivElement>(null);
  const wide = useRef<HTMLDivElement>(null);
  const off = useRef({ x: 0, y: 0 });
  const z = useRef(10);
  const moved = useRef(false);
  const [right, setRight] = useState(false);
  const [height, setHeight] = useState(DESK_H);
  const saveT = useRef(0);

  const limits = () => {
    const v = view.current;
    return v ? { x: Math.min(0, v.clientWidth - DESK_W), y: Math.min(0, v.clientHeight - Math.max(DESK_H, v.clientHeight)) } : { x: 0, y: 0 };
  };
  const set = useCallback((x: number, y: number) => {
    const l = limits();
    off.current = { x: Math.max(l.x, Math.min(0, x)), y: Math.max(l.y, Math.min(0, y)) };
    wide.current?.style.setProperty("transform", `translate(${Math.round(off.current.x)}px, ${Math.round(off.current.y)}px)`);
    setRight(off.current.x < l.x / 2);
  }, []);

  // the desk is as tall as the window allows (at least DESK_H); keep the offset inside it when the window changes
  useLayoutEffect(() => {
    const v = view.current;
    if (!v) return;
    const fit = () => {
      setHeight(Math.max(DESK_H, v.clientHeight));
      set(off.current.x, off.current.y);
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(v);
    return () => ro.disconnect();
  }, [set]);

  const glide = (x: number) => {
    if (reducedMotion()) return set(x, off.current.y);
    const from = off.current.x;
    const t0 = performance.now();
    const step = (now: number) => {
      const k = Math.min(1, (now - t0) / 420);
      set(from + (x - from) * (1 - Math.pow(1 - k, 3)), off.current.y);
      if (k < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || (e.target as Element).closest(`.dk-note,.dk-nav,${LIVE}`)) return;
    const v = view.current!;
    v.setPointerCapture(e.pointerId);
    v.classList.add("panning");
    const x0 = e.clientX - off.current.x;
    const y0 = e.clientY - off.current.y;
    let last = { x: e.clientX, y: e.clientY, t: performance.now() };
    let vel = { x: 0, y: 0 };
    const move = (ev: PointerEvent) => {
      const now = performance.now();
      const dt = Math.max(8, now - last.t);
      vel = { x: ((ev.clientX - last.x) / dt) * 16, y: ((ev.clientY - last.y) / dt) * 16 };
      last = { x: ev.clientX, y: ev.clientY, t: now };
      set(ev.clientX - x0, ev.clientY - y0);
    };
    const up = () => {
      v.removeEventListener("pointermove", move);
      v.removeEventListener("pointerup", up);
      v.removeEventListener("pointercancel", up);
      v.classList.remove("panning");
      if (reducedMotion()) return;
      const tick = () => {
        vel = { x: vel.x * 0.92, y: vel.y * 0.92 };
        const before = off.current;
        set(before.x + vel.x, before.y + vel.y);
        const stuck = off.current.x === before.x && off.current.y === before.y;
        if (!stuck && Math.abs(vel.x) + Math.abs(vel.y) > 0.3) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    };
    v.addEventListener("pointermove", move);
    v.addEventListener("pointerup", up);
    v.addEventListener("pointercancel", up);
  };

  // the trackpad: sideways moves the desk; up and down too when the desk is taller than the window, else sideways
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    const wheel = (e: WheelEvent) => {
      if ((e.target as Element).closest("[data-scroll]")) return;
      const tall = limits().y < 0;
      const dx = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : tall ? 0 : e.deltaY;
      const dy = tall && Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : 0;
      if (!dx && !dy) return;
      e.preventDefault();
      set(off.current.x - dx, off.current.y - dy);
    };
    v.addEventListener("wheel", wheel, { passive: false });
    return () => v.removeEventListener("wheel", wheel);
  }, [set]);

  const pos = saved?.notes ?? {};
  const place = (id: string, x: number, y: number) => {
    const next: Layout = { notes: { ...pos, [id]: [Math.round(x), Math.round(y)] } };
    qc.setQueryData(QUERY, next);
    clearTimeout(saveT.current);
    saveT.current = window.setTimeout(() => void api.settingsSet(KEY, next).catch(() => {}), 300);
  };
  const ctx: Ctx = { pos, place, size: () => [DESK_W, height], top: () => ++z.current, moved };

  return (
    <div ref={view} className="dk-view" onPointerDown={onPointerDown}>
      <div ref={wide} className="dk-wide" style={{ width: DESK_W, height }}>
        <DeskCtx.Provider value={ctx}>{saved && props.children}</DeskCtx.Provider>
      </div>
      <div className="dk-nav" role="group" aria-label="桌面的哪一边">
        <button type="button" className={`pm-press ${right ? "" : "on"}`} aria-label="桌面左边" onClick={() => glide(0)}>
          <span />
        </button>
        <button type="button" className={`pm-press ${right ? "on" : ""}`} aria-label="桌面右边" onClick={() => glide(limits().x)}>
          <span />
        </button>
      </div>
    </div>
  );
}

/** A note on the desk at its saved place (else x, y), w wide; `i`: its turn in the greeting (they fall in one by one). */
export function DeskNote(props: { id: string; x: number; y: number; w: number; i: number; label: string; children: ReactNode }) {
  const ctx = useContext(DeskCtx)!;
  const el = useRef<HTMLDivElement>(null);
  const [x, y] = ctx.pos[props.id] ?? [props.x, props.y];
  const [z, setZ] = useState(0);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || (e.target as Element).closest(LIVE)) return;
    const n = el.current!;
    e.preventDefault();
    n.setPointerCapture(e.pointerId);
    n.classList.add("drag");
    setZ(ctx.top());
    ctx.moved.current = false;
    const sx = e.clientX;
    const sy = e.clientY;
    let p = { x: 0, y: 0 };
    let last = { x: e.clientX, y: e.clientY, t: performance.now() };
    let v = { x: 0, y: 0 };
    const show = () => (n.style.transform = `translate(${p.x}px, ${p.y}px)`);
    const move = (ev: PointerEvent) => {
      p = { x: ev.clientX - sx, y: ev.clientY - sy };
      if (Math.abs(p.x) + Math.abs(p.y) > 3) ctx.moved.current = true;
      const now = performance.now();
      const dt = Math.max(8, now - last.t);
      v = { x: ((ev.clientX - last.x) / dt) * 16, y: ((ev.clientY - last.y) / dt) * 16 };
      last = { x: ev.clientX, y: ev.clientY, t: now };
      show();
    };
    const rest = () => {
      n.style.transform = "";
      n.classList.remove("drag");
      if (p.x || p.y) ctx.place(props.id, x + p.x, y + p.y);
      // a drag isn't a click on what's inside
      setTimeout(() => (ctx.moved.current = false), 0);
    };
    const up = () => {
      n.removeEventListener("pointermove", move);
      n.removeEventListener("pointerup", up);
      n.removeEventListener("pointercancel", up);
      if (reducedMotion() || !ctx.moved.current) return rest();
      const [W, H] = ctx.size();
      const w = n.offsetWidth;
      const h = n.offsetHeight;
      const tick = () => {
        p = { x: p.x + v.x, y: p.y + v.y };
        v = { x: v.x * 0.92, y: v.y * 0.92 };
        // bounce off the desk's edges
        if (x + p.x < 0) (p.x = -x), (v.x = Math.abs(v.x) * 0.6);
        if (x + p.x + w > W) (p.x = W - w - x), (v.x = -Math.abs(v.x) * 0.6);
        if (y + p.y < 0) (p.y = -y), (v.y = Math.abs(v.y) * 0.6);
        if (y + p.y + h > H) (p.y = H - h - y), (v.y = -Math.abs(v.y) * 0.6);
        p = { x: Math.round(p.x * 10) / 10, y: Math.round(p.y * 10) / 10 };
        show();
        if (Math.abs(v.x) + Math.abs(v.y) > 0.3) requestAnimationFrame(tick);
        else rest();
      };
      requestAnimationFrame(tick);
    };
    n.addEventListener("pointermove", move);
    n.addEventListener("pointerup", up);
    n.addEventListener("pointercancel", up);
  };

  return (
    <div
      ref={el}
      className="dk-note"
      role="group"
      aria-label={props.label}
      style={{ left: x, top: y, width: props.w, zIndex: z || undefined }}
      onPointerDown={onPointerDown}
      onClickCapture={(e) => ctx.moved.current && (e.stopPropagation(), e.preventDefault())}
    >
      <div className="pm-greet-note" style={{ "--pm-i": Math.min(props.i, 12) } as React.CSSProperties}>
        {props.children}
      </div>
    </div>
  );
}
