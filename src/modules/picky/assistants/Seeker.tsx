import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FileUser, Plus, Trash2, X } from "lucide-react";
import { Dropdown } from "@/shared/Dropdown";
import { InlineMd } from "@/shared/InlineMd";
import { Modal } from "@/shared/Modal";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, EmptyState, SearchInput, Segmented, TextArea, TextInput } from "@/shared/ui";
import { useModels } from "../Schedules";
import { SeekerLibrary, SeekerImportDialog } from "./SeekerLibrary";
import { SeekerHistory } from "./SeekerHistory";
import { SeekerExportDialog, SeekerTemplateSettings } from "./SeekerOutput";
import { ResumeEditor } from "./SeekerResume";
import { SeekerDiff, SeekerSort, draftText, recordText } from "./SeekerParts";
import { flushSeekerDraft, queueSeekerDraft, useSeekerDraft } from "./seekerDraft";
import { SEEKER_KEY, seekerApi, type SeekerCatalog, type SeekerEdit, type SeekerHeader, type SeekerImport, type SeekerSelection, type SeekerSuggestion, type SeekerWorkspace, type SourceChange } from "./seekerApi";

type Tab = "resume" | "library" | "apply" | "history" | "settings";
const EMPTY: SeekerCatalog = { records: [], revisions: [], versions: [], settings: { selectionModel: null, reviewModel: null, thinking: false, cloudAllowed: false, developerRole: false } };

export function SeekerPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const [confirm, confirmDialog] = useConfirm();
  const view = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const local = useSeekerDraft();
  const [tab, setTab] = useState<Tab>("resume");
  const [mode, setMode] = useState<"edit" | "diff" | "preview">("edit");
  const [drawer, setDrawer] = useState<"materials" | "assistant" | null>(null);
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const [edit, setEdit] = useState<SeekerEdit | null>(null);
  const [headers, setHeaders] = useState<SeekerHeader[] | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selections, setSelections] = useState<SeekerSelection[]>([]);
  const [filter, setFilter] = useState("");
  const [category, setCategory] = useState("全部");
  const [preview, setPreview] = useState<SeekerImport | null>(null);
  const [sourceChanges, setSourceChanges] = useState<SourceChange[] | null>(null);
  const [aiBusy, setAiBusy] = useState(false);
  const [suggestion, setSuggestion] = useState<SeekerSuggestion | null>(null);
  const [request, setRequest] = useState("");
  const [exporting, setExporting] = useState(false);
  const [versionName, setVersionName] = useState<string | null>(null);
  const data = view.data;
  const draft = local.draft ?? data?.draft ?? null;
  const catalog = data?.catalog ?? EMPTY;
  const settings = catalog.settings;
  const selectModels = useModels(settings.selectionModel, settings.thinking).models.filter((m) => m.value != null);
  const reviewModels = useModels(settings.reviewModel, settings.thinking).models.filter((m) => m.value != null);
  const flush = () => flushSeekerDraft(qc);
  useEffect(() => () => { void flushSeekerDraft(qc); }, [qc]);
  useEffect(() => { setSelected(new Set()); }, [data?.draft]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "s") { e.preventDefault(); void flushSeekerDraft(qc); }
      // Flush before the app handles undo; pending typed text must not overwrite the undone result.
      if (e.metaKey && e.key.toLowerCase() === "z" && !["INPUT", "TEXTAREA"].includes((e.target as HTMLElement).tagName) && (local.saving || local.error)) {
        e.preventDefault(); e.stopImmediatePropagation(); void flushSeekerDraft(qc).then((ok) => ok && toast("草稿已保存，再按一次撤销"));
      }
    };
    window.addEventListener("keydown", key, true);
    const leaving = (e: BeforeUnloadEvent) => { if (local.saving || local.error) { e.preventDefault(); e.returnValue = ""; void flushSeekerDraft(qc); } };
    window.addEventListener("beforeunload", leaving);
    return () => { window.removeEventListener("keydown", key, true); window.removeEventListener("beforeunload", leaving); };
  }, [qc, local.saving, local.error, toast]);
  const write = async (action: () => Promise<SeekerWorkspace>) => {
    if (lock.current) return false;
    lock.current = true; setBusy(true);
    try {
      if (!await flush()) return false;
      const next = await action(); qc.setQueryData(SEEKER_KEY, next); return true;
    } catch (e) { toast(errorText(e), "error"); return false; }
    finally { lock.current = false; setBusy(false); }
  };
  const begin = (value: SeekerEdit) => { setEditError(null); setEdit(value); };
  const create = async () => {
    if (draft && !await confirm({ title: "新建简历？", message: "从基本信息和教育重新开始，当前草稿会被替换。" })) return;
    if (await write(seekerApi.newDraft)) { setMode("edit"); setSelections([]); }
  };
  const assemble = async (replace: boolean) => {
    if (replace && draft && !await confirm({ title: "重新组装整份草稿？", message: "用所选素材重新组装，当前草稿会被替换。" })) return;
    if (await write(() => seekerApi.assemble(selections, replace))) { setMode("edit"); setDrawer(null); }
  };
  const run = async (kind: string, recordId: string | null = null) => {
    if (aiBusy) return;
    setAiBusy(true);
    try { if (await flush()) setSuggestion(await seekerApi.suggest(kind, request, recordId)); }
    catch (e) { toast(errorText(e), "error"); }
    finally { setAiBusy(false); }
  };
  const importPreview = async () => {
    if (busy) return;
    setBusy(true);
    try { setPreview(await seekerApi.previewImport()); }
    catch (e) { toast(errorText(e), "error"); }
    finally { setBusy(false); }
  };
  if (view.isError) return <EmptyState title="Seeker 暂时打不开" hint={errorText(view.error)} action={<Button onClick={() => void view.refetch()}>重试</Button>} />;
  if (!data) return <EmptyState title="正在读取 Seeker…" />;
  const materials = catalog.records.filter((r) => r.kind === "material" && r.enabled);
  const defaults = (id: string) => data.choices?.find((s) => s.recordId === id)?.pointIds ?? [];
  const materialPanel = <><strong>选材</strong><SearchInput aria-label="搜索素材" value={filter} onChange={(e) => setFilter(e.target.value)} /><Dropdown ariaLabel="素材类别" value={category} options={["全部", ...new Set(materials.map((r) => r.category))].map((c) => ({ value: c, label: c }))} onChange={setCategory} /><div className="-m-1 min-h-0 flex-1 space-y-3 overflow-auto p-1">
    {!materials.length && <p className="py-5 text-sm text-muted-foreground">还没有素材</p>}{materials.filter((r) => (category === "全部" || r.category === category) && `${r.title} ${r.text} ${r.points.map((p) => p.text).join(" ")}`.toLowerCase().includes(filter.toLowerCase())).map((r) => { const picked = selections.find((s) => s.recordId === r.id); return <div key={r.id} className="space-y-2 rounded-control border p-2"><label className="flex gap-2 text-sm"><input type="checkbox" checked={!!picked} disabled={busy} onChange={() => setSelections(picked ? selections.filter((s) => s.recordId !== r.id) : [...selections, { recordId: r.id, pointIds: defaults(r.id) }])} /><strong>{r.title}</strong></label>{picked && r.points.map((p) => <label key={p.id} className="flex gap-2 pl-2 text-xs"><input type="checkbox" checked={picked.pointIds.includes(p.id)} disabled={busy} onChange={() => setSelections(selections.map((s) => s.recordId !== r.id ? s : { ...s, pointIds: s.pointIds.includes(p.id) ? s.pointIds.filter((id) => id !== p.id) : [...s.pointIds, p.id] }))} /><span><InlineMd text={p.text} /></span></label>)}</div>; })}</div><div className="flex flex-wrap gap-1"><Button variant="ghost" disabled={busy} onClick={() => setSelections(materials.map((r) => ({ recordId: r.id, pointIds: defaults(r.id) })))}>全选</Button><Button variant="ghost" onClick={() => setSelections([])}>清空选材</Button></div><div className="flex gap-2"><Button disabled={busy || !selections.length} onClick={() => void assemble(false)}>增加</Button><Button disabled={busy || !selections.length} onClick={() => void assemble(true)}>重新组装</Button></div></>;
  const assistantPanel = <><strong>岗位与建议</strong><label className="mt-2 text-sm">目标 JD<TextArea rows={6} value={draft?.jd ?? ""} disabled={!draft || busy} onChange={(e) => draft && queueSeekerDraft({ ...draft, jd: e.target.value }, qc)} /></label>
    <fieldset disabled={busy || aiBusy} className="space-y-3"><div className="space-y-1 text-sm"><p>选材模型</p><Dropdown value={settings.selectionModel} options={selectModels} placeholder="选择模型" ariaLabel="选材模型" onChange={(value) => void write(() => seekerApi.settings({ ...settings, selectionModel: value }))} /></div><Button disabled={!draft || !settings.selectionModel || !materials.length} onClick={() => void run("select")}>按 JD 推荐素材</Button>
      <label className="flex gap-2 text-sm"><input type="checkbox" checked={settings.developerRole} onChange={(e) => void write(() => seekerApi.settings({ ...settings, developerRole: e.target.checked }))} />纯开发岗</label>
      <div className="space-y-1 text-sm"><p>审查与润色模型</p><Dropdown value={settings.reviewModel} options={reviewModels} placeholder="选择模型" ariaLabel="审查与润色模型" onChange={(value) => void write(() => seekerApi.settings({ ...settings, reviewModel: value }))} /></div>
      <label className="flex gap-2 text-sm"><input type="checkbox" checked={settings.thinking} onChange={(e) => void write(() => seekerApi.settings({ ...settings, thinking: e.target.checked }))} />思考</label>
      <TextArea aria-label="本次要求" rows={3} placeholder="本次要求" value={request} onChange={(e) => setRequest(e.target.value)} /><Button disabled={!draft || !settings.reviewModel} onClick={() => void run("review")}>审查整份简历</Button><Button disabled={!draft || !settings.reviewModel} onClick={() => void run("polish")}>润色要点</Button>
    </fieldset>{aiBusy && <p role="status" className="text-sm text-muted-foreground">正在生成建议…</p>}</>;
  return <div className="relative flex min-h-0 min-w-0 flex-1 flex-col px-6 pb-4">
    <div className="flex flex-wrap items-center gap-4 border-b pb-3"><strong className="flex items-center gap-2"><FileUser size={18} />Seeker</strong><div className="-m-1 overflow-x-auto p-1"><Segmented<Tab> ariaLabel="Seeker 子页面" value={tab} onChange={(v) => { void flush(); setTab(v); setDrawer(null); }} options={[{ value: "resume", label: "简历" }, { value: "library", label: "素材" }, { value: "apply", label: "网申" }, { value: "history", label: "历史" }, { value: "settings", label: "模板与规则" }]} /></div></div>
    {local.error && <div role="alert" className="flex items-center gap-2 py-2 text-sm text-destructive"><span>{local.error}；输入仍保留</span><Button onClick={() => void flush()}>重试保存</Button></div>}
    {tab === "resume" ? <>
      <div className="flex flex-wrap items-center gap-2 py-3"><Button className="xl:hidden" onClick={() => setDrawer("materials")}>选材</Button><Button disabled={!draft || busy} onClick={() => setVersionName("")}>确认版本</Button><Button disabled={!draft} onClick={() => setMode("preview")}>预览</Button><Button disabled={busy} onClick={() => void create()}>新建</Button><Button disabled={!draft || busy} onClick={() => void flush().then((ok) => { if (ok) setExporting(true); })}>导出</Button><span className="flex-1" /><span role="status" className="text-xs text-muted-foreground">{local.error ? "保存失败" : busy || local.saving ? "正在保存…" : data.updatedAt ? "草稿已保存" : "尚无草稿"}</span><Button className="xl:hidden" onClick={() => setDrawer("assistant")}>助手</Button></div>
      <div className="flex min-h-0 flex-1 gap-4"><aside aria-label="选材" className="hidden w-56 shrink-0 flex-col gap-2 rounded-card border bg-card p-4 xl:flex">{materialPanel}</aside><section className="flex min-h-0 min-w-0 flex-1 flex-col rounded-card border bg-muted/30">
        <div className="flex flex-wrap items-center gap-1 border-b p-3"><strong className="mr-auto text-sm">{draft?.name || "中文简历"}</strong><Button disabled={!draft || busy} variant="ghost" onClick={() => draft && begin({ title: "姓名", fields: [{ label: "姓名", value: draft.name }], apply: (d, v) => { d.name = v[0]; } })}>姓名</Button><Button disabled={!draft || busy} variant="ghost" onClick={() => draft && setHeaders(structuredClone(draft.header))}>页首字段</Button><Button disabled={!draft || busy} variant="ghost" onClick={() => begin({ title: "新增章节", fields: [{ label: "章节名称", value: "" }], apply: (d, v) => { d.sections.push({ id: crypto.randomUUID(), title: v[0], category: "", entries: [] }); } })}><Plus size={13} />章节</Button><Button disabled={!draft || busy} variant="ghost" onClick={() => { void flush().then(async (ok) => { if (ok) setSourceChanges(await seekerApi.sources()); }).catch((e) => toast(errorText(e), "error")); }}>来源差异</Button><Segmented value={mode} onChange={setMode} options={[{ value: "edit", label: "编辑" }, { value: "diff", label: "修改差异" }, { value: "preview", label: "预览" }]} /></div>
        {(data.warnings ?? []).map((warning) => <p key={warning} className="px-3 py-1 text-xs text-muted-foreground">{warning}</p>)}
        {!draft ? <EmptyState title="还没有简历草稿" action={<Button disabled={busy} onClick={() => void create()}>新建简历</Button>} /> : mode === "edit" ? <ResumeEditor draft={draft} busy={busy} selected={selected} setSelected={setSelected} edit={begin} remove={() => void write(() => seekerApi.deleteBlocks([...selected]))} onDraft={(d) => queueSeekerDraft(d, qc)} reorder={(kind, parent, ids) => void write(() => seekerApi.reorder(kind, parent, ids))} /> : mode === "diff" ? <div className="-m-1 flex-1 overflow-auto p-4"><SeekerDiff before={draftText(data.baseline)} after={draftText(draft)} /></div> : <div className="-m-1 flex-1 overflow-auto p-4"><article className="mx-auto max-w-[794px] border bg-card p-8 text-sm"><h2 className="text-center text-xl font-bold">{draft.name}</h2><p className="my-4 text-center">{draft.header.filter((h) => h.visible && h.value).map((h) => h.value).join(" · ")}</p>{draft.sections.map((s) => <section key={s.id} className="mb-5"><h3 className="mb-3 border-b font-bold">{s.title}</h3>{s.entries.map((e) => <div key={e.id} className="mb-3">{s.category !== "技能" && <div className="flex flex-wrap gap-x-6">{s.category === "教育" ? <><strong className="flex-1">{e.name}</strong><span>{e.time}</span></> : <><strong className="w-32">{e.time}</strong><strong className="flex-1">{e.name}</strong><span>{e.role}</span><span>{e.location}</span></>}</div>}{(s.category === "教育" ? [e.role, e.note].filter(Boolean).join("；") : e.note) && <p className="text-muted-foreground">{s.category === "教育" ? [e.role, e.note].filter(Boolean).join("；") : e.note}</p>}<ul className="mt-1 list-disc pl-5">{e.points.map((p) => <li key={p.id} className="whitespace-pre-wrap"><InlineMd text={p.text} /></li>)}</ul></div>)}</section>)}</article></div>}
        {selected.size > 0 && mode === "edit" && <div className="flex items-center gap-2 border-t bg-card p-3"><span className="mr-auto text-sm">已选 {selected.size} 项</span><Button variant="ghost-danger" disabled={busy} onClick={() => void write(() => seekerApi.deleteBlocks([...selected]))}><Trash2 size={14} />删除所选</Button><Button variant="ghost" onClick={() => setSelected(new Set())}>取消选择</Button></div>}
      </section><aside aria-label="岗位与建议" className="hidden w-60 shrink-0 flex-col gap-2 overflow-auto rounded-card border bg-card p-4 xl:flex">{assistantPanel}</aside></div>
    </> : tab === "library" || tab === "apply" ? <><div className="flex gap-2 pt-3"><Button disabled={!data.folder || busy} onClick={() => void importPreview()}>从 transfer 导入</Button></div><SeekerLibrary key={tab} catalog={catalog} kinds={tab === "apply" ? ["application"] : ["material", "profile", "fact"]} busy={busy} write={write} onPolish={(id) => void run("material", id)} aiBusy={aiBusy} /></> : tab === "history" ? <SeekerHistory output={data.output} catalog={catalog} draft={draft} busy={busy} write={write} /> : <div className="-m-1 flex-1 overflow-auto p-1"><div className="mx-auto max-w-3xl space-y-5 py-5"><section className="space-y-2"><h2 className="font-bold">Seeker 文件夹</h2><p className="break-all text-sm text-muted-foreground">{data.folder || "未选择"}</p><div className="flex flex-wrap gap-2"><Button disabled={busy} onClick={() => void openDialog({ directory: true, multiple: false, title: "选择 Seeker 文件夹" }).then((path) => { if (typeof path === "string") void write(() => seekerApi.folder(path)); }).catch((e) => toast(errorText(e), "error"))}>选择文件夹</Button><Button disabled={busy || !data.folder} onClick={() => void write(() => seekerApi.folder(""))}>清除</Button><Button disabled={busy || !data.folder} onClick={() => void importPreview()}>预览导入</Button></div></section><label className="flex gap-2 text-sm"><input type="checkbox" checked={settings.cloudAllowed} disabled={busy || aiBusy} onChange={(e) => void write(() => seekerApi.settings({ ...settings, cloudAllowed: e.target.checked }))} />允许将 Seeker 资料发给所选云端模型</label><section><h2 className="font-bold">写作规则与个人范例</h2><SeekerLibrary catalog={catalog} kinds={["rule", "example"]} busy={busy} write={write} onPolish={(id) => void run("material", id)} aiBusy={aiBusy} /></section><SeekerTemplateSettings output={data.output} folder={data.folder} busy={busy} write={write} /></div></div>}
    {drawer && tab === "resume" && <div className="absolute inset-0 z-20 flex justify-end bg-black/20 xl:hidden" onClick={() => setDrawer(null)}><aside className="flex h-full w-80 max-w-full flex-col gap-2 overflow-auto border-l bg-card p-4" onClick={(e) => e.stopPropagation()}><Button className="self-end" variant="ghost" aria-label="关闭面板" onClick={() => setDrawer(null)}><X size={16} /></Button>{drawer === "materials" ? materialPanel : assistantPanel}</aside></div>}
    {exporting && <SeekerExportDialog name={draft?.name ?? ""} flush={flush} saved={(next) => qc.setQueryData(SEEKER_KEY, next)} close={() => setExporting(false)} />}
    {preview && <SeekerImportDialog preview={preview} busy={busy} write={write} close={() => setPreview(null)} />}
    {edit && draft && <Modal title={edit.title} error={editError} onClose={() => { if (!busy) setEdit(null); }} submitLabel="应用到草稿" submitDisabled={busy} onSubmit={(e) => { e.preventDefault(); const next = structuredClone(draft); edit.apply(next, edit.fields.map((f) => f.value)); void write(() => seekerApi.save(next)).then((ok) => ok ? setEdit(null) : setEditError("未保存，请重试")); }}>{edit.fields.map((field, i) => <label key={i} className="block space-y-1 text-sm"><span>{field.label}</span>{field.label === "正文" ? <TextArea autoFocus rows={6} value={field.value} disabled={busy} onChange={(e) => setEdit({ ...edit, fields: edit.fields.map((f, j) => i === j ? { ...f, value: e.target.value } : f) })} /> : <TextInput autoFocus={i === 0} value={field.value} disabled={busy} onChange={(e) => setEdit({ ...edit, fields: edit.fields.map((f, j) => i === j ? { ...f, value: e.target.value } : f) })} />}</label>)}</Modal>}
    {headers && draft && <Modal wide title="页首字段" error={null} onClose={() => { if (!busy) setHeaders(null); }} submitLabel="保存" submitDisabled={busy} onSubmit={(e) => { e.preventDefault(); void write(() => seekerApi.save({ ...draft, header: headers })).then((ok) => { if (ok) setHeaders(null); }); }}><div className="max-h-[55vh] overflow-auto"><SeekerSort items={headers} disabled={busy} onSort={(ids) => setHeaders(ids.map((id) => headers.find((h) => h.id === id)!))}>{(h, grip) => <div className="mb-2 flex items-center gap-2">{grip}<input type="checkbox" aria-label="显示字段" checked={h.visible} onChange={(e) => setHeaders(headers.map((v) => v.id === h.id ? { ...v, visible: e.target.checked } : v))} /><TextInput aria-label="字段名" value={h.label} onChange={(e) => setHeaders(headers.map((v) => v.id === h.id ? { ...v, label: e.target.value } : v))} /><TextInput aria-label="字段值" value={h.value} onChange={(e) => setHeaders(headers.map((v) => v.id === h.id ? { ...v, value: e.target.value } : v))} /><Button variant="ghost-danger" onClick={() => setHeaders(headers.filter((v) => v.id !== h.id))}>删除</Button></div>}</SeekerSort><Button onClick={() => setHeaders([...headers, { id: crypto.randomUUID(), label: "", value: "", visible: true }])}>自定义字段</Button></div></Modal>}
    {versionName !== null && <Modal title="确认版本" error={null} onClose={() => setVersionName(null)} submitLabel="确认" submitDisabled={busy} onSubmit={(e) => { e.preventDefault(); void write(() => seekerApi.confirm(versionName)).then((ok) => { if (ok) setVersionName(null); }); }}><TextInput autoFocus aria-label="版本名称" placeholder="版本名称" value={versionName} onChange={(e) => setVersionName(e.target.value)} /></Modal>}
    {sourceChanges && <Modal wide title="来源差异" error={null} onClose={() => setSourceChanges(null)} submitLabel="完成" onSubmit={(e) => { e.preventDefault(); setSourceChanges(null); }}><div className="max-h-[58vh] space-y-4 overflow-auto">{!sourceChanges.length && <p>当前来源没有新版</p>}{sourceChanges.map((change) => <div key={change.entryId} className="space-y-2"><strong>{change.title}</strong><SeekerDiff before={[change.before.time, change.before.name, change.before.role, change.before.location, ...change.before.points.map((p) => p.text)].join("\n")} after={[change.after.time, change.after.name, change.after.role, change.after.location, ...change.after.points.map((p) => p.text)].join("\n")} /><div className="flex gap-2"><Button disabled={busy} onClick={() => void write(() => seekerApi.sourceApply(change)).then((ok) => { if (ok) setSourceChanges(sourceChanges.filter((c) => c.entryId !== change.entryId)); })}>更新这段经历</Button><Button onClick={() => setSourceChanges(sourceChanges.filter((c) => c.entryId !== change.entryId))}>保留原稿</Button></div></div>)}</div></Modal>}
    {suggestion && <Modal wide title={suggestion.kind === "review" ? "整份简历建议" : "查看建议"} error={null} onClose={() => setSuggestion(null)} submitDisabled={busy} submitLabel={suggestion.kind === "review" ? "完成" : suggestion.kind === "select" ? "采用选材" : "接受修改"} onSubmit={(e) => { e.preventDefault(); if (suggestion.kind === "review") { setSuggestion(null); return; } if (suggestion.kind === "select") { setSelections(suggestion.selections); setSuggestion(null); setTab("resume"); setDrawer("materials"); return; } const action = suggestion.beforeDraft && suggestion.afterDraft ? () => seekerApi.apply(suggestion.beforeDraft!, suggestion.afterDraft!) : suggestion.afterRecord ? () => seekerApi.recordSave(suggestion.afterRecord!) : null; if (action) void write(action).then((ok) => { if (ok) setSuggestion(null); }); }}><div className="max-h-[60vh] space-y-3 overflow-auto"><p className="text-xs text-muted-foreground">{suggestion.model} {suggestion.note}</p><p className="whitespace-pre-wrap text-sm">{suggestion.advice}</p>{suggestion.kind === "select" && suggestion.selections.map((s) => <div key={s.recordId} className="rounded-control border p-2 text-sm"><strong>{catalog.records.find((r) => r.id === s.recordId)?.title}</strong>{s.pointIds.map((id) => <p key={id}>• <InlineMd text={catalog.records.find((r) => r.id === s.recordId)?.points.find((p) => p.id === id)?.text ?? ""} /></p>)}</div>)}{suggestion.afterDraft && <SeekerDiff before={draftText(suggestion.beforeDraft)} after={draftText(suggestion.afterDraft)} />}{suggestion.afterRecord && <SeekerDiff before={recordText(suggestion.beforeRecord)} after={recordText(suggestion.afterRecord)} />}</div></Modal>}
    {confirmDialog}
  </div>;
}
