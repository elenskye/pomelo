// 1–5 stars as the Design System's star stickers (data-hand="star"): rated ones are filled yuzu, the rest outlines
// (filled vs outline, not color alone). Clicking pops them in one after another up to the chosen one (motion part
// rating); clicking the current value clears it. Read-only when no onChange; small tables keep the small size.
import { useRef, useState } from "react";
import { flash } from "./hand";

export function StarRating(props: { value: number | null; onChange?: (v: number | null) => void; size?: number }) {
  const [hover, setHover] = useState<number | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const size = props.size ?? 14;
  const shown = hover ?? props.value ?? 0;
  const editable = !!props.onChange;
  return (
    <div ref={box} role="group" aria-label={`评分：${props.value ?? 0} 星`} className="pm-rating inline-flex" style={{ gap: 0 }} onMouseLeave={() => setHover(null)}>
      {[1, 2, 3, 4, 5].map((n) => {
        const on = n <= shown;
        const star = <span data-hand="star" data-size={size + 4} data-seed={1350 + n - 1} />;
        if (!editable)
          return (
            <span key={n} className={`pm-rating-star ${on ? "is-on" : ""}`} style={{ width: size + 4, height: size + 4 }}>
              {star}
            </span>
          );
        return (
          <button
            key={n}
            type="button"
            aria-label={`${n} 星`}
            className={`pm-rating-star rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "is-on" : ""}`}
            style={{ width: size + 6, height: size + 6 }}
            onMouseEnter={() => setHover(n)}
            onClick={(e) => {
              e.stopPropagation();
              const next = props.value === n ? null : n;
              props.onChange!(next);
              if (next) flash(box.current, "is-pop", 620);
            }}
          >
            {star}
          </button>
        );
      })}
    </div>
  );
}
