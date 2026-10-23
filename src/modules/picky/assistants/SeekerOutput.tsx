import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "@/shared/Modal";
import { Button, TextInput } from "@/shared/ui";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { jobLabel, useJobs } from "@/modules/jobs/api";
import { Dropdown } from "@/shared/Dropdown";
import { seekerApi, type SeekerDraft, type SeekerOutput, type SeekerOutputPreview, type SeekerSlots, type SeekerTemplate, type SeekerWorkspace } from "./seekerApi";
import { SeekerDiff, draftText, when } from "./SeekerParts";
import type { SeekerWrite } from "./SeekerLibrary";

const SLOTS: { key: keyof SeekerSlots; label: string; optional?: boolean }[] = [
  { key: "name", label: "姓名" },
  { key: "contact", label: "联系方式" },
  { key: "section", label: "章节标题" },
  { key: "entry", label: "经历抬头" },
  { key: "education", label: "教育抬头", optional: true },
  { key: "note", label: "说明", optional: true },
  { key: "point", label: "要点" },
];
/** A first guess from the template's text: the user checks it. */
function guess(paragraphs: (string | null)[]): SeekerSlots {
  const at = (test: (t: string, i: number) => boolean, from = 0) => paragraphs.findIndex((t, i) => i >= from && t != null && test(t, i));
  const tabs = (t: string) => t.split("\t").length - 1;
  const name = at(() => true);
  const contact = at(() => true, name + 1);
  const section = at((t) => !tabs(t) && t.trim().length <= 6, contact + 1);
  const entry = at((t) => tabs(t) >= 2);
  const education = at((t) => tabs(t) === 1);
  const point = at((t) => !tabs(t) && /^[•·\s]*[^\s\[]{2,6}：/.test(t), section + 1);
  const note = at((t, i) => !tabs(t) && i !== point && i !== section && [entry, education].includes(i - 1));
  return { name, contact, section, entry, point, education: education >= 0 ? education : null, note: note >= 0 ? note : null };
}

export function SeekerTemplateSettings(props: { output: SeekerOutput; folder: string; busy: boolean; write: SeekerWrite }) {
  const toast = useToast();
  const current = props.output.template;
  const [editing, setEditing] = useState<SeekerTemplate | null>(null);
  const [reading, setReading] = useState(false);
  const pick = async (existing?: string) => {
    setReading(true);
    try {
      const path = existing ?? await open({ multiple: false, title: "选择中文简历模板", defaultPath: props.folder ? `${props.folder}/cv/cn/` : undefined, filters: [{ name: "Word", extensions: ["docx"] }] });
      if (typeof path !== "string") return;
      const paragraphs = await seekerApi.templateRead(path);
      setEditing({ path, paragraphs, slots: existing && current ? { ...current.slots } : guess(paragraphs) });
    } catch (e) { toast(errorText(e), "error"); }
    finally { setReading(false); }
  };
  const ready = editing && SLOTS.every((s) => s.optional || (editing.slots[s.key] ?? -1) >= 0);
  return <section className="space-y-3"><h2 className="font-bold">中文简历模板</h2><p className="break-all text-sm text-muted-foreground">{current?.path ?? "未选择"}</p><div className="flex gap-2"><Button disabled={props.busy || reading} onClick={() => void pick()}>选择 Word 模板</Button>{current && <><Button disabled={props.busy || reading} onClick={() => void pick(current.path)}>重新对应段落</Button><Button disabled={props.busy || reading} onClick={() => void props.write(() => seekerApi.template(null))}>清除</Button></>}</div>
    {editing && <Modal title="模板段落" error={null} onClose={() => { if (!props.busy) setEditing(null); }} submitLabel="保存" submitDisabled={props.busy || !ready} onSubmit={(e) => { e.preventDefault(); void props.write(() => seekerApi.template(editing)).then((ok) => { if (ok) setEditing(null); }); }}>
      <div className="space-y-3">{SLOTS.map((slot) => <div key={slot.key} className="grid grid-cols-[5rem_1fr] items-center gap-3 text-sm"><span>{slot.label}</span><Dropdown<number> ariaLabel={`${slot.label}对应的段落`} value={editing.slots[slot.key] ?? -1} placeholder={slot.optional ? "不用" : "选择段落"} options={[...(slot.optional ? [{ value: -1, label: "不用" }] : []), ...editing.paragraphs.flatMap((text, index) => text ? [{ value: index, label: `${index + 1}. ${text.replace(/\t/g, " ⇥ ").slice(0, 60)}` }] : [])]} onChange={(value) => setEditing({ ...editing, slots: { ...editing.slots, [slot.key]: slot.optional && value < 0 ? null : value } })} /></div>)}</div>
    </Modal>}
  </section>;
}

export function SeekerExportDialog(props: { name: string; flush: () => Promise<boolean>; saved: (workspace: SeekerWorkspace) => void; close: () => void }) {
  const toast = useToast();
  const jobs = useJobs();
  const [preview, setPreview] = useState<SeekerOutputPreview | null>(null);
  const [directory, setDirectory] = useState("");
  const [filename, setFilename] = useState(() => `${props.name || "简历"}-${new Date().toISOString().slice(0, 10).replace(/-/g, "")}.docx`);
  const [jobId, setJobId] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ticket = useRef<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; if (ticket.current) void seekerApi.outputDiscard(ticket.current).catch(() => {}); }; }, []);
  const prepare = async () => {
    setBusy(true); setError(null); setPreview(null);
    try {
      if (!await props.flush()) { setError("请先保存当前草稿"); return; }
      if (ticket.current) { await seekerApi.outputDiscard(ticket.current); ticket.current = null; }
      const next = await seekerApi.outputPreview();
      if (!mounted.current) { await seekerApi.outputDiscard(next.ticket); return; }
      ticket.current = next.ticket; setPreview(next); setDirectory((d) => d || next.defaultFolder);
    } catch (e) { if (mounted.current) setError(errorText(e)); }
    finally { if (mounted.current) setBusy(false); }
  };
  const save = async () => {
    if (!preview || preview.pages !== 1 || busy) return;
    setBusy(true); setError(null);
    try {
      if (!await props.flush()) { setError("请先保存当前草稿"); return; }
      const result = await seekerApi.outputSave(preview.ticket, directory, filename, jobId);
      ticket.current = null; props.saved(result.workspace); toast(`已导出：${result.export.path}`); props.close();
    } catch (e) { setError(errorText(e)); }
    finally { if (mounted.current) setBusy(false); }
  };
  return <Modal title="导出一页 Word" error={error} onClose={() => { if (!busy) props.close(); }} submitLabel={busy ? "正在处理…" : "另存 Word"} submitDisabled={busy || preview?.pages !== 1 || !directory.trim() || !filename.trim()} onSubmit={(e) => { e.preventDefault(); void save(); }}>
    <div className="space-y-3"><Button disabled={busy} onClick={() => void prepare()}>{preview ? "重新分页" : "生成分页预览"}</Button>
      {preview && <div role="status" className="space-y-2 rounded-control border p-3"><p className="text-sm">{preview.renderer} 实际渲染：{preview.pages} 页</p>{preview.pages > 1 && <p className="text-sm text-destructive">超过一页，请精简后重新分页</p>}<Button disabled={busy} onClick={() => void ipc.openLink(preview.pdfPath).catch((e) => setError(errorText(e)))}>打开分页预览</Button></div>}
      <label className="block space-y-1 text-sm"><span>导出文件夹</span><div className="flex gap-2"><TextInput value={directory} disabled={busy} onChange={(e) => setDirectory(e.target.value)} /><Button disabled={busy} onClick={() => void open({ directory: true, multiple: false, defaultPath: directory || undefined }).then((p) => { if (typeof p === "string") setDirectory(p); }).catch((e) => setError(errorText(e)))}>选择</Button></div></label>
      <label className="block space-y-1 text-sm"><span>文件名</span><TextInput value={filename} disabled={busy} onChange={(e) => setFilename(e.target.value)} /></label>
      <label className="block space-y-1 text-sm"><span>关联投石问路</span><Dropdown<number | null> ariaLabel="关联投石问路" value={jobId} options={[{ value: null, label: "暂不关联" }, ...(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))]} onChange={setJobId} /></label>{jobs.isError && <p className="text-xs text-destructive">{errorText(jobs.error)}</p>}
    </div>
  </Modal>;
}

export function SeekerExports(props: { output: SeekerOutput; draft: SeekerDraft | null; busy: boolean; write: SeekerWrite }) {
  const toast = useToast();
  const [confirm, dialog] = useConfirm();
  const jobs = useJobs();
  const [id, setId] = useState<string | null>(null);
  const [jobId, setJobId] = useState<number | null>(null);
  const exports = props.output.exports;
  const item = exports.find((e) => e.id === id) ?? exports[0];
  if (!exports.length) return <section className="py-3 text-sm text-muted-foreground">还没有 Word 导出</section>;
  return <section className="space-y-3 border-b pb-4"><h2 className="font-bold">Word 导出</h2><Dropdown ariaLabel="导出版本" value={item.id} options={exports.map((e) => ({ value: e.id, label: `${e.label} · ${when(e.createdAt)}` }))} onChange={setId} />
    <p className="break-all text-xs text-muted-foreground">{item.path} · {item.renderer} {item.pages} 页</p><div className="flex flex-wrap gap-2"><Button onClick={() => void ipc.openLink(item.path).catch((e) => toast(errorText(e), "error"))}>打开 Word</Button><Button disabled={props.busy} onClick={async () => { if (await confirm({ title: "从此导出另开草稿？", message: "当前草稿会被替换，导出文件和历史快照保持原样。" })) void props.write(() => seekerApi.exportRestore(item.id)); }}>另开草稿</Button></div>
    <div className="flex flex-wrap gap-2"><Dropdown<number | null> ariaLabel="关联岗位" value={jobId} placeholder="选择岗位" options={(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))} onChange={setJobId} /><Button disabled={props.busy || jobId === null} onClick={() => jobId !== null && void props.write(() => seekerApi.jobLink(item.id, jobId))}>关联并用于本次投递</Button></div>
    {jobs.isError && <p className="text-xs text-destructive">{errorText(jobs.error)}</p>}
    {props.output.links.filter((l) => l.exportId === item.id).map((link) => <details key={link.id} className="rounded-control border p-2 text-sm"><summary>{jobLabel(link.job)}{link.current ? " · 本次使用" : " · 历史关联"} · {when(link.createdAt)}</summary><p className="whitespace-pre-wrap pt-2">{[link.job.location, link.job.link, link.job.note].filter(Boolean).join("\n")}</p><p className="whitespace-pre-wrap pt-2">简历目标 JD：{link.jd || "未填写"}</p></details>)}
    <details><summary className="text-sm">与当前草稿比较</summary><SeekerDiff before={draftText(item.draft)} after={draftText(props.draft)} /></details><details><summary className="text-sm">导出正文快照</summary><pre className="whitespace-pre-wrap font-sans text-sm">{draftText(item.draft)}</pre></details>{dialog}
  </section>;
}
