import type { ReactNode } from "react";
import { diff } from "@/shared/diff";
import { DragHandle, useSortable } from "@/shared/sortable";
import type { SeekerDraft, SeekerRecord } from "./seekerApi";

/** Sections without entry headings (their points only) and the Summary (one paragraph). */
export const HEADLESS = ["技能", "证书"];
export const SUMMARY = "摘要";

export function SeekerDiff({ before, after }: { before: string; after: string }) {
  return <div className="max-h-[45vh] overflow-auto whitespace-pre-wrap break-words rounded-control border p-3 text-sm">{diff(before, after).map((part, i) => part.kind === "del" ? <del key={i} className="bg-destructive/10 text-destructive">{part.text}</del> : part.kind === "add" ? <ins key={i} className="bg-primary/10 text-primary">{part.text}</ins> : <span key={i}>{part.text}</span>)}</div>;
}

/** 「2026 / 10 / 23 17:33」 */
export function when(iso: string): string {
  const d = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** A web address, not an e-mail (output.rs `is_link`). */
export function isLink(value: string): boolean {
  const v = value.trim().toLowerCase();
  if (v.includes("@") || /\s/.test(v)) return false;
  if (v.includes("://") || v.startsWith("www.")) return true;
  return /^[^/]+\.[a-z]{2,}(\/|$)/.test(v);
}

/** 页首 as the Word export writes it: links on a second line by name when the template has one. */
export function contactLines(d: SeekerDraft, second: boolean): string[] {
  const shown = d.header.filter((h) => h.visible && h.value.trim());
  const links = second ? shown.filter((h) => isLink(h.value)) : [];
  return [shown.filter((h) => !links.includes(h)).map((h) => h.value.trim()).join(" · "), links.map((h) => `${h.label.trim()}：${h.value.trim()}`).join(" · ")].filter(Boolean);
}

export function draftText(d: SeekerDraft | null | undefined): string {
  if (!d) return "";
  const en = d.lang === "en";
  // English: the heading as the export writes it (角色｜时间, then 名称, 地点); Summary and points-only sections without one
  const head = (category: string, e: SeekerDraft["sections"][number]["entries"][number]) => !en ? [[e.name, e.role, e.time].filter(Boolean).join("｜")] : category === SUMMARY || HEADLESS.includes(category) ? [] : category === "教育" || !e.role ? [[e.name, e.time].filter(Boolean).join("｜"), e.role, e.location] : [[e.role, e.time].filter(Boolean).join("｜"), [e.name, e.location].filter(Boolean).join(", ")];
  return [...(en ? [d.form === "cv" ? "CV" : "Resume"] : []), d.name, d.header.filter((h) => h.visible).map((h) => `${h.label}：${h.value}`).join("\n"), ...d.sections.map((s) => [s.title, ...s.entries.map((e) => [...head(s.category ?? "", e).filter(Boolean), ...(e.note ? [e.note] : []), ...e.points.map((p) => `• ${p.text}`)].join("\n"))].join("\n\n")), `目标 JD：\n${d.jd}`].join("\n\n");
}
export function recordText(r: SeekerRecord | null | undefined): string {
  if (!r) return "";
  return [r.title, [r.time, r.role, r.location].filter(Boolean).join("｜"), r.text, r.stack ? `技术栈：${r.stack}` : "", ...r.points.map((p, i) => (r.kind === "rule" ? `${i + 1}. ${p.text}` : `• ${p.text}`)), ...r.fields.map((f) => `${f.label}：${f.value}`), r.enabled ? "" : "（已停用）"].filter(Boolean).join("\n");
}

/** Each instance is a sibling group. Numeric positions adapt the existing pointer sorter to block IDs. */
export function SeekerSort<T extends { id: string }>(props: { items: T[]; disabled: boolean; onSort: (ids: string[]) => void; children: (item: T, handle: ReactNode) => ReactNode }) {
  const sort = useSortable((positions) => props.onSort(positions.map((i) => props.items[i].id)));
  return <div ref={sort.listRef}>{props.items.map((item, i) => <div key={item.id} {...sort.item(i)} className="group relative">{props.children(item, props.disabled ? null : <DragHandle {...sort.handle(i)} />)}</div>)}</div>;
}
