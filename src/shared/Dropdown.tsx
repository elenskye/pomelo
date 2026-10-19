// Custom dropdown (replaces native <select>): popover rendered in a portal and shown in the top layer
// (never clipped by scroll containers, tables or a modal dialog it sits in), flips upward when needed, keyboard: ↑↓ Home End Enter Space Esc Tab.
// Clicks inside never bubble to parents (safe inside clickable table rows).
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

/** `group`: options sharing one get a small heading above them in the menu (consecutive options only). */
export type DropdownOption<T> = { value: T; label: ReactNode; group?: string };
export type DropdownAction = { label: ReactNode; onSelect: () => void };

export function Dropdown<T>(props: {
  value: T;
  options: DropdownOption<T>[];
  onChange: (v: T) => void;
  ariaLabel: string;
  /** Extra commands below a divider (e.g. 管理类型…). */
  actions?: DropdownAction[];
  align?: "start" | "end";
  /** Custom trigger content (default: selected label + chevron in a bordered button). */
  renderTrigger?: (selected: DropdownOption<T> | undefined, open: boolean) => ReactNode;
  triggerClassName?: string;
  placeholder?: string;
  menuMaxHeight?: number;
  /** Action menu: no check column. */
  plain?: boolean;
  /** Several at once (多选): every option shown ticked by `isPicked`, choosing one toggles it and the menu stays open. */
  isPicked?: (v: T) => boolean;
}) {
  const { options, actions = [] } = props;
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ top: number; left: number; minWidth: number } | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const selectedIndex = options.findIndex((o) => Object.is(o.value, props.value));
  const selected = options[selectedIndex];
  const total = options.length + actions.length;

  const show = () => {
    setActive(Math.max(0, selectedIndex));
    setPos(null);
    setOpen(true);
  };
  const hide = (refocus: boolean) => {
    setOpen(false);
    if (refocus) btn.current?.focus();
  };
  const choose = (i: number) => {
    if (props.isPicked && i < options.length) return props.onChange(options[i].value);
    hide(true);
    if (i < options.length) {
      if (!Object.is(options[i].value, props.value)) props.onChange(options[i].value);
    } else actions[i - options.length]?.onSelect();
  };
  const picked = (i: number) => (props.isPicked ? props.isPicked(options[i].value) : i === selectedIndex);

  // Position after the menu renders (measure its size), before paint. In the top layer first: a
  // dialog (WebKit) would otherwise clip it, and a hidden popover has no size.
  useLayoutEffect(() => {
    if (!open || !btn.current || !menu.current) return;
    const r = btn.current.getBoundingClientRect();
    const m = menu.current;
    try {
      if (typeof m.showPopover === "function" && !m.matches(":popover-open")) m.showPopover();
    } catch {
      // no Popover API: stays a fixed element (fine outside dialogs)
    }
    const w = Math.max(m.offsetWidth, r.width);
    const h = m.offsetHeight;
    const fitsBelow = window.innerHeight - r.bottom - 8 >= h || r.top < h + 8;
    const top = fitsBelow ? r.bottom + 4 : r.top - 4 - h;
    const rawLeft = props.align === "end" ? r.right - w : r.left;
    const left = Math.min(Math.max(8, rawLeft), window.innerWidth - w - 8);
    setPos({ top, left, minWidth: r.width });
  }, [open, props.align]);

  useEffect(() => {
    if (open && pos) menu.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [open, pos, active]);

  useEffect(() => {
    if (!open) return;
    const outside = (e: Event) => {
      const t = e.target as Node;
      if (!btn.current?.contains(t) && !menu.current?.contains(t)) hide(false);
    };
    const onScroll = (e: Event) => {
      if (!menu.current?.contains(e.target as Node)) hide(false);
    };
    const onResize = () => hide(false);
    document.addEventListener("mousedown", outside);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }, [open]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
        e.preventDefault();
        show();
      }
      return;
    }
    const move = (i: number) => {
      e.preventDefault();
      setActive((i + total) % total);
    };
    if (e.key === "ArrowDown") move(active + 1);
    else if (e.key === "ArrowUp") move(active - 1);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(total - 1);
    else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      choose(active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation(); // don't close the surrounding panel / dialog
      hide(true);
    } else if (e.key === "Tab") hide(false);
  };

  const container = btn.current?.closest("dialog") ?? document.body; // modal dialogs are top-layer
  const item = (i: number, content: ReactNode, isSelected: boolean, muted = false) => (
    <div
      key={i}
      id={`${id}-${i}`}
      data-index={i}
      role="option"
      aria-selected={isSelected}
      onMouseEnter={() => setActive(i)}
      onClick={() => choose(i)}
      className={`flex cursor-default items-center gap-2 rounded-md py-1.5 pr-3 ${props.plain ? "pl-3" : "pl-2"} text-sm whitespace-nowrap ${
        i === active ? "bg-muted" : ""
      } ${muted ? "text-muted-foreground" : ""}`}
    >
      {!props.plain && <Check size={14} className={`shrink-0 text-primary ${isSelected ? "" : "invisible"}`} />}
      {content}
    </div>
  );

  return (
    <>
      <button
        ref={btn}
        type="button"
        aria-label={props.ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-activedescendant={open ? `${id}-${active}` : undefined}
        onClick={(e) => {
          e.stopPropagation();
          if (open) hide(true);
          else show();
        }}
        onKeyDown={onKeyDown}
        className={
          props.triggerClassName ??
          `inline-flex shrink-0 items-center gap-1.5 rounded-control border bg-card py-1.5 pr-2 pl-3 text-sm whitespace-nowrap transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${
            open ? "bg-muted" : ""
          }`
        }
      >
        {props.renderTrigger ? (
          props.renderTrigger(selected, open)
        ) : (
          <>
            <span className={selected ? "" : "text-muted-foreground"}>{selected?.label ?? props.placeholder ?? "请选择"}</span>
            <ChevronDown
              size={14}
              className={`text-muted-foreground transition-transform duration-150 ${open ? "rotate-180" : ""}`}
            />
          </>
        )}
      </button>
      {open &&
        createPortal(
          <div
            ref={menu}
            popover="manual"
            role="listbox"
            aria-multiselectable={props.isPicked ? true : undefined}
            aria-label={props.ariaLabel}
            onMouseDown={(e) => e.preventDefault()} // keep focus on the trigger
            onClick={(e) => e.stopPropagation()}
            style={{
              position: "fixed",
              top: pos?.top ?? 0,
              left: pos?.left ?? 0,
              right: "auto",
              bottom: "auto",
              margin: 0,
              minWidth: pos?.minWidth,
              maxHeight: props.menuMaxHeight ?? 320,
              visibility: pos ? "visible" : "hidden",
            }}
            className="z-50 animate-pop-in overflow-y-auto rounded-card border bg-popover p-1 text-foreground shadow-lg"
          >
            {options.map((o, i) =>
              o.group && o.group !== options[i - 1]?.group ? (
                <div key={i} role="presentation">
                  <div className={`px-2 pb-1 text-xs text-muted-foreground ${i > 0 ? "pt-2" : "pt-1"}`}>{o.group}</div>
                  {item(i, o.label, picked(i))}
                </div>
              ) : (
                item(i, o.label, picked(i))
              ),
            )}
            {actions.length > 0 && <div className="mx-1 my-1 border-t" />}
            {actions.map((a, j) => item(options.length + j, a.label, false, true))}
          </div>,
          container,
        )}
    </>
  );
}
