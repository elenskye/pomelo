import type { ReactNode } from "react";
import { diff } from "@/shared/diff";
import { DragHandle, useSortable } from "@/shared/sortable";
import type { SeekerDraft, SeekerRecord } from "./seekerApi";

export function SeekerDiff({ before, after }: { before: string; after: string }) {
  return <div className="max-h-[45vh] overflow-auto whitespace-pre-wrap break-words rounded-control border p-3 text-sm">{diff(before, after).map((part, i) => part.kind === "del" ? <del key={i} className="bg-destructive/10 text-destructive">{part.text}</del> : part.kind === "add" ? <ins key={i} className="bg-primary/10 text-primary">{part.text}</ins> : <span key={i}>{part.text}</span>)}</div>;
}

/** 「2026 / 10 / 23 17:33」 */
export function when(iso: string): string {
  const d = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function draftText(d: SeekerDraft | null | undefined): string {
  if (!d) return "";
  return [d.name, d.header.filter((h) => h.visible).map((h) => `${h.label}：${h.value}`).join("\n"), ...d.sections.map((s) => [s.title, ...s.entries.map((e) => [[e.time, e.name, e.role, e.location].filter(Boolean).join("｜"), ...(e.note ? [e.note] : []), ...e.points.map((p) => `• ${p.text}`)].join("\n"))].join("\n\n")), `目标 JD：\n${d.jd}`].join("\n\n");
}
export function recordText(r: SeekerRecord | null | undefined): string {
  if (!r) return "";
  return [r.title, [r.time, r.role, r.location].filter(Boolean).join("｜"), r.text, ...r.points.map((p) => `• ${p.text}`), ...r.fields.map((f) => `${f.label}：${f.value}`), r.enabled ? "" : "（已停用）"].filter(Boolean).join("\n");
}

/** Each instance is a sibling group. Numeric positions adapt the existing pointer sorter to block IDs. */
export function SeekerSort<T extends { id: string }>(props: { items: T[]; disabled: boolean; onSort: (ids: string[]) => void; children: (item: T, handle: ReactNode) => ReactNode }) {
  const sort = useSortable((positions) => props.onSort(positions.map((i) => props.items[i].id)));
  return <div ref={sort.listRef}>{props.items.map((item, i) => <div key={item.id} {...sort.item(i)} className="group relative">{props.children(item, props.disabled ? null : <DragHandle {...sort.handle(i)} />)}</div>)}</div>;
}
