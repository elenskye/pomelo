import { useState } from "react";
import { Star } from "lucide-react";

/** 1–5 stars; clicking the current value clears it. Read-only when no onChange. Filled vs outline (not color alone). */
export function StarRating(props: { value: number | null; onChange?: (v: number | null) => void; size?: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const size = props.size ?? 14;
  const shown = hover ?? props.value ?? 0;
  const editable = !!props.onChange;
  return (
    <div role="group" aria-label={`评分：${props.value ?? 0} 星`} className="inline-flex" onMouseLeave={() => setHover(null)}>
      {[1, 2, 3, 4, 5].map((n) => {
        const on = n <= shown;
        const star = (
          <Star
            size={size}
            strokeWidth={1.5}
            fill={on ? "var(--star)" : "none"}
            color={on ? "var(--star)" : "var(--muted-foreground)"}
            className={on ? "" : "opacity-60"}
          />
        );
        if (!editable) return <span key={n} className="p-px">{star}</span>;
        return (
          <button
            key={n}
            type="button"
            aria-label={`${n} 星`}
            className="rounded-sm p-px outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onMouseEnter={() => setHover(n)}
            onClick={(e) => {
              e.stopPropagation();
              props.onChange!(props.value === n ? null : n);
            }}
          >
            {star}
          </button>
        );
      })}
    </div>
  );
}
