import { CheckCircle2, Circle, CircleDot } from "./icons";
import { Dropdown } from "./Dropdown";

// Unified status (matches Rust modules::common::Status). Labels differ per module.
export type StatusCode = "unmarked" | "todo" | "doing" | "done";
export type StatusLabels = Record<StatusCode, string>;

export const STATUS_ORDER: StatusCode[] = ["unmarked", "todo", "doing", "done"];

function StatusTag(props: { value: StatusCode; labels: StatusLabels }) {
  return (
    // the Design System's chip: the status's tint, a dot of its color
    <span style={{ "--pm-tag-bg": `var(--status-${props.value}-bg)`, "--pm-dot": `var(--status-${props.value}-fg)` } as React.CSSProperties} className="pm-chip">
      {props.labels[props.value]}
    </span>
  );
}

/** Colored pill opening a status menu. Safe inside clickable rows (Dropdown stops propagation). */
export function StatusSelect(props: { value: StatusCode; labels: StatusLabels; onChange: (s: StatusCode) => void; order?: StatusCode[] }) {
  return (
    <Dropdown
      ariaLabel="状态"
      value={props.value}
      onChange={props.onChange}
      options={(props.order ?? STATUS_ORDER).map((s) => ({ value: s, label: <StatusTag value={s} labels={props.labels} /> }))}
      triggerClassName="rounded-md outline-none transition-opacity duration-150 hover:opacity-80 focus-visible:ring-2 focus-visible:ring-ring"
      renderTrigger={() => <StatusTag value={props.value} labels={props.labels} />}
    />
  );
}

/** The round check before a lesson (随意画画, 循序渐进): done / doing / not started; a click toggles done. */
export function DoneToggle(props: { lesson: { title: string; status: StatusCode }; onToggle: () => void }) {
  const s = props.lesson.status;
  const Icon = s === "done" ? CheckCircle2 : s === "doing" ? CircleDot : Circle;
  const color = s === "done" ? "var(--status-done-fg)" : s === "doing" ? "var(--status-doing-fg)" : "var(--muted-foreground)";
  return (
    <button
      type="button"
      aria-label={s === "done" ? `把「${props.lesson.title}」标为未开始` : `把「${props.lesson.title}」标为已完成`}
      onClick={(e) => {
        e.stopPropagation();
        props.onToggle();
      }}
      className="rounded-full p-1 outline-none transition-transform duration-150 hover:scale-110 focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Icon size={18} style={{ color }} />
    </button>
  );
}
