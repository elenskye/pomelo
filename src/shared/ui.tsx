// Shared UI primitives in the Design System「Pomelo 柚儿园」 look (ds.css, synced from visual-atelier; docs/design.md):
// hand-drawn edges painted by shared/hand.ts (data-hand), the controls' small motions (按下扁一下, 荧光笔拖过去, 圆贴纸滑过去,
// 小签晃两下, 空状态画完再弹…) from the Design System's motion parts. Sizes stay Pomelo's (compact) where a page is dense.
// Dropdowns: ./Dropdown.tsx.
import type { IconType } from "@/shared/icons";
import { RefreshCw } from "@/shared/icons";
import { useId, useLayoutEffect, useMemo, useRef, useState, type ButtonHTMLAttributes, type ComponentProps, type ReactNode } from "react";
import { flash } from "./hand";
import { HandIcon, Tip } from "./handParts";
import { markSvg, sizedMark } from "./marks";

/** A page's title, as the typical pages (visual-atelier's canvases/pomelo-pages): big, no icon — the module's sticker
 *  is slapped on the paper's corner — clear of that sticker; what the page holds in soft small print on the right.
 *  `icon` / `color` stay for pages shown outside the paper. */
export function PageHeader(props: { icon?: IconType; color?: string; title: string; badge?: ReactNode; right?: ReactNode }) {
  return (
    <header data-tauri-drag-region className="pm-page-head flex h-[68px] shrink-0 items-end justify-between gap-4 pr-8 pb-3 pl-14">
      <h1 className="pm-sheet-title pointer-events-none flex shrink-0 items-center gap-2 whitespace-nowrap">
        {props.title}
        {props.badge}
      </h1>
      <div className="min-w-0 text-right text-sm text-[var(--color-ink-soft)]">{props.right}</div>
    </header>
  );
}

/** The empty pages' drawings (visual-atelier marks/empty/): a page that has nothing yet draws its picture, then
 *  each thing in it pops, then the line (motion part empty-state). */
export type EmptyArt = "todo" | "notes" | "days" | "packing" | "books" | "films" | "jobs" | "vault";

function EmptyPicture(props: { art: EmptyArt }) {
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const svg = markSvg(`empty/${props.art}`);
  const html = useMemo(() => (svg ? sizedMark(svg, 256, uid, 150) : ""), [svg, uid]);
  return svg ? <span aria-hidden className="block" dangerouslySetInnerHTML={{ __html: html }} /> : null;
}

export function EmptyState(props: { title: string; hint?: ReactNode; action?: ReactNode; art?: EmptyArt }) {
  if (props.art && markSvg(`empty/${props.art}`))
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-1 pb-16 text-center">
        <div className="pm-empty is-in">
          <EmptyPicture art={props.art} />
          <p className="pm-empty-line">{props.title}</p>
        </div>
        {props.hint && <p className="text-xs text-muted-foreground">{props.hint}</p>}
        {props.action && <div className="mt-3">{props.action}</div>}
      </div>
    );
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-1 pb-16 text-center">
      <p className="text-lg">{props.title}</p>
      {props.hint && <p className="text-xs text-muted-foreground">{props.hint}</p>}
      {props.action && <div className="mt-3">{props.action}</div>}
    </div>
  );
}

type Variant = "primary" | "secondary" | "ghost" | "danger" | "ghost-danger";
/** The hand-drawn button frame (data-hand="button") and its fill; ghost ones have no frame. */
const FRAMES: Record<Variant, { cls: string; fill?: string } | null> = {
  primary: { cls: "text-[var(--color-on-primary)]" },
  secondary: { cls: "pm-btn--plain", fill: "card" },
  danger: { cls: "text-[var(--color-on-danger)]", fill: "danger" },
  ghost: null,
  "ghost-danger": null,
};
const GHOST: Partial<Record<Variant, string>> = { ghost: "hover:bg-muted", "ghost-danger": "text-destructive hover:bg-muted" };

export function Button({ variant = "secondary", className, type, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  const frame = FRAMES[variant];
  // a seed from the label keeps a button's wobble the same each time it's drawn
  const seed = typeof rest.children === "string" ? 1430 + ([...rest.children].reduce((a, c) => a + c.charCodeAt(0), 0) % 50) : 1430;
  if (!frame)
    return (
      <button
        type={type ?? "button"}
        className={`pm-press inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-control px-3 py-1.5 text-sm transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 ${GHOST[variant]} ${className ?? ""}`}
        {...rest}
      />
    );
  return (
    <button
      type={type ?? "button"}
      data-hand="button"
      data-seed={seed}
      data-fill={frame.fill}
      className={`pm-btn h-8 gap-1.5 px-3 text-sm ${frame.cls} ${className ?? ""}`}
      {...rest}
    />
  );
}

/** A field without a box of its own: the hand-drawn edge around it thickens when it has focus (Design System Input). */
const FIELD = "pm-field pm-compact";

/** Input look without a width — add one (w-24, w-auto…); inputClass is the full-width version. Plain inputs
 *  (selects, number boxes) keep this; TextInput / TextArea / SearchInput get the hand-drawn field. */
export const fieldClass =
  "rounded-control border border-[var(--color-pen)]/40 bg-card px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring";
export const inputClass = `w-full ${fieldClass}`;

export function TextInput({ className, ...rest }: ComponentProps<"input">) {
  return (
    <span className={`${FIELD} w-full ${className ?? ""}`} data-hand="field" data-seed="1410">
      <input type="text" autoComplete="off" spellCheck={false} {...rest} />
    </span>
  );
}

export function TextArea({ className, ...rest }: ComponentProps<"textarea">) {
  return (
    <span className={`${FIELD} pm-area w-full ${className ?? ""}`} data-hand="field" data-seed="1411">
      <textarea spellCheck={false} className="resize-none leading-relaxed" {...rest} />
    </span>
  );
}

export function SearchInput(props: ComponentProps<"input">) {
  const { className, ...rest } = props;
  return (
    <span className={`${FIELD} w-full ${className ?? ""}`} data-hand="field" data-seed="1412">
      <span className="pm-field-icon text-muted-foreground" style={{ order: -1 }}>
        <HandIcon name="search" size={16} seed={14} />
      </span>
      <input type="search" autoComplete="off" spellCheck={false} {...rest} />
    </span>
  );
}

/** One of a few: a marker stroke under the chosen one, dragged across to a new one (Design System Segmented). */
export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  ariaLabel?: string;
}) {
  const box = useRef<HTMLDivElement>(null);
  const hl = useRef<HTMLSpanElement>(null);
  const was = useRef(props.value);
  useLayoutEffect(() => {
    const on = box.current?.querySelector<HTMLElement>('[aria-checked="true"]');
    const h = hl.current;
    if (!on || !h) return;
    h.style.width = `${on.offsetWidth}px`;
    h.style.setProperty("--pm-seg-x", `${on.offsetLeft - 3}px`);
    if (was.current !== props.value) flash(h, "is-pop", 440);
    was.current = props.value;
  });
  return (
    <div ref={box} role="radiogroup" aria-label={props.ariaLabel} className="pm-seg pm-compact shrink-0" data-hand="field" data-seed="1390" data-sw="1.5" data-amp="0.55">
      <span ref={hl} className="pm-seg-hl" data-hand="pick" data-seed="1391" />
      {props.options.map((o) => {
        const on = o.value === props.value;
        return (
          <button key={o.value} type="button" role="radio" aria-checked={on} onClick={() => props.onChange(o.value)} className="pm-seg-item outline-none focus-visible:ring-2 focus-visible:ring-ring">
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** Label + control. A div (not <label>) so clicks on the text don't trigger button-based controls. */
/** A labelled field; `action` sits at the right end of the label row (柚子's 贴 JD, 拆成子任务). */
export function Field(props: { label: string; action?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      {props.action ? (
        <div className="flex items-center justify-between gap-3">
          <span className="text-xs text-muted-foreground">{props.label}</span>
          {props.action}
        </div>
      ) : (
        <span className="text-xs text-muted-foreground">{props.label}</span>
      )}
      {props.children}
    </div>
  );
}

/** Small icon-only button (ghost): its name hangs under it on a string when pointed at (Design System Tooltip). */
export function IconButton(props: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      className={`pm-press pm-tip-host relative inline-flex shrink-0 items-center justify-center rounded-control p-1.5 transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30 ${
        props.danger ? "text-destructive" : "text-muted-foreground hover:text-foreground"
      }`}
    >
      {props.children}
      <Tip>{props.label}</Tip>
    </button>
  );
}

/** 重新读文件夹 (a page showing what's in a folder: 呦呦漫游, Seeker, 知识库): spins until `onRefresh` is done. */
export function RefreshButton(props: { label: string; onRefresh: () => Promise<unknown>; disabled?: boolean }) {
  const [busy, setBusy] = useState(false);
  return (
    <IconButton label={props.label} disabled={props.disabled || busy} onClick={() => { setBusy(true); void props.onRefresh().finally(() => setBusy(false)); }}>
      <RefreshCw size={14} strokeWidth={1.75} className={busy ? "animate-spin" : ""} />
    </IconButton>
  );
}

/** The hand-cut backing under the chosen row of a side list (index.css .side-row): first inside a row that is `on`. */
export function Picked(props: { seed?: number }) {
  return <span aria-hidden className="side-picked" data-hand="pick" data-amp="1" data-pad="2" data-seed={props.seed ?? 1420} />;
}

/** Thin progress bar: done (green) + doing (amber) segments. */
export function ProgressBar(props: { done: number; doing?: number; total: number }) {
  const pct = (n: number) => (props.total > 0 ? `${(n / props.total) * 100}%` : "0%");
  return (
    <div className="flex h-1.5 w-full overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuenow={props.done} aria-valuemax={props.total}>
      <div style={{ width: pct(props.done), background: "var(--status-done-fg)" }} />
      <div style={{ width: pct(props.doing ?? 0), background: "var(--status-doing-fg)", opacity: 0.55 }} />
    </div>
  );
}

/** On / off (settings rows): a round sticker slides across, a marker fills the track (Design System Switch). */
export function Switch(props: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  const ref = useRef<HTMLButtonElement>(null);
  return (
    <button
      ref={ref}
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => {
        props.onChange(!props.checked);
        flash(ref.current, "is-pop", 460);
      }}
      className="pm-switch pm-switch-only shrink-0 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
    >
      <span data-hand="switch" data-seed="1371" />
    </button>
  );
}
