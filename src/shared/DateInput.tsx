// Date (or month) field with a Pomelo calendar popover instead of WebKit's native picker. Value is
// "YYYY-MM-DD" ("YYYY-MM" with `month`), "" = empty. Popover: portal like Dropdown (never clipped,
// flips up, stays in a modal dialog's top layer). Day view: ‹ 2026 年 10 月 › (click the title for
// months), Monday first, 6 rows; keys ←→↑↓ PageUp/PageDown Enter Esc; footer 今天 / 清除.
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";
import { addDays, localToday, mondayOf, toYmd } from "./dates";
import { fieldClass } from "./ui";

const WEEK = ["一", "二", "三", "四", "五", "六", "日"];
const pad = (n: number) => String(n).padStart(2, "0");
const monthOf = (d: string) => d.slice(0, 7);
const shiftMonth = (ym: string, k: number) => {
  const [y, m] = ym.split("-").map(Number);
  const d = new Date(y, m - 1 + k, 1);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
};
/** Same day in another month (clamped to its end). */
const sameDayIn = (day: string, ym: string) => {
  const [y, m] = ym.split("-").map(Number);
  return `${ym}-${pad(Math.min(Number(day.slice(8)), new Date(y, m, 0).getDate()))}`;
};
const slash = (v: string) => v.split("-").join(" / ");

export function DateInput(props: {
  value: string;
  onChange: (v: string) => void;
  ariaLabel: string;
  /** Pick a month ("YYYY-MM") instead of a day. */
  month?: boolean;
  /** Show × to empty it (and 清除 in the popover). */
  clearable?: boolean;
  min?: string;
  max?: string;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const pop = useRef<HTMLDivElement>(null);
  const id = useId();

  const hide = (refocus: boolean) => {
    setOpen(false);
    if (refocus) btn.current?.focus();
  };
  const pick = (v: string) => {
    hide(true);
    if (v !== props.value) props.onChange(v);
  };

  useLayoutEffect(() => {
    if (!open || !btn.current || !pop.current) return;
    const r = btn.current.getBoundingClientRect();
    const w = pop.current.offsetWidth;
    const h = pop.current.offsetHeight;
    const below = window.innerHeight - r.bottom - 8 >= h || r.top < h + 8;
    setPos({ top: below ? r.bottom + 4 : r.top - 4 - h, left: Math.min(Math.max(8, r.left), window.innerWidth - w - 8) });
  }, [open]);

  // focus the grid once the popover is placed (a hidden element can't take focus)
  useEffect(() => {
    if (open && pos) pop.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }, [open, pos]);

  useEffect(() => {
    if (!open) return;
    const outside = (e: Event) => {
      const t = e.target as Node;
      if (!btn.current?.contains(t) && !pop.current?.contains(t)) hide(false);
    };
    const onScroll = (e: Event) => {
      if (!pop.current?.contains(e.target as Node)) hide(false);
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

  const container = btn.current?.closest("dialog") ?? document.body;
  return (
    <div className="inline-flex shrink-0 items-center gap-0.5">
      <button
        ref={btn}
        type="button"
        aria-label={props.ariaLabel}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => (open ? hide(true) : (setPos(null), setOpen(true)))}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            setPos(null);
            setOpen(true);
          }
        }}
        className={`${fieldClass} inline-flex items-center gap-2 tabular-nums ${open ? "ring-2 ring-ring" : ""} ${props.className ?? ""}`}
      >
        <span className={props.value ? "" : "text-muted-foreground"}>{props.value ? slash(props.value) : props.month ? "年 / 月" : "年 / 月 / 日"}</span>
        <CalendarDays size={14} className="ml-auto shrink-0 text-muted-foreground" />
      </button>
      {props.clearable && props.value && (
        <button
          type="button"
          aria-label={`清除${props.ariaLabel}`}
          title={`清除${props.ariaLabel}`}
          onClick={() => props.onChange("")}
          className="rounded p-0.5 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X size={13} />
        </button>
      )}
      {open &&
        createPortal(
          <div
            ref={pop}
            id={id}
            role="dialog"
            aria-label={props.ariaLabel}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation(); // don't close the panel / dialog around it
                hide(true);
              }
            }}
            style={{ position: "fixed", top: pos?.top ?? 0, left: pos?.left ?? 0, visibility: pos ? "visible" : "hidden" }}
            className="z-50 w-64 animate-pop-in rounded-card border bg-popover p-3 text-foreground shadow-lg"
          >
            {props.month ? (
              <MonthGrid value={props.value} onPick={pick} onClear={props.clearable ? () => pick("") : undefined} min={props.min} max={props.max} />
            ) : (
              <DayGrid value={props.value} onPick={pick} onClear={props.clearable ? () => pick("") : undefined} min={props.min} max={props.max} />
            )}
          </div>,
          container,
        )}
    </div>
  );
}

const navBtn =
  "rounded-control p-1 text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30";
const footBtn = "rounded-control px-2 py-1 text-sm text-primary outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40";

type GridProps = { value: string; onPick: (v: string) => void; onClear?: () => void; min?: string; max?: string };

function DayGrid(props: GridProps) {
  const today = localToday();
  const [active, setActive] = useState(props.value || (props.max && today > props.max ? props.max : today));
  const [months, setMonths] = useState(false); // month chooser instead of days
  const shown = monthOf(active);
  const grid = useRef<HTMLDivElement>(null);
  const ok = (d: string) => (!props.min || d >= props.min) && (!props.max || d <= props.max);
  const [y, m] = shown.split("-").map(Number);

  useEffect(() => {
    if (!months) grid.current?.focus();
  }, [months]);

  if (months) {
    return (
      <MonthGrid
        value={shown}
        onPick={(ym) => {
          setActive(sameDayIn(active, ym));
          setMonths(false);
        }}
      />
    );
  }

  const start = mondayOf(`${shown}-01`);
  const cells = Array.from({ length: 42 }, (_, i) => addDays(start, i));
  const onKey = (e: KeyboardEvent) => {
    const move: Record<string, () => string> = {
      ArrowLeft: () => addDays(active, -1),
      ArrowRight: () => addDays(active, 1),
      ArrowUp: () => addDays(active, -7),
      ArrowDown: () => addDays(active, 7),
      PageUp: () => sameDayIn(active, shiftMonth(shown, -1)),
      PageDown: () => sameDayIn(active, shiftMonth(shown, 1)),
    };
    if (move[e.key]) {
      e.preventDefault();
      setActive(move[e.key]());
    } else if ((e.key === "Enter" || e.key === " ") && ok(active)) {
      e.preventDefault();
      props.onPick(active);
    }
  };
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <button type="button" aria-label="上个月" onClick={() => setActive(sameDayIn(active, shiftMonth(shown, -1)))} className={navBtn}>
          <ChevronLeft size={15} />
        </button>
        <button type="button" onClick={() => setMonths(true)} className="rounded-control px-2 py-0.5 text-sm font-bold outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
          {y} 年 {m} 月
        </button>
        <button type="button" aria-label="下个月" onClick={() => setActive(sameDayIn(active, shiftMonth(shown, 1)))} className={navBtn}>
          <ChevronRight size={15} />
        </button>
      </div>
      <div ref={grid} data-autofocus tabIndex={0} role="grid" aria-label={`${y} 年 ${m} 月`} onKeyDown={onKey} className="grid grid-cols-7 gap-0.5 text-center text-sm outline-none">
        {WEEK.map((w) => (
          <span key={w} className="pb-1 text-xs text-muted-foreground">
            {w}
          </span>
        ))}
        {cells.map((d) => {
          const inMonth = monthOf(d) === shown;
          const selected = d === props.value;
          const enabled = ok(d);
          return (
            <button
              key={d}
              type="button"
              tabIndex={-1}
              disabled={!enabled}
              aria-selected={selected}
              aria-label={`${Number(d.slice(5, 7))} 月 ${Number(d.slice(8))} 日`}
              onClick={() => props.onPick(d)}
              onMouseEnter={() => enabled && setActive(d)}
              className={`aspect-square rounded-full tabular-nums transition-colors duration-100 disabled:opacity-30 ${
                selected ? "bg-primary text-primary-foreground" : d === active ? "bg-muted" : "hover:bg-muted"
              } ${!inMonth && !selected ? "text-muted-foreground/60" : ""} ${d === today && !selected ? "ring-1 ring-ring" : ""}`}
            >
              {Number(d.slice(8))}
            </button>
          );
        })}
      </div>
      <div className="flex items-center justify-between border-t pt-2">
        <button type="button" disabled={!ok(today)} onClick={() => props.onPick(today)} className={footBtn}>
          今天
        </button>
        {props.onClear && props.value && (
          <button type="button" onClick={props.onClear} className={`${footBtn} text-muted-foreground`}>
            清除
          </button>
        )}
      </div>
    </div>
  );
}

function MonthGrid(props: GridProps) {
  const now = toYmd(new Date()).slice(0, 7);
  const [year, setYear] = useState(Number((props.value || now).slice(0, 4)));
  const ok = (ym: string) => (!props.min || ym >= props.min.slice(0, 7)) && (!props.max || ym <= props.max.slice(0, 7));
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <button type="button" aria-label="上一年" onClick={() => setYear(year - 1)} className={navBtn}>
          <ChevronLeft size={15} />
        </button>
        <span className="text-sm font-bold">{year} 年</span>
        <button type="button" aria-label="下一年" onClick={() => setYear(year + 1)} className={navBtn}>
          <ChevronRight size={15} />
        </button>
      </div>
      <div className="grid grid-cols-3 gap-1 text-sm">
        {Array.from({ length: 12 }, (_, i) => {
          const ym = `${year}-${pad(i + 1)}`;
          const selected = ym === props.value;
          return (
            <button
              key={ym}
              type="button"
              disabled={!ok(ym)}
              aria-selected={selected}
              data-autofocus={selected || (!props.value && ym === now) || undefined}
              onClick={() => props.onPick(ym)}
              className={`rounded-control py-1.5 outline-none transition-colors duration-100 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-30 ${
                selected ? "bg-primary text-primary-foreground" : "hover:bg-muted"
              } ${ym === now && !selected ? "ring-1 ring-ring" : ""}`}
            >
              {i + 1} 月
            </button>
          );
        })}
      </div>
      <div className="flex items-center justify-between border-t pt-2">
        <button type="button" disabled={!ok(now)} onClick={() => props.onPick(now)} className={footBtn}>
          本月
        </button>
        {props.onClear && props.value && (
          <button type="button" onClick={props.onClear} className={`${footBtn} text-muted-foreground`}>
            清除
          </button>
        )}
      </div>
    </div>
  );
}
