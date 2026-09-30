// Settings page building blocks: titled card sections with label/value rows. Explanations hide
// behind a small (?) that shows them on hover / focus, so rows stay one line.
import type { ReactNode } from "react";
import { CircleQuestionMark } from "lucide-react";

export function HelpTip(props: { text: ReactNode }) {
  return (
    <span className="group/help relative inline-flex align-middle">
      <button
        type="button"
        aria-label="说明"
        className="rounded-full text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <CircleQuestionMark size={14} strokeWidth={1.75} />
      </button>
      <span
        role="tooltip"
        className="pointer-events-none invisible absolute top-full left-0 z-30 mt-1.5 w-72 rounded-control border bg-popover px-3 py-2 text-xs leading-relaxed font-normal text-foreground opacity-0 shadow-lg transition-opacity duration-150 group-focus-within/help:visible group-focus-within/help:opacity-100 group-hover/help:visible group-hover/help:opacity-100"
      >
        {props.text}
      </span>
    </span>
  );
}

/** Label (with optional help) on the left, content on the right; `actions` align to the far right. */
export function Row(props: { label: string; help?: ReactNode; children?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex min-h-12 items-center gap-6 border-b px-4 py-2.5 last:border-b-0">
      <div className="flex w-28 shrink-0 items-center gap-1.5 text-muted-foreground">
        {props.label}
        {props.help && <HelpTip text={props.help} />}
      </div>
      <div className="min-w-0 flex-1">{props.children}</div>
      {props.actions && <div className="flex shrink-0 gap-2">{props.actions}</div>}
    </div>
  );
}

export function Section(props: { title: string; help?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="mb-6">
      <div className="mb-2 flex items-end justify-between gap-4 px-1">
        <h2 className="flex items-center gap-1.5 text-sm text-muted-foreground">
          {props.title}
          {props.help && <HelpTip text={props.help} />}
        </h2>
        {props.actions && <div className="flex shrink-0 gap-2">{props.actions}</div>}
      </div>
      <div className="rounded-card border bg-card">{props.children}</div>
    </section>
  );
}

export const Path = ({ value }: { value: string }) => <span className="selectable break-all font-mono text-xs">{value}</span>;
