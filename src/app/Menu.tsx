// A small menu at a point (right-click, 「⋯」, right-click on ‹ ›): rows with an icon or a colored dot, a
// check on the current one, grey headings and lines between groups. Closes on choosing, Esc or a click
// outside; ↑↓ ↩ work. Stays inside the window.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check } from "lucide-react";

export type MenuRow = { label: string; icon?: ReactNode; dot?: string; checked?: boolean; onSelect: () => void } | { head: string } | "-";
export type MenuAt = { x: number; y: number; rows: MenuRow[] };

export function Menu(props: MenuAt & { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const items = props.rows.flatMap((r, i) => (typeof r === "object" && "onSelect" in r ? [i] : []));
  const [active, setActive] = useState(-1);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(props.x, window.innerWidth - width - 8)), top: props.y + height > window.innerHeight - 8 ? Math.max(8, props.y - height) : props.y });
  }, [props.x, props.y]);
  useEffect(() => {
    const away = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && props.onClose();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const k = items.indexOf(active);
        setActive(items[(k + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length] ?? -1);
      } else if (e.key === "Enter" && active >= 0) {
        e.preventDefault();
        const r = props.rows[active];
        if (typeof r === "object" && "onSelect" in r) {
          props.onClose();
          r.onSelect();
        }
      }
    };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("blur", props.onClose);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("blur", props.onClose);
    };
  });
  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={{ position: "fixed", left: pos?.left ?? props.x, top: pos?.top ?? props.y, visibility: pos ? "visible" : "hidden" }}
      className="z-[60] min-w-40 animate-pop-in rounded-card border bg-popover p-1 text-foreground shadow-lg"
      onContextMenu={(e) => e.preventDefault()}
    >
      {props.rows.map((r, i) => {
        if (r === "-") return <div key={i} className="mx-1 my-1 border-t" />;
        if ("head" in r) return <div key={i} className="px-2 pt-1 pb-0.5 text-xs text-muted-foreground">{r.head}</div>;
        return (
          <button
            key={i}
            type="button"
            role="menuitem"
            onMouseEnter={() => setActive(i)}
            onClick={() => {
              props.onClose();
              r.onSelect();
            }}
            className={`flex w-full cursor-default items-center gap-2 rounded-md py-1.5 pr-3 pl-2 text-left text-sm whitespace-nowrap outline-none ${i === active ? "bg-muted" : ""}`}
          >
            <span className="flex w-4 shrink-0 justify-center text-muted-foreground">{r.dot ? <span className="size-2 rounded-full" style={{ background: r.dot }} /> : r.icon}</span>
            <span className="min-w-0 flex-1 truncate">{r.label}</span>
            {r.checked && <Check size={14} className="shrink-0 text-primary" />}
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
