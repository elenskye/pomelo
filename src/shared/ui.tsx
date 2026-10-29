// Shared UI primitives styled to docs/design.md (no component library). Dropdowns: ./Dropdown.tsx.
import type { LucideIcon } from "lucide-react";
import { Search } from "lucide-react";
import type { ButtonHTMLAttributes, ComponentProps, ReactNode } from "react";

export function PageHeader(props: { icon?: LucideIcon; color?: string; title: string; badge?: ReactNode; right?: ReactNode }) {
  const Icon = props.icon;
  return (
    <header data-tauri-drag-region className="flex h-16 shrink-0 items-end justify-between gap-4 px-8 pb-3">
      <h1 className="pointer-events-none flex shrink-0 items-center gap-2 text-[22px] font-bold whitespace-nowrap">
        {Icon && <Icon size={22} strokeWidth={1.75} style={{ color: props.color }} />}
        {props.title}
        {props.badge}
      </h1>
      <div className="min-w-0 text-right">{props.right}</div>
    </header>
  );
}

export function EmptyState(props: { title: string; hint?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-1 pb-16 text-center">
      <p className="text-lg">{props.title}</p>
      {props.hint && <p className="text-xs text-muted-foreground">{props.hint}</p>}
      {props.action && <div className="mt-3">{props.action}</div>}
    </div>
  );
}

type Variant = "primary" | "secondary" | "ghost" | "danger" | "ghost-danger";
const VARIANTS: Record<Variant, string> = {
  primary: "bg-primary text-primary-foreground hover:opacity-90",
  secondary: "border bg-card hover:bg-muted",
  ghost: "hover:bg-muted",
  danger: "bg-destructive text-white hover:opacity-90 dark:text-background",
  "ghost-danger": "text-destructive hover:bg-muted",
};

export function Button({ variant = "secondary", className, type, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      type={type ?? "button"}
      className={`inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-control px-3 py-1.5 text-sm transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 ${VARIANTS[variant]} ${className ?? ""}`}
      {...rest}
    />
  );
}

/** Input look without a width — add one (w-24, w-auto…); inputClass is the full-width version. */
export const fieldClass =
  "rounded-control border bg-card px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring";
export const inputClass = `w-full ${fieldClass}`;

export function TextInput({ className, ...rest }: ComponentProps<"input">) {
  return <input type="text" autoComplete="off" spellCheck={false} className={`${inputClass} ${className ?? ""}`} {...rest} />;
}

export function TextArea({ className, ...rest }: ComponentProps<"textarea">) {
  return <textarea spellCheck={false} className={`${inputClass} resize-none leading-relaxed ${className ?? ""}`} {...rest} />;
}

export function SearchInput(props: ComponentProps<"input">) {
  return (
    <div className="relative">
      <Search size={14} className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-muted-foreground" />
      <TextInput type="search" {...props} className={`pl-8 ${props.className ?? ""}`} />
    </div>
  );
}

export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (v: T) => void;
  ariaLabel?: string;
}) {
  return (
    <div role="radiogroup" aria-label={props.ariaLabel} className="inline-flex shrink-0 rounded-control bg-muted p-0.5">
      {props.options.map((o) => {
        const on = o.value === props.value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            onClick={() => props.onChange(o.value)}
            className={`rounded-md px-2.5 py-1 text-sm whitespace-nowrap transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring ${
              on ? "bg-card shadow-sm" : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

/** Label + control. A div (not <label>) so clicks on the text don't trigger button-based controls. */
/** A labelled field; `action` sits at the right end of the label row (小拾's 贴 JD, 拆成子任务). */
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

/** Small square icon-only button (ghost). */
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
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
      className={`inline-flex shrink-0 items-center justify-center rounded-control p-1.5 transition-colors duration-150 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-30 ${
        props.danger ? "text-destructive" : "text-muted-foreground hover:text-foreground"
      }`}
    >
      {props.children}
    </button>
  );
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

/** On / off switch (settings rows). */
export function Switch(props: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={() => props.onChange(!props.checked)}
      className={`relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full transition-colors duration-150 outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 ${
        props.checked ? "bg-primary" : "bg-muted-foreground/30"
      }`}
    >
      <span className={`size-3.5 rounded-full bg-white shadow-sm transition-transform duration-150 ${props.checked ? "translate-x-4" : "translate-x-0.5"}`} />
    </button>
  );
}
