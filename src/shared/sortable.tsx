// Drag to reorder a vertical list with pointer events (HTML5 drag-and-drop is unreliable in
// Tauri's macOS webview). One hook per list of siblings; nested lists get their own hook.
//   const sort = useSortable((ids) => save(ids));
//   <div ref={sort.listRef}>{rows.map((r) => <div key={r.id} {...sort.item(r.id)}><Row {...sort.handle(r.id)} /></div>)}</div>
// `handle` can go on a whole row: a drag starts only after the pointer moves a few pixels, so plain
// clicks (open, buttons inside) still work, and the click that ends a drag is swallowed. Moves are
// followed on window, so a fast flick that leaves the row still drags.
// Only direct children carrying data-sort-id take part; the new order is read from the DOM.
import { useRef, useState, type CSSProperties, type DragEvent, type MouseEvent, type PointerEvent as ReactPointerEvent } from "react";
import { GripVertical } from "lucide-react";

type Rect = { id: number; mid: number; height: number };
type Drag = { id: number; dy: number; rects: Rect[] };

const THRESHOLD = 4;

function target(d: Drag) {
  const from = d.rects.findIndex((r) => r.id === d.id);
  const center = d.rects[from].mid + d.dy;
  let to = from;
  for (let i = from + 1; i < d.rects.length; i++) if (center > d.rects[i].mid) to = i;
  for (let i = from - 1; i >= 0; i--) if (center < d.rects[i].mid) to = i;
  return { from, to };
}

export function useSortable<E extends HTMLElement = HTMLDivElement>(onReorder: (ids: number[]) => void) {
  const listRef = useRef<E>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const dragged = useRef(false);
  const save = useRef(onReorder);
  save.current = onReorder;

  const handle = (id: number) => ({
    onPointerDown: (e: ReactPointerEvent<HTMLElement>) => {
      dragged.current = false;
      const list = listRef.current;
      if (e.button !== 0 || !list) return;
      e.stopPropagation(); // nested lists: only the innermost row
      const startY = e.clientY;
      let current: Drag | null = null;
      const move = (ev: PointerEvent) => {
        if (!current) {
          if (Math.abs(ev.clientY - startY) < THRESHOLD) return;
          const els = [...list.querySelectorAll<HTMLElement>(":scope > [data-sort-id]")];
          if (els.length < 2) return stop();
          const rects = els.map((el) => {
            const r = el.getBoundingClientRect();
            return { id: Number(el.dataset.sortId), mid: r.top + r.height / 2, height: r.height };
          });
          window.getSelection()?.removeAllRanges();
          current = { id, dy: 0, rects };
        }
        current = { ...current, dy: ev.clientY - startY };
        setDrag(current);
      };
      const up = () => {
        stop();
        if (!current) return;
        dragged.current = true;
        setDrag(null);
        const { from, to } = target(current);
        if (from === to) return;
        const ids = current.rects.map((r) => r.id);
        ids.splice(to, 0, ids.splice(from, 1)[0]);
        save.current(ids);
      };
      const cancel = () => {
        stop();
        setDrag(null);
      };
      const stop = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", cancel);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", cancel);
    },
    // links / images inside a row would start a native drag and cancel the pointer stream
    onDragStart: (e: DragEvent<HTMLElement>) => e.preventDefault(),
    onClickCapture: (e: MouseEvent<HTMLElement>) => {
      if (!dragged.current) return;
      dragged.current = false;
      e.stopPropagation();
      e.preventDefault();
    },
  });

  const item = (id: number) => {
    const style: CSSProperties = {};
    if (drag) {
      const { from, to } = target(drag);
      const idx = drag.rects.findIndex((r) => r.id === id);
      const h = drag.rects[from].height;
      if (id === drag.id) {
        Object.assign(style, { transform: `translateY(${drag.dy}px)`, position: "relative", zIndex: 20, boxShadow: "0 6px 18px rgb(0 0 0 / 0.12)", cursor: "grabbing" });
      } else if (from < to && idx > from && idx <= to) {
        style.transform = `translateY(${-h}px)`;
      } else if (to < from && idx >= to && idx < from) {
        style.transform = `translateY(${h}px)`;
      }
      if (id !== drag.id) style.transition = "transform 150ms ease-out";
    }
    return { "data-sort-id": id, style };
  };

  return { listRef, handle, item, dragging: drag?.id ?? null };
}

/** Grip shown on row hover (parent needs the `group` class), for rows whose click does something else. */
export function DragHandle(props: ReturnType<ReturnType<typeof useSortable>["handle"]> & { label?: string }) {
  const { label, ...rest } = props;
  return (
    <span
      {...rest}
      onClick={(e) => e.stopPropagation()}
      role="button"
      aria-label={label ?? "拖动排序"}
      title={label ?? "拖动排序"}
      className="flex shrink-0 cursor-grab touch-none items-center text-muted-foreground opacity-0 transition-opacity duration-150 group-hover:opacity-100 active:cursor-grabbing"
    >
      <GripVertical size={14} />
    </span>
  );
}
