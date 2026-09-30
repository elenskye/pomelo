// Settings page building blocks: titled card sections with label/value rows.
import type { ReactNode } from "react";

export function Row(props: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-center gap-6 border-b px-4 py-3 last:border-b-0">
      <div className="w-24 shrink-0 text-muted-foreground">{props.label}</div>
      <div className="min-w-0 flex-1">{props.children}</div>
    </div>
  );
}

/** Row body: description on the left, actions on the right. */
export function RowAction(props: { hint: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-sm text-muted-foreground">{props.hint}</span>
      <div className="flex shrink-0 gap-2">{props.children}</div>
    </div>
  );
}

export function Section(props: { title: string; children: ReactNode }) {
  return (
    <section className="mb-6">
      <h2 className="mb-2 px-1 text-sm text-muted-foreground">{props.title}</h2>
      <div className="rounded-card border bg-card">{props.children}</div>
    </section>
  );
}

export const Path = ({ value }: { value: string }) => <span className="selectable break-all font-mono text-xs">{value}</span>;
