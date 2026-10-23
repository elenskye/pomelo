import { useState } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { InlineMd } from "@/shared/InlineMd";
import { Modal } from "@/shared/Modal";
import { Button, EmptyState, SearchInput, TextArea, TextInput } from "@/shared/ui";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { SeekerDiff, recordText, when } from "./SeekerParts";
import { seekerApi, type SeekerCatalog, type SeekerImport, type SeekerRecord, type SeekerWorkspace } from "./seekerApi";

export const KINDS = [{ value: "material", label: "简历素材" }, { value: "profile", label: "基本信息" }, { value: "fact", label: "已确认事实" }, { value: "rule", label: "写作规则" }, { value: "example", label: "个人范例" }, { value: "application", label: "网申资料" }];
/** A rule's category says which model task reads it (Rust model.rs `context`). */
const RULE_USES = [{ value: "共用准则", label: "共用准则" }, { value: "素材润色", label: "素材润色" }, { value: "整体审查", label: "整体审查（选材、审查）" }, { value: "写作规则", label: "所有任务" }];
export type SeekerWrite = (action: () => Promise<SeekerWorkspace>) => Promise<boolean>;

export function SeekerLibrary(props: { catalog: SeekerCatalog; kinds: string[]; busy: boolean; write: SeekerWrite; onPolish: (id: string) => void; aiBusy: boolean }) {
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<SeekerRecord | null>(null);
  const [past, setPast] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const toast = useToast();
  const rows = props.catalog.records.filter((r) => props.kinds.includes(r.kind) && `${r.title}\n${r.text}\n${r.points.map((p) => p.text).join("\n")}`.toLowerCase().includes(query.toLowerCase()));
  const create = () => setEditing({ id: crypto.randomUUID(), kind: props.kinds[0], category: props.kinds[0] === "application" ? "常见问答" : props.kinds[0] === "material" ? "项目" : "写作规则", title: "", text: "", time: "", role: "", location: "", points: [], fields: [], source: null, revision: 0, enabled: true });
  return <div className="flex min-h-0 flex-1 flex-col gap-3 py-4">
    <div className="flex gap-2"><SearchInput aria-label="搜索资料" value={query} onChange={(e) => setQuery(e.target.value)} /><Button disabled={props.busy} onClick={create}><Plus size={14} />新增</Button></div>
    <div className="-m-1 flex-1 space-y-3 overflow-auto p-1">{!rows.length && <EmptyState title="暂无资料" />}{rows.map((r) => <article key={r.id} className="rounded-card border bg-card p-4">
      <div className="flex flex-wrap items-center gap-2"><strong className="mr-auto">{r.title}</strong><span className="text-xs text-muted-foreground">{r.kind === "rule" ? `用于${RULE_USES.find((u) => u.value === r.category)?.label ?? r.category}` : r.category}{r.enabled ? "" : " · 已停用"}</span><Button disabled={props.busy} onClick={() => setEditing(structuredClone(r))}>编辑</Button><Button variant="ghost" disabled={props.aiBusy || props.busy} onClick={() => props.onPolish(r.id)}>润色</Button><Button variant="ghost" onClick={() => setPast(r.id)}>历史</Button>
        {r.kind === "application" && <Button variant="ghost" onClick={() => void navigator.clipboard.writeText(r.text).then(() => toast("已复制"), (e) => toast(errorText(e), "error"))}><Copy size={14} />复制</Button>}
        <Button variant="ghost-danger" disabled={props.busy} aria-label={`删除${r.title}`} onClick={async () => { if (await confirm({ title: "删除资料", message: `删除「${r.title}」？已有简历保留原文。`, danger: true })) void props.write(() => seekerApi.recordDelete(r.id)); }}><Trash2 size={14} /></Button>
      </div>
      <p className="mt-2 whitespace-pre-wrap text-sm">{r.text}</p>{r.points.length > 0 && <ul className="mt-2 list-disc space-y-1 pl-5 text-sm">{r.points.map((p) => <li key={p.id}><InlineMd text={p.text} /></li>)}</ul>}
      {r.fields.map((f) => <p key={f.id} className="text-sm">{f.label}：{f.value}</p>)}
    </article>)}</div>
    {editing && <RecordEditor initial={editing} kinds={props.kinds} busy={props.busy} write={props.write} close={() => setEditing(null)} />}
    {past && <Modal wide title="资料历史" error={null} onClose={() => setPast(null)} onSubmit={(e) => { e.preventDefault(); setPast(null); }} submitLabel="完成"><div className="max-h-[55vh] space-y-4 overflow-auto">{props.catalog.revisions.filter((r) => r.recordId === past).map((r) => <div key={r.id}><p className="mb-2 text-xs text-muted-foreground">{when(r.savedAt)}</p><SeekerDiff before={recordText(r.body)} after={recordText(props.catalog.records.find((v) => v.id === past))} /><Button disabled={props.busy} onClick={() => { const current = props.catalog.records.find((v) => v.id === past); setEditing({ ...r.body, revision: current?.revision ?? 0 }); setPast(null); }}>以此版本编辑</Button></div>)}{!props.catalog.revisions.some((r) => r.recordId === past) && <p>还没有历史版本</p>}</div></Modal>}
    {confirmDialog}
  </div>;
}

function RecordEditor(props: { initial: SeekerRecord; kinds: string[]; busy: boolean; write: SeekerWrite; close: () => void }) {
  const [value, setValue] = useState(props.initial);
  const [review, setReview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return <Modal wide title={review ? "确认资料修改" : "编辑资料"} error={error} onClose={props.close} submitDisabled={props.busy || !value.title.trim()} submitLabel={review ? "确认保存" : "查看差异"} footerLeft={review ? <Button disabled={props.busy} onClick={() => setReview(false)}>返回编辑</Button> : undefined} onSubmit={(e) => { e.preventDefault(); if (!review) { setReview(true); return; } void props.write(() => seekerApi.recordSave(value)).then((ok) => ok ? props.close() : setError("未保存，请重试或重新打开当前资料")); }}>
    {review ? <SeekerDiff before={props.initial.revision ? recordText(props.initial) : ""} after={recordText(value)} /> : <fieldset disabled={props.busy} className="-m-1 max-h-[58vh] space-y-3 overflow-auto p-1">
      <div className="flex gap-3">{props.kinds.length > 1 && <label className="block space-y-1 text-sm"><span>类型</span><Dropdown ariaLabel="类型" value={value.kind} options={KINDS.filter((k) => props.kinds.includes(k.value))} onChange={(kind) => setValue({ ...value, kind, category: kind === "rule" ? "写作规则" : kind === "example" ? "个人范例" : value.category })} /></label>}
        <label className="block space-y-1 text-sm"><span>{value.kind === "rule" ? "用于" : "分类"}</span>{value.kind === "rule" ? <Dropdown ariaLabel="用于" value={value.category} options={RULE_USES} onChange={(category) => setValue({ ...value, category })} /> : <TextInput value={value.category} onChange={(e) => setValue({ ...value, category: e.target.value })} />}</label></div>
      <label className="block text-sm">名称<TextInput value={value.title} onChange={(e) => setValue({ ...value, title: e.target.value })} /></label>
      {value.kind === "material" && <div className="grid grid-cols-3 gap-2">{(["time", "role", "location"] as const).map((key, i) => <label key={key} className="text-sm">{["时间", "角色", "地点"][i]}<TextInput value={value[key]} onChange={(e) => setValue({ ...value, [key]: e.target.value })} /></label>)}</div>}
      {value.kind !== "profile" && <label className="block text-sm">{value.kind === "material" ? "说明" : "正文"}<TextArea rows={value.kind === "material" ? 2 : 8} value={value.text} onChange={(e) => setValue({ ...value, text: e.target.value })} /></label>}
      {value.kind === "material" && <div className="space-y-2">{value.points.map((p, i) => <div key={p.id} className="flex gap-2"><TextArea aria-label={`要点 ${i + 1}`} rows={3} value={p.text} onChange={(e) => setValue({ ...value, points: value.points.map((v) => v.id === p.id ? { ...v, text: e.target.value } : v) })} /><Button variant="ghost-danger" aria-label="删除要点" onClick={() => setValue({ ...value, points: value.points.filter((v) => v.id !== p.id) })}><Trash2 size={14} /></Button></div>)}<Button onClick={() => setValue({ ...value, points: [...value.points, { id: crypto.randomUUID(), text: "" }] })}>增加要点</Button></div>}
      {value.kind === "profile" && <div className="space-y-2">{value.fields.map((f) => <div key={f.id} className="flex gap-2"><TextInput aria-label="字段名" value={f.label} onChange={(e) => setValue({ ...value, fields: value.fields.map((v) => v.id === f.id ? { ...v, label: e.target.value } : v) })} /><TextInput aria-label="字段值" value={f.value} onChange={(e) => setValue({ ...value, fields: value.fields.map((v) => v.id === f.id ? { ...v, value: e.target.value } : v) })} /><Button variant="ghost-danger" onClick={() => setValue({ ...value, fields: value.fields.filter((v) => v.id !== f.id) })}>删除</Button></div>)}<Button onClick={() => setValue({ ...value, fields: [...value.fields, { id: crypto.randomUUID(), label: "", value: "", visible: true }] })}>增加字段</Button></div>}
      <label className="flex gap-2 text-sm"><input type="checkbox" checked={value.enabled} onChange={(e) => setValue({ ...value, enabled: e.target.checked })} />用于选材或写作参考</label>
    </fieldset>}
  </Modal>;
}

export function SeekerImportDialog(props: { preview: SeekerImport; busy: boolean; write: SeekerWrite; close: () => void }) {
  const [items, setItems] = useState(props.preview.items);
  const [chosen, setChosen] = useState(new Set(props.preview.items.filter((i) => !i.current).map((i) => i.incoming.id)));
  const [error, setError] = useState<string | null>(null);
  return <Modal wide title="导入预览" onClose={props.close} error={error} submitLabel={`确认导入 ${chosen.size} 项`} submitDisabled={props.busy || !chosen.size} onSubmit={(e) => { e.preventDefault(); void props.write(() => seekerApi.import(items.filter((i) => chosen.has(i.incoming.id)).map((i) => i.incoming))).then((ok) => ok ? props.close() : setError("导入未完成；若资料已变化，请重新预览")); }}>
    <fieldset disabled={props.busy} className="-m-1 max-h-[60vh] space-y-5 overflow-auto p-1">{items.map((item, index) => <section key={item.incoming.id} className="space-y-2"><div className="flex items-center gap-2"><strong className="mr-auto text-sm">{item.incoming.title}</strong><label className="flex gap-1 text-sm"><input type="checkbox" checked={chosen.has(item.incoming.id)} onChange={() => setChosen((previous) => { const next = new Set(previous); if (next.has(item.incoming.id)) next.delete(item.incoming.id); else next.add(item.incoming.id); return next; })} />{item.current ? "采用文件版" : "导入"}</label></div>
      <div className="flex gap-2"><Dropdown ariaLabel="导入类型" value={item.incoming.kind} options={KINDS} onChange={(kind) => setItems(items.map((v, i) => i === index ? { ...v, incoming: { ...v.incoming, kind } } : v))} /><TextInput aria-label="导入分类" value={item.incoming.category} onChange={(e) => setItems(items.map((v, i) => i === index ? { ...v, incoming: { ...v.incoming, category: e.target.value } } : v))} /></div>
      <p className="break-all text-xs text-muted-foreground">{item.incoming.source}{item.current && !chosen.has(item.incoming.id) ? " · 保留当前版" : ""}</p><SeekerDiff before={recordText(item.current)} after={recordText(item.incoming)} />
    </section>)}{!items.length && <p>没有可导入的正文</p>}{props.preview.skipped.length > 0 && <details><summary className="text-sm">未导入的文件（{props.preview.skipped.length}）</summary>{props.preview.skipped.map((s, i) => <p key={i} className="text-xs text-muted-foreground">{s}</p>)}</details>}</fieldset>
  </Modal>;
}
