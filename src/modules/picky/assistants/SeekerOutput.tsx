import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Modal } from "@/shared/Modal";
import { Button, TextInput } from "@/shared/ui";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { jobLabel, useJobs } from "@/modules/jobs/api";
import { Dropdown } from "@/shared/Dropdown";
import { Path, Row, Section } from "@/settings/layout";
import { draftLang, seekerApi, templateOf, type SeekerDraft, type SeekerForm, type SeekerJobRef, type SeekerLang, type SeekerOutput, type SeekerSlots, type SeekerTemplate, type SeekerWorkspace } from "./seekerApi";
import { SeekerDiff, draftText, when } from "./SeekerParts";
import type { SeekerWrite } from "./SeekerLibrary";

type Slot = { key: keyof SeekerSlots; label: string; optional?: boolean; blank?: boolean };
const SLOTS: Slot[] = [
  { key: "name", label: "姓名" },
  { key: "contact", label: "联系方式" },
  { key: "contact2", label: "第二行联系方式", optional: true },
  { key: "section", label: "章节标题" },
  { key: "entry", label: "经历抬头" },
  { key: "education", label: "教育抬头", optional: true },
  { key: "note", label: "说明", optional: true },
  { key: "point", label: "要点" },
];
/** English (Rust output.rs `lines`): 经历抬头 角色 ⇥ 时间, its second line 名称 (and 学位, 概览); a blank paragraph between parts. */
const SLOTS_EN: Slot[] = [
  { key: "name", label: "姓名" },
  { key: "contact", label: "联系方式" },
  { key: "section", label: "章节标题" },
  { key: "summary", label: "Summary", optional: true },
  { key: "entry", label: "经历抬头" },
  { key: "education", label: "教育抬头", optional: true },
  { key: "note", label: "第二行" },
  { key: "point", label: "要点" },
  { key: "blank", label: "空行", optional: true, blank: true },
];
const slotsOf = (lang: SeekerLang) => (lang === "en" ? SLOTS_EN : SLOTS);
/** A first guess for an English template like the user's finished resume: name, contact line, a
 * short section title, the summary under it, a blank paragraph, 「Role ⇥ dates」 headings (the one
 * under 「education」 is 教育抬头), the line under a heading, the line after that. */
function guessEn(paragraphs: (string | null)[]): SeekerSlots {
  const at = (test: (t: string, i: number) => boolean, from = 0) => paragraphs.findIndex((t, i) => i >= from && t != null && test(t, i));
  const tabs = (t: string) => t.split("\t").length - 1;
  const name = at(() => true);
  const contact = at(() => true, name + 1);
  // a short line; when the template spaces its parts with blank paragraphs, only one right after a blank
  const spaced = paragraphs.some((t, i) => t == null && i > contact && paragraphs.slice(i + 1).some((x) => x != null));
  const title = (t: string, i: number) => !tabs(t) && t.trim().length <= 30 && t.trim().split(/\s+/).length <= 3 && (!spaced || paragraphs[i - 1] == null);
  const section = at(title, contact + 1);
  const summary = section >= 0 && paragraphs[section + 1] && !tabs(paragraphs[section + 1]!) && !title(paragraphs[section + 1]!, section + 1) ? section + 1 : -1;
  const blank = paragraphs.findIndex((t, i) => i > Math.max(contact, section, summary) && t == null && paragraphs.slice(i + 1).some((x) => x != null));
  const educationTitle = at((t, i) => title(t, i) && /educat/i.test(t));
  const education = educationTitle >= 0 ? at((t) => tabs(t) === 1, educationTitle + 1) : -1;
  // the section a paragraph is in: the last title before it
  const sectionOf = (i: number) => paragraphs.slice(0, i).reduce((last, t, j) => (j > contact && t != null && title(t, j) ? j : last), -1);
  const entry = at((t, i) => tabs(t) === 1 && (educationTitle < 0 || sectionOf(i) !== educationTitle));
  const note = entry >= 0 && paragraphs[entry + 1] ? entry + 1 : -1;
  const point = entry >= 0 && paragraphs[entry + 2] ? entry + 2 : at((t, i) => i > section && i !== summary && !tabs(t) && !title(t, i), section + 1);
  return { name, contact, contact2: null, section, entry, point, education: education >= 0 ? education : null, note, summary: summary >= 0 ? summary : null, blank: blank >= 0 ? blank : null };
}
/** A first guess from the template's text: the user checks it. */
function guess(paragraphs: (string | null)[]): SeekerSlots {
  const at = (test: (t: string, i: number) => boolean, from = 0) => paragraphs.findIndex((t, i) => i >= from && t != null && test(t, i));
  const tabs = (t: string) => t.split("\t").length - 1;
  const name = at(() => true);
  const contact = at(() => true, name + 1);
  const section = at((t) => !tabs(t) && t.trim().length <= 6, contact + 1);
  const contact2 = at((t, i) => !tabs(t) && i < section, contact + 1);
  const entry = at((t) => tabs(t) >= 2);
  const education = at((t) => tabs(t) === 1);
  const bullet = (t: string) => !tabs(t) && /^[•·\s]*[^\s\[]{2,6}：/.test(t);
  const afterEntry = entry >= 0 ? at(bullet, entry + 1) : -1;
  const point = afterEntry >= 0 ? afterEntry : at(bullet, section + 1);
  const note = at((t, i) => !tabs(t) && i !== point && i !== section && [entry, education].includes(i - 1));
  return { name, contact, contact2: contact2 >= 0 && section >= 0 ? contact2 : null, section, entry, point, education: education >= 0 ? education : null, note: note >= 0 ? note : null };
}

export function SeekerTemplateSettings(props: { lang: SeekerLang; output: SeekerOutput; folder: string; busy: boolean; write: SeekerWrite }) {
  const toast = useToast();
  const { lang } = props;
  const current = templateOf(props.output, lang);
  const slots = slotsOf(lang);
  const word = lang === "en" ? "英文" : "中文";
  const [editing, setEditing] = useState<SeekerTemplate | null>(null);
  const [reading, setReading] = useState(false);
  const pick = async (existing?: string) => {
    setReading(true);
    try {
      const path = existing ?? await open({ multiple: false, title: `选择${word}简历模板`, defaultPath: props.folder ? `${props.folder}/cv/${lang}/` : undefined, filters: [{ name: "Word", extensions: ["docx"] }] });
      if (typeof path !== "string") return;
      const paragraphs = await seekerApi.templateRead(path);
      setEditing({ path, paragraphs, slots: existing && current ? { ...current.slots } : lang === "en" ? guessEn(paragraphs) : guess(paragraphs) });
    } catch (e) { toast(errorText(e), "error"); }
    finally { setReading(false); }
  };
  const ready = editing && slots.every((s) => s.optional || (editing.slots[s.key] ?? -1) >= 0);
  // the blank line takes an empty paragraph; every other part one with text
  const choices = (slot: Slot) => editing ? editing.paragraphs.flatMap((text, index) => (slot.blank ? text == null && index > 0 : !!text) ? [{ value: index, label: `${index + 1}. ${slot.blank ? "（空段）" : text!.replace(/\t/g, " ⇥ ").slice(0, 60)}` }] : []) : [];
  return <Section title={`${word}简历模板`}><Row label="模板" actions={<><Button disabled={props.busy || reading} onClick={() => void pick()}>{current ? "换一份" : "选择"}</Button>{current && <><Button disabled={props.busy || reading} onClick={() => void pick(current.path)}>重新对应段落</Button><Button variant="ghost" disabled={props.busy || reading} onClick={() => void props.write(() => seekerApi.template(null, lang))}>清除</Button></>}</>}>{current ? <Path value={current.path} /> : <span className="text-muted-foreground">未选择</span>}</Row>
    {editing && <Modal title="模板段落" error={null} onClose={() => { if (!props.busy) setEditing(null); }} submitLabel="保存" submitDisabled={props.busy || !ready} onSubmit={(e) => { e.preventDefault(); void props.write(() => seekerApi.template(editing, lang)).then((ok) => { if (ok) setEditing(null); }); }}>
      <div className="space-y-3">{slots.map((slot) => <div key={slot.key} className="grid grid-cols-[7rem_1fr] items-center gap-3 text-sm"><span>{slot.label}</span><Dropdown<number> ariaLabel={`${slot.label}对应的段落`} value={editing.slots[slot.key] ?? -1} placeholder={slot.optional ? "不用" : "选择段落"} options={[...(slot.optional ? [{ value: -1, label: "不用" }] : []), ...choices(slot)]} onChange={(value) => setEditing({ ...editing, slots: { ...editing.slots, [slot.key]: slot.optional && value < 0 ? null : value } })} /></div>)}</div>
    </Modal>}
  </Section>;
}

/** 另存 Word: no page count (Word opens it right after, to see the pages). Default folder cv/cn/ or
 * cv/en/, default job the draft's. */
export function SeekerExportDialog(props: { lang: SeekerLang; form: SeekerForm; name: string; folder: string; job: SeekerJobRef | null | undefined; flush: () => Promise<boolean>; saved: (workspace: SeekerWorkspace) => void; close: () => void }) {
  const toast = useToast();
  const jobs = useJobs();
  const [directory, setDirectory] = useState(() => (props.folder ? `${props.folder}/cv/${props.lang}` : ""));
  const [filename, setFilename] = useState(() => `${props.name || "简历"}${props.lang === "en" ? (props.form === "cv" ? "-CV" : "-Resume") : ""}-${new Date().toISOString().slice(0, 10).replace(/-/g, "")}.docx`);
  const own = jobs.data?.find((j) => props.job && j.id === props.job.id && j.createdAt === props.job.createdAt)?.id ?? null;
  const [picked, setPicked] = useState<number | null | undefined>(undefined);
  const jobId = picked === undefined ? own : picked;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    if (busy) return;
    setBusy(true); setError(null);
    try {
      if (!await props.flush()) { setError("请先保存当前草稿"); return; }
      const result = await seekerApi.outputSave(directory, filename, jobId, props.lang);
      props.saved(result.workspace); toast(`已导出：${result.export.path}`); props.close();
      void ipc.openLink(result.export.path).catch((e) => toast(errorText(e), "error"));
    } catch (e) { setError(errorText(e)); setBusy(false); }
  };
  return <Modal title="导出 Word" error={error} onClose={() => { if (!busy) props.close(); }} submitLabel={busy ? "正在导出…" : "另存 Word"} submitDisabled={busy || !directory.trim() || !filename.trim()} onSubmit={(e) => { e.preventDefault(); void save(); }}>
    <div className="space-y-3">
      <label className="block space-y-1 text-sm"><span>导出文件夹</span><div className="flex gap-2"><TextInput value={directory} disabled={busy} onChange={(e) => setDirectory(e.target.value)} /><Button disabled={busy} onClick={() => void open({ directory: true, multiple: false, defaultPath: directory || undefined }).then((p) => { if (typeof p === "string") setDirectory(p); }).catch((e) => setError(errorText(e)))}>选择</Button></div></label>
      <label className="block space-y-1 text-sm"><span>文件名</span><TextInput value={filename} disabled={busy} onChange={(e) => setFilename(e.target.value)} /></label>
      <label className="block space-y-1 text-sm"><span>关联投石问路</span><Dropdown<number | null> ariaLabel="关联投石问路" value={jobId} options={[{ value: null, label: "暂不关联" }, ...(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))]} onChange={setPicked} /></label>{jobs.isError && <p className="text-xs text-destructive">{errorText(jobs.error)}</p>}
    </div>
  </Modal>;
}

export function SeekerExports(props: { lang: SeekerLang; output: SeekerOutput; draft: SeekerDraft | null; busy: boolean; write: SeekerWrite }) {
  const toast = useToast();
  const [confirm, dialog] = useConfirm();
  const jobs = useJobs();
  const [id, setId] = useState<string | null>(null);
  const [jobId, setJobId] = useState<number | null>(null);
  const exports = props.output.exports.filter((e) => draftLang(e.draft) === props.lang);
  const item = exports.find((e) => e.id === id) ?? exports[0];
  if (!exports.length) return <section className="py-3 text-sm text-muted-foreground">还没有 Word 导出</section>;
  return <section className="space-y-3 border-b pb-4"><h2 className="font-bold">Word 导出</h2><Dropdown ariaLabel="导出版本" value={item.id} options={exports.map((e) => ({ value: e.id, label: `${e.label} · ${when(e.createdAt)}` }))} onChange={setId} />
    <p className="break-all text-xs text-muted-foreground">{item.path}</p><div className="flex flex-wrap gap-2"><Button onClick={() => void ipc.openLink(item.path).catch((e) => toast(errorText(e), "error"))}>打开 Word</Button><Button disabled={props.busy} onClick={async () => { if (await confirm({ title: "从此导出另开草稿？", message: "当前草稿会被替换，导出文件和历史快照保持原样。" })) void props.write(() => seekerApi.exportRestore(item.id)); }}>另开草稿</Button></div>
    <div className="flex flex-wrap gap-2"><Dropdown<number | null> ariaLabel="关联岗位" value={jobId} placeholder="选择岗位" options={(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))} onChange={setJobId} /><Button disabled={props.busy || jobId === null} onClick={() => jobId !== null && void props.write(() => seekerApi.jobLink(item.id, jobId))}>关联并用于本次投递</Button></div>
    {jobs.isError && <p className="text-xs text-destructive">{errorText(jobs.error)}</p>}
    {props.output.links.filter((l) => l.exportId === item.id).map((link) => <details key={link.id} className="rounded-control border p-2 text-sm"><summary>{jobLabel(link.job)}{link.current ? " · 本次使用" : " · 历史关联"} · {when(link.createdAt)}</summary><p className="whitespace-pre-wrap pt-2">{[link.job.location, link.job.link, link.job.note].filter(Boolean).join("\n")}</p><p className="whitespace-pre-wrap pt-2">简历目标 JD：{link.jd || "未填写"}</p></details>)}
    <details><summary className="text-sm">与当前草稿比较</summary><SeekerDiff before={draftText(item.draft)} after={draftText(props.draft)} /></details><details><summary className="text-sm">导出正文快照</summary><pre className="whitespace-pre-wrap font-sans text-sm">{draftText(item.draft)}</pre></details>{dialog}
  </section>;
}
