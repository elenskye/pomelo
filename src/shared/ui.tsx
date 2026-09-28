// Small shared UI primitives. shadcn/ui components get added in Phase 2 when needed.
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

export function PageHeader(props: { icon?: LucideIcon; color?: string; title: string; right?: ReactNode }) {
  const Icon = props.icon;
  return (
    <header data-tauri-drag-region className="flex h-16 shrink-0 items-end justify-between px-8 pb-2">
      <h1 className="pointer-events-none flex items-center gap-2 text-[22px] font-bold">
        {Icon && <Icon size={22} strokeWidth={1.75} style={{ color: props.color }} />}
        {props.title}
      </h1>
      {props.right}
    </header>
  );
}

export function EmptyState(props: { title: string; hint?: string }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-1 pb-16 text-center">
      <p className="text-lg">{props.title}</p>
      {props.hint && <p className="text-xs text-muted-foreground">{props.hint}</p>}
    </div>
  );
}

export function Button(props: { onClick?: () => void; children: ReactNode; variant?: "primary" | "secondary" }) {
  const base =
    "inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-control px-3 py-1.5 text-sm transition-colors duration-150";
  const look =
    props.variant === "primary"
      ? "bg-primary text-primary-foreground hover:opacity-90"
      : "border bg-card hover:bg-muted";
  return (
    <button type="button" onClick={props.onClick} className={`${base} ${look}`}>
      {props.children}
    </button>
  );
}
