import { useEffect, useRef, useState, type ReactNode } from "react";
import { Pencil, Plus } from "lucide-react";
import { InlineMd } from "@/shared/InlineMd";
import { Button } from "@/shared/ui";
import { HEADLESS, SUMMARY, SeekerSort, contactLines } from "./SeekerParts";
import { PointEditor } from "./SeekerPoint";
import { draftLang, type SeekerDraft, type SeekerEdit, type SeekerEntry, type SeekerLang } from "./seekerApi";

/** An entry's heading as the Word export writes it (Rust output.rs `lines`): 中文 时间 · 名称 · 角色 · 地点
 * (教育: 名称 ⇥ 时间, then 专业；说明); English 角色 ⇥ 时间, then 名称, 地点 (教育: 名称 ⇥ 时间, then the degree). */
export function EntryHead({ lang, category, entry: e }: { lang: SeekerLang; category: string; entry: SeekerEntry }) {
  const grey = (lines: string[]) => lines.filter(Boolean).map((l, i) => <p key={i} className="text-muted-foreground">{l}</p>);
  if (lang === "en") {
    const education = category === "教育";
    const top = education || !e.role.trim() ? e.name : e.role;
    const below = education ? [e.role, e.note] : e.role.trim() ? [[e.name, e.location].filter((x) => x.trim()).join(", "), e.note] : [e.location, e.note];
    return <div><div className="flex flex-wrap gap-x-6"><strong className="flex-1">{top || "未命名"}</strong><strong>{e.time}</strong></div>{grey(below)}</div>;
  }
  if (category === "教育") return <div><div className="flex flex-wrap gap-x-6"><strong className="flex-1">{e.name || "未命名"}</strong><span>{e.time}</span></div>{grey([[e.role, e.note].filter(Boolean).join("；")])}</div>;
  return <div><div className="flex flex-wrap gap-x-6"><strong className="w-32">{e.time}</strong><strong className="flex-1">{e.name || "未命名"}</strong><span>{e.role}</span><span>{e.location}</span></div>{grey([e.note])}</div>;
}

/** 预览: the draft laid out like its Word export. */
export function ResumePreview({ draft, second }: { draft: SeekerDraft; second: boolean }) {
  const lang = draftLang(draft);
  return <article className="mx-auto max-w-[794px] border bg-card p-8 text-sm"><h2 className={`text-center text-xl font-bold ${lang === "en" ? "uppercase" : ""}`}>{draft.name}</h2><div className="my-4 text-center">{contactLines(draft, second).map((line) => <p key={line}>{line}</p>)}</div>{draft.sections.map((s) => <section key={s.id} className="mb-5"><h3 className={`mb-3 border-b font-bold ${lang === "en" ? "uppercase" : ""}`}>{s.title}</h3>{s.entries.map((e) => <div key={e.id} className="mb-3">{s.category === SUMMARY ? <p className="whitespace-pre-wrap">{e.note}</p> : !HEADLESS.includes(s.category ?? "") && <EntryHead lang={lang} category={s.category ?? ""} entry={e} />}<ul className="mt-1 list-disc pl-5">{e.points.map((p) => <li key={p.id} className="whitespace-pre-wrap"><InlineMd text={p.text} /></li>)}</ul></div>)}</section>)}</article>;
}

export function ResumeEditor(props: { draft: SeekerDraft; busy: boolean; selected: Set<string>; setSelected: (ids: Set<string>) => void; edit: (edit: SeekerEdit) => void; remove: () => void; reorder: (kind: string, parent: string | null, ids: string[]) => void; onDraft: (draft: SeekerDraft) => void }) {
  const { draft, busy, selected, setSelected, edit } = props;
  const lang = draftLang(draft);
  const scroll = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; clientX: number; clientY: number; base: Set<string>; pointerId: number } | null>(null);
  const frame = useRef(0);
  const [box, setBox] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  useEffect(() => () => cancelAnimationFrame(frame.current), []);

  const updateSelection = (animate: boolean) => {
    const root = scroll.current;
    const active = drag.current;
    if (!root || !active) return;
    const bounds = root.getBoundingClientRect();
    // only while the pointer is past the edge (or within 8 px of it)
    if (animate) root.scrollTop += active.clientY < bounds.top + 8 ? -8 : active.clientY > bounds.bottom - 8 ? 8 : 0;
    const x = active.clientX - bounds.left + root.scrollLeft;
    const y = active.clientY - bounds.top + root.scrollTop;
    const area = { left: Math.min(active.x, x), top: Math.min(active.y, y), width: Math.abs(x - active.x), height: Math.abs(y - active.y) };
    setBox(area);
    const next = new Set(active.base);
    root.querySelectorAll<HTMLElement>("[data-seeker-block]").forEach((row) => {
      const r = row.getBoundingClientRect();
      const left = r.left - bounds.left + root.scrollLeft;
      const top = r.top - bounds.top + root.scrollTop;
      if (left <= area.left + area.width && left + r.width >= area.left && top <= area.top + area.height && top + r.height >= area.top) next.add(row.dataset.seekerBlock!);
    });
    setSelected(next);
    if (animate) frame.current = requestAnimationFrame(() => updateSelection(true));
  };

  const block = (id: string, content: ReactNode, onEdit: () => void, inherited = false, handle: ReactNode = null) => <div key={id} data-seeker-block={id} className={`group flex items-start gap-2 rounded-control px-2 py-1.5 ${selected.has(id) || inherited ? "bg-accent ring-1 ring-ring/30" : "hover:bg-muted/50"}`}>
    {handle}<input type="checkbox" className="mt-1 shrink-0" checked={selected.has(id)} disabled={busy} aria-label={`选择${typeof content === "string" ? content : "简历内容"}`} onChange={() => { const next = new Set(selected); if (next.has(id)) next.delete(id); else next.add(id); setSelected(next); }} />
    <div className="min-w-0 flex-1 whitespace-pre-wrap break-words">{content}</div>
    <Button variant="ghost" className="px-1 py-0.5 opacity-0 group-hover:opacity-100 focus:opacity-100" aria-label="编辑此项" disabled={busy} onClick={onEdit}><Pencil size={13} /></Button>
  </div>;

  return <div ref={scroll} tabIndex={0} aria-label="简历正文，多选编辑" className="relative -m-1 min-h-0 flex-1 overflow-auto p-1 outline-none focus-visible:ring-2 focus-visible:ring-ring"
    onKeyDown={(e) => {
      if ((e.target as HTMLElement).closest("button,input,textarea,[contenteditable]")) return;
      if (e.key === "Escape") {
        cancelAnimationFrame(frame.current);
        if (drag.current && e.currentTarget.hasPointerCapture(drag.current.pointerId)) e.currentTarget.releasePointerCapture(drag.current.pointerId);
        drag.current = null;
        setBox(null);
        setSelected(new Set());
      }
      if (!busy && selected.size && (e.key === "Delete" || e.key === "Backspace")) { e.preventDefault(); props.remove(); }
    }}
    onPointerDown={(e) => {
      if (busy || e.button !== 0 || (e.target as HTMLElement).closest("button,input,textarea,a,[contenteditable]")) return;
      const root = e.currentTarget;
      const rect = root.getBoundingClientRect();
      e.preventDefault();
      root.focus();
      root.setPointerCapture(e.pointerId);
      drag.current = { x: e.clientX - rect.left + root.scrollLeft, y: e.clientY - rect.top + root.scrollTop, clientX: e.clientX, clientY: e.clientY, base: e.metaKey || e.ctrlKey || e.shiftKey ? new Set(selected) : new Set(), pointerId: e.pointerId };
      cancelAnimationFrame(frame.current);
      updateSelection(true);
    }}
    onPointerMove={(e) => { if (drag.current) { drag.current.clientX = e.clientX; drag.current.clientY = e.clientY; } }}
    onPointerUp={(e) => { if (!drag.current) return; cancelAnimationFrame(frame.current); drag.current.clientX = e.clientX; drag.current.clientY = e.clientY; updateSelection(false); drag.current = null; setBox(null); if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId); }}
    onLostPointerCapture={() => { cancelAnimationFrame(frame.current); drag.current = null; setBox(null); }}
    onPointerCancel={() => { cancelAnimationFrame(frame.current); drag.current = null; setBox(null); }}>
    <article className="mx-auto my-5 min-h-[650px] max-w-[794px] select-none rounded-sm border bg-card p-6 text-sm shadow-sm sm:p-8">
      <h2 className={`mb-3 text-center text-xl font-bold ${lang === "en" ? "uppercase" : ""}`}>{draft.name || "姓名"}</h2>
      <div className="mb-6 grid gap-1 sm:grid-cols-2">{draft.header.filter((h) => h.visible).map((h) => block(h.id, `${h.label}：${h.value || "—"}`, () => edit({ title: h.label, fields: [{ label: h.label, value: h.value }], apply: (d, values) => { d.header.find((field) => field.id === h.id)!.value = values[0]; } })))}</div>
      <SeekerSort items={draft.sections} disabled={busy} onSort={(ids) => props.reorder("section", null, ids)}>{(section, sectionHandle) => <section key={section.id} className="mb-6">
        <div className="border-b font-bold">{block(section.id, section.title || "未命名章节", () => edit({ title: "章节名称", fields: [{ label: "名称", value: section.title }], apply: (d, values) => { d.sections.find((s) => s.id === section.id)!.title = values[0]; } }), false, sectionHandle)}</div>
        <SeekerSort items={section.entries} disabled={busy} onSort={(ids) => props.reorder("entry", section.id, ids)}>{(entry, entryHandle) => <div key={entry.id} className="mt-3">
          {section.category === SUMMARY ? block(entry.id, <p>{entry.note || "—"}</p>, () => edit({ title: "Summary", fields: [{ label: "正文", value: entry.note }], apply: (d, v) => { d.sections.find((s) => s.id === section.id)!.entries.find((e) => e.id === entry.id)!.note = v[0]; } }), selected.has(section.id), entryHandle)
            : !HEADLESS.includes(section.category ?? "") && block(entry.id, <EntryHead lang={lang} category={section.category ?? ""} entry={entry} />, () => edit({ title: "经历", fields: [{ label: "时间", value: entry.time }, { label: "名称", value: entry.name }, { label: "角色", value: entry.role }, { label: "地点", value: entry.location }, { label: "说明", value: entry.note }], apply: (d, v) => { const e = d.sections.find((s) => s.id === section.id)!.entries.find((e) => e.id === entry.id)!; [e.time, e.name, e.role, e.location, e.note] = v; } }), selected.has(section.id), entryHandle)}
          {section.category !== SUMMARY && <><div className={HEADLESS.includes(section.category ?? "") ? "" : "pl-4"}><SeekerSort items={entry.points} disabled={busy} onSort={(ids) => props.reorder("point", entry.id, ids)}>{(point, pointHandle) => block(point.id, <PointEditor label="要点" value={point.text} disabled={busy} onChange={(text) => { const next = structuredClone(draft); next.sections.find((s) => s.id === section.id)!.entries.find((e) => e.id === entry.id)!.points.find((p) => p.id === point.id)!.text = text; props.onDraft(next); }} />, () => edit({ title: "要点", fields: [{ label: "正文", value: point.text }], apply: (d, v) => { d.sections.find((s) => s.id === section.id)!.entries.find((e) => e.id === entry.id)!.points.find((p) => p.id === point.id)!.text = v[0]; } }), selected.has(section.id) || selected.has(entry.id), pointHandle)}</SeekerSort></div>
          <Button className="ml-4" variant="ghost" disabled={busy} onClick={() => edit({ title: "新要点", fields: [{ label: "正文", value: "" }], apply: (d, v) => { d.sections.find((s) => s.id === section.id)!.entries.find((e) => e.id === entry.id)!.points.push({ id: crypto.randomUUID(), text: v[0] }); } })}><Plus size={13} />要点</Button></>}
        </div>}</SeekerSort>
        {section.category !== SUMMARY && <Button variant="ghost" disabled={busy} onClick={() => edit({ title: "新经历", fields: [{ label: "时间", value: "" }, { label: "名称", value: "" }, { label: "角色", value: "" }, { label: "地点", value: "" }, { label: "说明", value: "" }], apply: (d, v) => { d.sections.find((s) => s.id === section.id)!.entries.push({ id: crypto.randomUUID(), time: v[0], name: v[1], role: v[2], location: v[3], note: v[4], points: [] }); } })}><Plus size={13} />经历</Button>}
      </section>}</SeekerSort>
    </article>
    {box && <div aria-hidden="true" className="pointer-events-none absolute z-10 border border-primary bg-primary/10" style={box} />}
  </div>;
}
