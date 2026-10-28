// Dragging modules between the 模块坞's zones, the 索引签 and the 柚子瓣 — mouse events, not the browser's
// drag and drop (CLAUDE.md). Press on a module and move a few pixels: a chip follows the pointer; the place
// under it is the zone (`data-drop`: pinned | hidden | cat:<id> | radial) and the module row under it
// (`data-mod`) is where it lands before. The 柚子瓣 answers for its wedges itself (`radialDrop`).
import { useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { moduleById } from "./modules";

export type Over = { zone: string; before?: string };
export type DragNow = { mod: string; from: string; x: number; y: number; over: Over | null };

let now: DragNow | null = null;
const listeners = new Set<() => void>();
const set = (d: DragNow | null) => {
  now = d;
  listeners.forEach((l) => l());
};

/** The 柚子瓣's own hit test (set while it is open). */
export const radialDrop: { hit: ((x: number, y: number) => Over | null) | null } = { hit: null };

function resolve(x: number, y: number): Over | null {
  const el = document.elementFromPoint(x, y);
  const z = el?.closest<HTMLElement>("[data-drop]");
  if (!el || !z) return null;
  if (z.dataset.drop === "radial") return radialDrop.hit?.(x, y) ?? null;
  const t = el.closest<HTMLElement>("[data-mod]");
  return { zone: z.dataset.drop!, before: t && z.contains(t) ? t.dataset.mod : undefined };
}

export function useDragNow(): DragNow | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => now,
  );
}

/** Starts watching a press on a module; a real drag ends in `drop`, a plain click stays a click. */
export function pressModule(e: React.PointerEvent, mod: string, from: string, drop: (mod: string, zone: string, before: string | undefined, from: string) => void) {
  if (e.button !== 0) return;
  const x0 = e.clientX,
    y0 = e.clientY;
  let started = false;
  const move = (ev: PointerEvent) => {
    if (!started) {
      if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
      started = true;
    }
    set({ mod, from, x: ev.clientX, y: ev.clientY, over: resolve(ev.clientX, ev.clientY) });
  };
  const end = (ev: PointerEvent | null) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("keydown", esc, true);
    if (!started) return;
    const over = ev ? resolve(ev.clientX, ev.clientY) : null;
    set(null);
    // the click that ends a drag isn't a click
    const swallow = (c: MouseEvent) => {
      c.stopPropagation();
      c.preventDefault();
    };
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true));
    if (over) drop(mod, over.zone, over.before, from);
  };
  const up = (ev: PointerEvent) => end(ev);
  const cancel = () => end(null);
  const esc = (ev: KeyboardEvent) => {
    if (ev.key !== "Escape" || !started) return;
    ev.stopPropagation();
    end(null);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("keydown", esc, true);
}

/** The chip under the pointer while dragging. */
export function DragChip() {
  const d = useDragNow();
  if (!d) return null;
  const m = moduleById(d.mod);
  const Icon = m.icon;
  return createPortal(
    <div style={{ left: d.x + 10, top: d.y + 8 }} className="pointer-events-none fixed z-[70] flex items-center gap-1.5 rounded-control border bg-popover px-2 py-1 text-sm shadow-lg">
      <Icon size={14} strokeWidth={1.75} style={{ color: m.color }} />
      {m.label}
    </div>,
    document.body,
  );
}
