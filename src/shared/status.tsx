import { Dropdown } from "./Dropdown";

// Unified status (matches Rust modules::common::Status). Labels differ per module.
export type StatusCode = "unmarked" | "todo" | "doing" | "done";
export type StatusLabels = Record<StatusCode, string>;

export const STATUS_ORDER: StatusCode[] = ["unmarked", "todo", "doing", "done"];

export const statusStyle = (s: StatusCode) => ({
  background: `var(--status-${s}-bg)`,
  color: `var(--status-${s}-fg)`,
});

export function StatusTag(props: { value: StatusCode; labels: StatusLabels }) {
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
