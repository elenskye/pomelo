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
    <span style={statusStyle(props.value)} className="inline-block rounded-md px-2 py-0.5 text-xs whitespace-nowrap">
      {props.labels[props.value]}
    </span>
  );
}

/** Colored pill that opens the native macOS menu. Stops click propagation (safe inside clickable rows). */
export function StatusSelect(props: { value: StatusCode; labels: StatusLabels; onChange: (s: StatusCode) => void }) {
  return (
    <select
      value={props.value}
      aria-label="状态"
      style={statusStyle(props.value)}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => props.onChange(e.target.value as StatusCode)}
      className="cursor-pointer appearance-none rounded-md px-2 py-0.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      {STATUS_ORDER.map((s) => (
        <option key={s} value={s}>
          {props.labels[s]}
        </option>
      ))}
    </select>
  );
}
