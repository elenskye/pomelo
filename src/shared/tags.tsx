// Option / 类型 tags colored by their position in the list (index.css --tag-1 … --tag-8, then again):
// 五花八门's 单选 / 多选 options, 循序渐进's 类型. Pomelo picks the color; there is no setting.
// TagSelect / TagMulti: the tag(s) in a table cell, a click opens the menu to switch — like the status
// pills of 摸鱼读读 (no arrow, no 「不选」; clearing is in the panel). Clicks don't reach the row.
import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { Dropdown } from "./Dropdown";

const COLORS = 8;

/** Colors for the option at `index` (-1, an unknown one: gray). */
export function tagStyle(index: number): CSSProperties {
  const n = index < 0 ? COLORS : (index % COLORS) + 1;
  return { background: `var(--tag-${n}-bg)`, color: `var(--tag-${n}-fg)` };
}

export function Tag(props: { index: number; children: ReactNode; className?: string }) {
  return (
    <span style={tagStyle(props.index)} className={`inline-block rounded-md px-2 py-0.5 text-xs whitespace-nowrap ${props.className ?? ""}`}>
      {props.children}
    </span>
  );
}

const TRIGGER = "inline-flex max-w-full items-center rounded-md outline-none transition-opacity duration-150 hover:opacity-80 focus-visible:ring-2 focus-visible:ring-ring";
/** Where an empty one is clicked. */
const EMPTY = <span className="inline-block h-5 w-10 rounded-md transition-colors duration-150 hover:bg-muted" />;

/** One of `options`. */
export function TagSelect<T extends string | number>(props: { label: string; options: { value: T; name: string }[]; value: T | null; onChange: (v: T) => void }) {
  const at = props.options.findIndex((o) => o.value === props.value);
  return (
    <Dropdown<T | null>
      ariaLabel={props.label}
      value={props.value}
      onChange={(v) => v !== null && props.onChange(v)}
      menuMaxHeight={280}
      options={props.options.map((o, i) => ({ value: o.value as T | null, label: <Tag index={i}>{o.name}</Tag> }))}
      triggerClassName={TRIGGER}
      renderTrigger={() =>
        at >= 0 ? (
          <Tag index={at} className="truncate">
            {props.options[at].name}
          </Tag>
        ) : (
          EMPTY
        )
      }
    />
  );
}

/** Any of `options`, in their order; the menu stays open while ticking. */
export function TagMulti(props: { label: string; options: string[]; value: string[]; onChange: (v: string[]) => void }) {
  // ticks show at once and add up, even before the saved value comes back
  const [picked, setPicked] = useState(props.value);
  const saved = props.value.join("\u0000");
  useEffect(() => setPicked(props.value), [saved]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (o: string) => {
    const next = picked.includes(o) ? picked.filter((x) => x !== o) : props.options.filter((x) => x === o || picked.includes(x));
    setPicked(next);
    props.onChange(next);
  };
  return (
    <Dropdown<string>
      ariaLabel={props.label}
      value=""
      onChange={toggle}
      isPicked={(o) => picked.includes(o)}
      menuMaxHeight={280}
      options={props.options.map((o, i) => ({ value: o, label: <Tag index={i}>{o}</Tag> }))}
      triggerClassName={TRIGGER}
      renderTrigger={() =>
        picked.length ? (
          <span className="flex min-w-0 gap-1 overflow-hidden">
            {picked.map((o) => (
              <Tag key={o} index={props.options.indexOf(o)}>
                {o}
              </Tag>
            ))}
          </span>
        ) : (
          EMPTY
        )
      }
    />
  );
}
