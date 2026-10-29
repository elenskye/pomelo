// Several cards at once and the user's own order, for Seeker's and Rater's lists (v0.7 refine):
// 「选择」 turns a click on a card into picking it (its background shows it; shift-click picks a range,
// ⌘A all shown, Esc leaves) and shows a bar of actions; outside it whole cards drag within their group
// (SortGroup). Writes are Rust's (one undo step each). No tick boxes in v0.7's pages: a choice is a
// button whose background shows it's on (`Chip`, `picked`), a setting a Switch (like Pub2MD).
import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { CheckSquare } from "lucide-react";
import { useSortable } from "@/shared/sortable";
import { Button } from "@/shared/ui";

export type Select = ReturnType<typeof useSelect>;

/** `ids`: the cards shown, in order. */
export function useSelect(ids: string[]) {
  const [on, setOn] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const last = useRef<string | null>(null);
  const key = ids.join("\n");
  // cards that went away (deleted, filtered out) aren't picked any more
  useEffect(() => { setPicked((p) => new Set([...p].filter((id) => ids.includes(id)))); }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const exit = () => { setOn(false); setPicked(new Set()); last.current = null; };
  useEffect(() => {
    if (!on) return;
    const keys = (e: KeyboardEvent) => {
      const typing = ["INPUT", "TEXTAREA"].includes((e.target as HTMLElement).tagName) && (e.target as HTMLInputElement).type !== "checkbox";
      if (e.key === "Escape" && !document.querySelector("dialog[open]")) { e.preventDefault(); exit(); }
      if (e.metaKey && e.key.toLowerCase() === "a" && !typing) { e.preventDefault(); setPicked(new Set(ids)); }
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [on, key]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (id: string, range: boolean) => {
    const from = last.current ? ids.indexOf(last.current) : -1;
    const to = ids.indexOf(id);
    last.current = id;
    setPicked((p) => {
      const n = new Set(p);
      if (range && from >= 0 && to >= 0) {
        for (const x of ids.slice(Math.min(from, to), Math.max(from, to) + 1)) n.add(x);
      } else if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  };
  /** A card's click while selecting: picks it (shift: the range from the last one). */
  const pick = (id: string) => (e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); toggle(id, e.shiftKey); };
  return { on, start: () => setOn(true), exit, picked, has: (id: string) => picked.has(id), toggle, pick, all: () => setPicked(new Set(ids)), count: picked.size, total: ids.length };
}

/** A picked card or row (Pub2MD's look: the border and background say it's on). */
export const picked = (on: boolean) => (on ? "border-primary bg-accent text-accent-foreground" : "");

/** A choice that is on or off (Pub2MD's chips): the background shows it. */
export function Chip(props: { on: boolean; onClick: () => void; disabled?: boolean; className?: string; children: ReactNode }) {
  return <button type="button" role="checkbox" aria-checked={props.on} disabled={props.disabled} onClick={props.onClick} className={`rounded-control border px-2.5 py-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 ${props.on ? "border-primary bg-accent text-accent-foreground" : "hover:bg-muted"} ${props.className ?? ""}`}>{props.children}</button>;
}

/** A row that is picked or not, the whole row the button (its background shows it). `framed`: a card
 * with its border when off too. */
export function PickRow(props: { on: boolean; onClick: (e: MouseEvent) => void; disabled?: boolean; framed?: boolean; className?: string; label?: string; children: ReactNode }) {
  return <button type="button" role="checkbox" aria-checked={props.on} aria-label={props.label} disabled={props.disabled} onClick={props.onClick} className={`flex w-full min-w-0 rounded-control border text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60 ${props.on ? "border-primary bg-accent text-accent-foreground" : props.framed ? "hover:bg-muted" : "border-transparent hover:bg-muted"} ${props.className ?? ""}`}>{props.children}</button>;
}

/** The toolbar's 「选择」. */
export function SelectButton(props: { select: Select; disabled?: boolean }) {
  if (props.select.on) return null;
  return <Button variant="ghost" disabled={props.disabled || !props.select.total} onClick={props.select.start}><CheckSquare size={14} />选择</Button>;
}

/** While selecting: how many, 全选, the actions, 完成. */
export function SelectBar(props: { select: Select; children: ReactNode }) {
  const s = props.select;
  if (!s.on) return null;
  return <div className="flex flex-wrap items-center gap-2 rounded-card border bg-card px-3 py-2 text-sm shadow-sm">
    <span className="mr-1 tabular-nums">已选 {s.count} 项</span>
    <Button variant="ghost" disabled={s.count === s.total} onClick={s.all}>全选</Button>
    <span className="flex-1" />
    {props.children}
    <Button onClick={s.exit}>完成</Button>
  </div>;
}

/** A group whose cards drag into a new order (the whole card is the handle; a plain click still
 * opens it). `children` gets the props to spread on the card. Off while selecting or busy. */
export function SortGroup<T extends { id: string }>(props: { items: T[]; disabled: boolean; onSort: (ids: string[]) => void; className?: string; children: (item: T, drag: Record<string, unknown>) => ReactNode }) {
  const sort = useSortable((positions) => props.onSort(positions.map((i) => props.items[i].id)));
  return <div ref={sort.listRef} className={props.className}>{props.items.map((item, i) => <div key={item.id} {...sort.item(i)}>{props.children(item, props.disabled || props.items.length < 2 ? {} : sort.handle(i))}</div>)}</div>;
}
