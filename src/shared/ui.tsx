// Shared UI primitives: native controls styled to docs/design.md (no component library).
import type { LucideIcon } from "lucide-react";
import { ChevronDown, Search } from "lucide-react";
import type { ButtonHTMLAttributes, ComponentProps, ReactNode, TextareaHTMLAttributes } from "react";

export function PageHeader(props: { icon?: LucideIcon; color?: string; title: string; right?: ReactNode }) {
  const Icon = props.icon;
  return (
    <header data-tauri-drag-region className="flex h-16 shrink-0 items-end justify-between gap-4 px-8 pb-3">
      <h1 className="pointer-events-none flex shrink-0 items-center gap-2 text-[22px] font-bold whitespace-nowrap">
        {Icon && <Icon size={22} strokeWidth={1.75} style={{ color: props.color }} />}
        {props.title}
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

export const inputClass =
  "w-full rounded-control border bg-card px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring";

export function TextInput({ className, ...rest }: ComponentProps<"input">) {
  return <input type="text" autoComplete="off" spellCheck={false} className={`${inputClass} ${className ?? ""}`} {...rest} />;
}

export function TextArea({ className, ...rest }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
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

export function Select(props: {
  value: string;
  onChange: (v: string) => void;
  children: ReactNode;
  ariaLabel?: string;
  className?: string;
}) {
  return (
    <div className="relative inline-flex">
      <select
        value={props.value}
        aria-label={props.ariaLabel}
        onChange={(e) => props.onChange(e.target.value)}
        className={`appearance-none rounded-control border bg-card py-1.5 pr-8 pl-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${props.className ?? ""}`}
      >
        {props.children}
      </select>
      <ChevronDown size={14} className="pointer-events-none absolute top-1/2 right-2.5 -translate-y-1/2 text-muted-foreground" />
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
export function Field(props: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-xs text-muted-foreground">{props.label}</span>
      {props.children}
    </div>
  );
}
