// Drag to reorder a vertical list with pointer events (HTML5 drag-and-drop is unreliable in
// Tauri's macOS webview). One hook per list of siblings; nested lists get their own hook.
//   const sort = useSortable((ids) => save(ids));
//   <div ref={sort.listRef}>{rows.map((r) => <div key={r.id} {...sort.item(r.id)}><DragHandle {...sort.handle(r.id)} />…</div>)}</div>
// Only direct children carrying data-sort-id take part; the new order is read from the DOM.
import { useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { GripVertical } from "lucide-react";

type Rect = { id: number; mid: number; height: number };
type Drag = { id: number; startY: number; dy: number; rects: Rect[] };

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

  const handle = (id: number) => ({
    onPointerDown: (e: PointerEvent<HTMLElement>) => {
      if (e.button !== 0 || !listRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      const els = [...listRef.current.querySelectorAll<HTMLElement>(":scope > [data-sort-id]")];
      if (els.length < 2) return;
      const rects = els.map((el) => {
        const r = el.getBoundingClientRect();
        return { id: Number(el.dataset.sortId), mid: r.top + r.height / 2, height: r.height };
      });
      try {
        e.currentTarget.setPointerCapture(e.pointerId); // keep receiving moves outside the grip
      } catch {
        /* synthetic / already released pointer */
      }
      setDrag({ id, startY: e.clientY, dy: 0, rects });
    },
    onPointerMove: (e: PointerEvent<HTMLElement>) => {
      if (drag?.id === id) setDrag({ ...drag, dy: e.clientY - drag.startY });
    },
    onPointerUp: () => {
      if (!drag) return;
      const { from, to } = target(drag);
      setDrag(null);
      if (from === to) return;
      const ids = drag.rects.map((r) => r.id);
      ids.splice(to, 0, ids.splice(from, 1)[0]);
      onReorder(ids);
    },
    onPointerCancel: () => setDrag(null),
    onClick: (e: { stopPropagation: () => void }) => e.stopPropagation(),
  });

  const item = (id: number) => {
    const style: CSSProperties = {};
    if (drag) {
      const { from, to } = target(drag);
      const idx = drag.rects.findIndex((r) => r.id === id);
      const h = drag.rects[from].height;
      if (id === drag.id) {
        Object.assign(style, { transform: `translateY(${drag.dy}px)`, position: "relative", zIndex: 20, boxShadow: "0 6px 18px rgb(0 0 0 / 0.12)" });
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

/** Grip shown on row hover (parent needs the `group` class). */
export function DragHandle(props: ReturnType<ReturnType<typeof useSortable>["handle"]> & { label?: string }) {
  const { label, ...rest } = props;
  return (
    <span
      {...rest}
      role="button"
      aria-label={label ?? "拖动排序"}
      title={label ?? "拖动排序"}
      className="flex shrink-0 cursor-grab touch-none items-center text-muted-foreground opacity-0 transition-opacity duration-150 group-hover:opacity-100 active:cursor-grabbing"
    >
      <GripVertical size={14} />
    </span>
  );
}
