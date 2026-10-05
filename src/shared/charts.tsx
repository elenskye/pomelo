// Small SVG charts on the theme tokens (--chart-*, docs/design.md). Method (dataviz skill): thin
// marks with 4px rounded data-ends on one baseline, hairline solid grid, one y-axis, hover tooltips
// (crosshair on lines, per-band on columns), text in text colors only — a colored key sits beside it.
// Every chart here is paired with a list or table on its page, so no value is hover-only.
import { useEffect, useRef, useState, type ReactNode } from "react";

export type Fmt = (n: number) => string;

/** Container width, kept current. */
function useWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setW(Math.floor(e.contentRect.width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, w] as const;
}

/** min … max in clean steps (1 / 2 / 2.5 / 5 × 10^k); min defaults to 0. */
export function niceTicks(max: number, count = 4, min = 0): number[] {
  if (!(max > min)) return max > 0 ? [0, max] : [0, 1];
  const raw = (max - min) / count;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw)!;
  const ticks: number[] = [];
  for (let v = Math.floor(min / step) * step; v < max + step * 0.001; v += step) ticks.push(v);
  if (ticks[ticks.length - 1] < max) ticks.push(ticks[ticks.length - 1] + step);
  return ticks;
}

function Tip(props: { x: number; width: number; children: ReactNode }) {
  const w = 168;
  const left = Math.max(0, Math.min(props.x - w / 2, props.width - w));
  return (
    <div
      role="status"
      className="pointer-events-none absolute top-0 z-20 rounded-control border bg-popover px-2.5 py-1.5 text-xs shadow-lg"
      style={{ left, width: w, transform: "translateY(-100%)" }}
    >
      {props.children}
    </div>
  );
}

/** Tooltip row: short line key, value first (strong), name after. */
export function TipRow(props: { color: string; value: string; name: string }) {
  return (
    <div className="flex items-center gap-1.5 tabular-nums">
      <span className="h-0.5 w-2.5 shrink-0 rounded-full" style={{ background: props.color }} />
      <span className="font-bold text-foreground">{props.value}</span>
      <span className="truncate text-muted-foreground">{props.name}</span>
    </div>
  );
}

/** Legend for ≥ 2 series (rect keys, like the marks). */
export function Legend(props: { items: { name: string; color: string }[] }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {props.items.map((s) => (
        <span key={s.name} className="flex items-center gap-1.5">
          <span className="size-2.5 rounded-sm" style={{ background: s.color }} />
          {s.name}
        </span>
      ))}
    </div>
  );
}

const PAD = { top: 10, right: 4, bottom: 22 };
/** Left margin that fits the widest y label (11px digits ≈ 6.6px, CJK ≈ 11px). */
const leftFor = (labels: string[]) => Math.max(28, 10 + Math.max(...labels.map((l) => [...l].reduce((w, ch) => w + (ch.charCodeAt(0) > 255 ? 11 : 6.6), 0))));

/** Rounded data-end (top), square at the baseline. */
function columnPath(x: number, y: number, w: number, h: number) {
  if (h <= 0) return "";
  const r = Math.min(4, w / 2, h);
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`;
}

function YGrid(props: { ticks: number[]; y: (v: number) => number; width: number; format: Fmt; left: number }) {
  return (
    <g>
      {props.ticks.map((t) => (
        <g key={t}>
          <line x1={props.left} x2={props.width - PAD.right} y1={props.y(t)} y2={props.y(t)} stroke={t === props.ticks[0] ? "var(--chart-axis)" : "var(--chart-grid)"} strokeWidth={1} />
          <text x={props.left - 6} y={props.y(t)} dy="0.32em" textAnchor="end" className="fill-muted-foreground text-[11px] tabular-nums">
            {props.format(t)}
          </text>
        </g>
      ))}
    </g>
  );
}

/** Grouped columns: `values[s][i]` for series s at x label i. One series → no legend. */
export function ColumnChart(props: {
  labels: string[];
  series: { name: string; color: string }[];
  values: number[][];
  format: Fmt;
  axisFormat?: Fmt;
  height?: number;
  title?: (i: number) => string;
  /** Show every nth x label (others stay in the tooltip). */
  labelEvery?: number;
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);
  const height = props.height ?? 180;
  const n = props.labels.length;
  const max = Math.max(0, ...props.values.flat());
  const ticks = niceTicks(max);
  const top = ticks[ticks.length - 1];
  const axisFmt = props.axisFormat ?? props.format;
  const left = leftFor(ticks.map(axisFmt));
  const plotH = height - PAD.top - PAD.bottom;
  const y = (v: number) => PAD.top + plotH - (v / top) * plotH;
  const band = n > 0 ? (width - left - PAD.right) / n : 0;
  const s = props.series.length;
  const barW = Math.max(2, Math.min(24, (band * 0.72 - (s - 1) * 2) / s));
  const groupW = s * barW + (s - 1) * 2;
  const every = props.labelEvery ?? 1;
  return (
    <div className="space-y-2">
      {s > 1 && <Legend items={props.series} />}
      <div ref={ref} className="relative" onPointerLeave={() => setHover(null)}>
        {width > 0 && (
          <svg width={width} height={height} role="img" aria-label={props.series.map((x) => x.name).join("、")}>
            <YGrid ticks={ticks} y={y} width={width} format={axisFmt} left={left} />
            {props.labels.map((label, i) => {
              const x0 = left + i * band;
              const gx = x0 + (band - groupW) / 2;
              return (
                <g key={i}>
                  {hover === i && <rect x={x0} y={PAD.top} width={band} height={plotH} className="fill-muted" />}
                  {props.series.map((ser, k) => {
                    const v = props.values[k][i] ?? 0;
                    return <path key={k} d={columnPath(gx + k * (barW + 2), y(v), barW, y(0) - y(v))} fill={ser.color} />;
                  })}
                  {i % every === 0 && (
                    <text x={x0 + band / 2} y={height - 6} textAnchor="middle" className="fill-muted-foreground text-[11px] tabular-nums">
                      {label}
                    </text>
                  )}
                  <rect x={x0} y={0} width={band} height={height} fill="transparent" onPointerEnter={() => setHover(i)} />
                </g>
              );
            })}
          </svg>
        )}
        {hover !== null && (
          <Tip x={left + (hover + 0.5) * band} width={width}>
            <div className="mb-1 text-muted-foreground">{props.title ? props.title(hover) : props.labels[hover]}</div>
            {props.series.map((ser, k) => (
              <TipRow key={k} color={ser.color} value={props.format(props.values[k][hover] ?? 0)} name={ser.name} />
            ))}
          </Tip>
        )}
      </div>
    </div>
  );
}

/** One series over time; gaps (null) break the line. Crosshair snaps to the nearest point.
 * `baseline="auto"` fits the y-range to the data (no area fill, which would need a zero baseline). */
export function LineChart(props: {
  labels: string[];
  values: (number | null)[];
  name: string;
  format: Fmt;
  axisFormat?: Fmt;
  height?: number;
  title?: (i: number) => string;
  labelEvery?: number;
  color?: string;
  baseline?: "zero" | "auto";
}) {
  const [ref, width] = useWidth();
  const [hover, setHover] = useState<number | null>(null);
  const height = props.height ?? 180;
  const color = props.color ?? "var(--chart-1)";
  const n = props.values.length;
  const present = props.values.filter((v): v is number => v !== null);
  const auto = props.baseline === "auto" && present.length > 0;
  const ticks = auto ? niceTicks(Math.max(...present), 4, Math.min(...present)) : niceTicks(Math.max(0, ...present));
  const [lo, top] = [ticks[0], ticks[ticks.length - 1]];
  const axisFmt = props.axisFormat ?? props.format;
  const left = leftFor(ticks.map(axisFmt));
  const plotH = height - PAD.top - PAD.bottom;
  const y = (v: number) => PAD.top + plotH - ((v - lo) / (top - lo || 1)) * plotH;
  const step = n > 1 ? (width - left - PAD.right - 16) / (n - 1) : 0;
  const x = (i: number) => left + 8 + (n > 1 ? i * step : (width - left - PAD.right) / 2);
  const segments: { i: number; v: number }[][] = [];
  props.values.forEach((v, i) => {
    if (v === null) return;
    const last = segments[segments.length - 1];
    if (last && last[last.length - 1].i === i - 1) last.push({ i, v });
    else segments.push([{ i, v }]);
  });
  const lastIdx = props.values.reduce<number>((acc, v, i) => (v !== null ? i : acc), -1);
  const every = props.labelEvery ?? 1;
  return (
    <div
      ref={ref}
      className="relative"
      onPointerLeave={() => setHover(null)}
      onPointerMove={(e) => {
        if (n === 0) return;
        const r = e.currentTarget.getBoundingClientRect();
        const i = n > 1 ? Math.round((e.clientX - r.left - left - 8) / step) : 0;
        setHover(Math.max(0, Math.min(n - 1, i)));
      }}
    >
      {width > 0 && (
        <svg width={width} height={height} role="img" aria-label={props.name}>
          <YGrid ticks={ticks} y={y} width={width} format={axisFmt} left={left} />
          {segments.map((seg, k) => (
            <g key={k}>
              {seg.length > 1 && !auto && (
                <path
                  d={`M${x(seg[0].i)},${y(0)}${seg.map((p) => `L${x(p.i)},${y(p.v)}`).join("")}L${x(seg[seg.length - 1].i)},${y(0)}Z`}
                  fill={color}
                  opacity={0.1}
                />
              )}
              <path d={seg.map((p, j) => `${j ? "L" : "M"}${x(p.i)},${y(p.v)}`).join("")} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
            </g>
          ))}
          {props.labels.map((label, i) =>
            i % every === 0 || i === n - 1 ? (
              <text key={i} x={x(i)} y={height - 6} textAnchor="middle" className="fill-muted-foreground text-[11px] tabular-nums">
                {label}
              </text>
            ) : null,
          )}
          {hover !== null && <line x1={x(hover)} x2={x(hover)} y1={PAD.top} y2={y(lo)} stroke="var(--chart-axis)" strokeWidth={1} />}
          {lastIdx >= 0 && <circle cx={x(lastIdx)} cy={y(props.values[lastIdx]!)} r={4} fill={color} stroke="var(--card)" strokeWidth={2} />}
          {hover !== null && props.values[hover] !== null && hover !== lastIdx && (
            <circle cx={x(hover)} cy={y(props.values[hover]!)} r={4} fill={color} stroke="var(--card)" strokeWidth={2} />
          )}
        </svg>
      )}
      {hover !== null && (
        <Tip x={x(hover)} width={width}>
          <div className="mb-1 text-muted-foreground">{props.title ? props.title(hover) : props.labels[hover]}</div>
          <TipRow color={color} value={props.values[hover] === null ? "—" : props.format(props.values[hover]!)} name={props.name} />
        </Tip>
      )}
    </div>
  );
}

/** Part-to-whole at a glance (≤ 6 slices; fold the rest into 其他 before calling). */
export function Donut(props: { slices: { key: string; label: string; value: number; color: string }[]; format: Fmt; center?: ReactNode; size?: number }) {
  const size = props.size ?? 168;
  const [hover, setHover] = useState<number | null>(null);
  const total = props.slices.reduce((s, x) => s + x.value, 0);
  const R = size / 2;
  const r0 = R - 18;
  const gap = props.slices.length > 1 ? 0.012 : 0; // radians of surface gap on each side
  let a = -Math.PI / 2;
  const arcs = props.slices.map((s) => {
    const span = total > 0 ? (s.value / total) * Math.PI * 2 : 0;
    const [a0, a1] = [a + gap, a + span - gap];
    a += span;
    if (a1 <= a0) return "";
    const pt = (rad: number, ang: number) => `${R + rad * Math.cos(ang)},${R + rad * Math.sin(ang)}`;
    const large = a1 - a0 > Math.PI ? 1 : 0;
    if (span >= Math.PI * 2 - 1e-6) {
      return `M${pt(R, -Math.PI / 2)}A${R},${R} 0 1 1 ${pt(R, Math.PI * 1.5 - 1e-4)}ZM${pt(r0, -Math.PI / 2)}A${r0},${r0} 0 1 1 ${pt(r0, Math.PI * 1.5 - 1e-4)}Z`;
    }
    return `M${pt(R, a0)}A${R},${R} 0 ${large} 1 ${pt(R, a1)}L${pt(r0, a1)}A${r0},${r0} 0 ${large} 0 ${pt(r0, a0)}Z`;
  });
  const h = hover !== null ? props.slices[hover] : null;
  return (
    <div className="relative shrink-0" style={{ width: size, height: size }} onPointerLeave={() => setHover(null)}>
      <svg width={size} height={size} role="img" aria-label={props.slices.map((s) => s.label).join("、")}>
        {total === 0 && <circle cx={R} cy={R} r={(R + r0) / 2} fill="none" stroke="var(--chart-grid)" strokeWidth={R - r0} />}
        {arcs.map((d, i) => (
          <path
            key={props.slices[i].key}
            d={d}
            fill={props.slices[i].color}
            fillRule="evenodd"
            opacity={hover === null || hover === i ? 1 : 0.45}
            className="transition-opacity duration-150"
            onPointerEnter={() => setHover(i)}
          />
        ))}
      </svg>
      <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center text-center">
        {h ? (
          <>
            <span className="text-xs text-muted-foreground">{h.label}</span>
            <span className="text-base font-bold">{props.format(h.value)}</span>
            <span className="text-xs text-muted-foreground tabular-nums">{total > 0 ? Math.round((h.value / total) * 100) : 0}%</span>
          </>
        ) : (
          props.center
        )}
      </div>
    </div>
  );
}

/** Ranked rows: label · thin bar · value (also the table view of a chart). */
export function BarList(props: {
  rows: { key: string; label: ReactNode; value: number; color?: string; detail?: ReactNode }[];
  format: Fmt;
  max?: number;
}) {
  const max = props.max ?? Math.max(0, ...props.rows.map((r) => r.value));
  return (
    <ul className="space-y-1.5">
      {props.rows.map((r) => (
        <li key={r.key} className="grid grid-cols-[minmax(4rem,8rem)_1fr_auto] items-center gap-3 text-sm">
          <span className="flex min-w-0 items-center gap-1.5">
            {r.color && <span className="size-2.5 shrink-0 rounded-sm" style={{ background: r.color }} />}
            <span className="truncate">{r.label}</span>
          </span>
          <span className="h-2 overflow-hidden rounded-full bg-muted">
            <span className="block h-full rounded-full" style={{ width: `${max > 0 ? (r.value / max) * 100 : 0}%`, background: r.color ?? "var(--chart-1)" }} />
          </span>
          <span className="text-right tabular-nums">
            {props.format(r.value)}
            {r.detail && <span className="ml-2 text-xs text-muted-foreground">{r.detail}</span>}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Budget meter: fill turns amber near the limit and red past it; the label says so too. */
export function Meter(props: { value: number; max: number }) {
  const ratio = props.max > 0 ? props.value / props.max : 0;
  const color = ratio > 1 ? "var(--destructive)" : ratio >= 0.9 ? "var(--status-doing-fg)" : "var(--status-done-fg)";
  return (
    <div className="h-2 overflow-hidden rounded-full bg-muted" role="meter" aria-valuenow={props.value} aria-valuemax={props.max}>
      <div className="h-full rounded-full" style={{ width: `${Math.min(1, ratio) * 100}%`, background: color }} />
    </div>
  );
}
