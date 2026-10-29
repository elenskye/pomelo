// Custom dropdown (replaces native <select>): popover rendered in a portal and shown in the top layer
// (never clipped by scroll containers, tables or a modal dialog it sits in), flips upward when needed, keyboard: ↑↓ Home End Enter Space Esc Tab.
// Clicks inside never bubble to parents (safe inside clickable table rows).
// A long grouped list (`foldKey`) folds: a group's heading opens and closes it; the chosen option's group
// is open, and the groups opened by hand stay open for the session.
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown, ChevronRight } from "lucide-react";

/** `group`: options sharing one get a small heading above them in the menu (consecutive options only). */
export type DropdownOption<T> = { value: T; label: ReactNode; group?: string };
export type DropdownAction = { label: ReactNode; onSelect: () => void };

/** foldKey → the groups opened by hand (this session). */
const opened = new Map<string, Set<string>>();
type Row = { kind: "group"; group: string; count: number } | { kind: "option"; i: number } | { kind: "action"; j: number };

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
  /** As wide as its container (forms), the label cut with … when long. */
  block?: boolean;
  /** Several at once (多选): every option shown ticked by `isPicked`, choosing one toggles it and the menu stays open. */
  isPicked?: (v: T) => boolean;
  /** Fold the groups when the list is long (over 8 options, 2+ groups); the opened ones are remembered under this key. */
  foldKey?: string;
}) {
  const { options, actions = [] } = props;
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [openGroups, setOpenGroups] = useState<Set<string>>(new Set());
  const [pos, setPos] = useState<{ top: number; left: number; minWidth: number } | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const id = useId();
  const selectedIndex = options.findIndex((o) => Object.is(o.value, props.value));
  const selected = options[selectedIndex];
  const groupCount = new Set(options.map((o) => o.group).filter(Boolean)).size;
  const folding = !!props.foldKey && groupCount > 1 && options.length > 8;
  const rowsFor = (shown: Set<string>) => {
    const rows: Row[] = [];
    options.forEach((o, i) => {
      if (folding && o.group && o.group !== options[i - 1]?.group) rows.push({ kind: "group", group: o.group, count: options.filter((x) => x.group === o.group).length });
      if (!folding || !o.group || shown.has(o.group)) rows.push({ kind: "option", i });
    });
    actions.forEach((_, j) => rows.push({ kind: "action", j }));
    return rows;
  };
  const rows = rowsFor(openGroups);
  const total = rows.length;

  const show = () => {
    const shown = new Set(props.foldKey ? opened.get(props.foldKey) : []);
    if (selected?.group) shown.add(selected.group);
    setOpenGroups(shown);
    setActive(Math.max(0, rowsFor(shown).findIndex((r) => r.kind === "option" && r.i === selectedIndex)));
    setPos(null);
    setOpen(true);
  };
  const toggle = (group: string) => {
    const shown = new Set(openGroups);
    if (shown.has(group)) shown.delete(group);
    else shown.add(group);
    const mine = new Set(props.foldKey ? opened.get(props.foldKey) : []);
    if (shown.has(group)) mine.add(group);
    else mine.delete(group);
    if (props.foldKey) opened.set(props.foldKey, mine);
    setOpenGroups(shown);
  };
  const hide = (refocus: boolean) => {
    setOpen(false);
    if (refocus) btn.current?.focus();
  };
  const choose = (r: number) => {
    const row = rows[r];
    if (!row) return;
    if (row.kind === "group") return toggle(row.group);
    if (props.isPicked && row.kind === "option") return props.onChange(options[row.i].value);
    hide(true);
    if (row.kind === "option") {
      if (!Object.is(options[row.i].value, props.value)) props.onChange(options[row.i].value);
    } else actions[row.j]?.onSelect();
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
  }, [open, props.align, total]);

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
      <span className="min-w-0 truncate" title={typeof content === "string" ? content : undefined}>{content}</span>
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
          `${props.block ? "flex w-full min-w-0 justify-between" : "inline-flex shrink-0"} items-center gap-1.5 rounded-control border bg-card py-1.5 pr-2 pl-3 text-sm whitespace-nowrap transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring ${
            open ? "bg-muted" : ""
          }`
        }
      >
        {props.renderTrigger ? (
          props.renderTrigger(selected, open)
        ) : (
          <>
            <span className={`${props.block ? "min-w-0 truncate text-left" : ""} ${selected ? "" : "text-muted-foreground"}`}>{selected?.label ?? props.placeholder ?? "请选择"}</span>
            <ChevronDown
              size={14}
              className={`shrink-0 text-muted-foreground transition-transform duration-150 ${open ? "rotate-180" : ""}`}
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
              // long labels end in … rather than running off the window
              maxWidth: "min(36rem, calc(100vw - 16px))",
              maxHeight: props.menuMaxHeight ?? 320,
              visibility: pos ? "visible" : "hidden",
            }}
            className="z-50 animate-pop-in overflow-y-auto rounded-card border bg-popover p-1 text-foreground shadow-lg"
          >
            {rows.map((row, r) => {
              if (row.kind === "group") {
                const shown = openGroups.has(row.group);
                return (
                  <div
                    key={`g-${row.group}`}
                    id={`${id}-${r}`}
                    data-index={r}
                    role="option"
                    aria-selected={false}
                    aria-expanded={shown}
                    onMouseEnter={() => setActive(r)}
                    onClick={() => choose(r)}
                    className={`flex cursor-default items-center gap-1.5 rounded-md px-2 py-1 text-xs whitespace-nowrap text-muted-foreground ${r === active ? "bg-muted" : ""} ${r > 0 ? "mt-1" : ""}`}
                  >
                    {shown ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                    {row.group}
                    {!shown && <span className="text-muted-foreground/70">{row.count}</span>}
                  </div>
                );
              }
              if (row.kind === "action")
                return (
                  <div key={`a-${row.j}`} role="presentation">
                    {row.j === 0 && <div className="mx-1 my-1 border-t" />}
                    {item(r, actions[row.j].label, false, true)}
                  </div>
                );
              const o = options[row.i];
              return !folding && o.group && o.group !== options[row.i - 1]?.group ? (
                <div key={row.i} role="presentation">
                  <div className={`px-2 pb-1 text-xs text-muted-foreground ${row.i > 0 ? "pt-2" : "pt-1"}`}>{o.group}</div>
                  {item(r, o.label, picked(row.i))}
                </div>
              ) : (
                item(r, o.label, picked(row.i))
              );
            })}
          </div>,
          container,
        )}
    </>
  );
}
