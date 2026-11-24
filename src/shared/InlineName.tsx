// Renaming in place: the name turns into a text box where it stands (no dialog, no explanation). ↩ or leaving keeps it,
// Esc gives up; an IME's ↩ (choosing a candidate) doesn't count. `onDone(null)`: nothing to change.
import { useEffect, useRef, useState } from "react";

export function InlineName(props: {
  initial: string;
  label: string;
  max?: number;
  /** An empty name is an answer too (back to an own name); otherwise it is the same as giving up. */
  clearable?: boolean;
  onDone: (name: string | null) => void;
}) {
  const [v, setV] = useState(props.initial);
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => ref.current?.select(), []);
  const finish = (keep: boolean) => {
    if (done.current) return;
    done.current = true;
    const t = v.trim();
    props.onDone(keep && t !== props.initial && (t || props.clearable) ? t : null);
  };
  return (
    <input
      ref={ref}
      autoFocus
      value={v}
      aria-label={props.label}
      maxLength={props.max ?? 80}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => finish(true)}
      onClick={(e) => e.stopPropagation()}
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter" && !e.nativeEvent.isComposing) finish(true);
        else if (e.key === "Escape") {
          e.preventDefault();
          finish(false);
        }
      }}
      className="w-full min-w-0 rounded-control border bg-card px-2 py-0.5 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
    />
  );
}
