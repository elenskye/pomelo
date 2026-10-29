import { PickField } from "@/shared/PickField";
import { useState } from "react";
import { createPortal } from "react-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { FileText, FolderOpen } from "lucide-react";
import { useTabs } from "@/app/tabs";
import { AssistAction } from "@/modules/picky/Assist";
import { SEEKER_JOB_RESUMES_KEY, SEEKER_KEY, deskOf, pageLang, seekerApi, seekerJobCompanyKey, seekerJobResumesKey, type SeekerCvFile, type SeekerLang, type SeekerLinked } from "@/modules/picky/assistants/seekerApi";
import { flushSeekerDraft } from "@/modules/picky/assistants/seekerDraft";
import { useSeekerKnowledge } from "@/modules/picky/assistants/SeekerJob";
import { cvDate, folderOf } from "@/modules/picky/assistants/SeekerFiles";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Modal } from "@/shared/Modal";
import { Button, Field, IconButton, SearchInput, Segmented } from "@/shared/ui";
import { jobLabel, type Job } from "./api";

const fileName = (name: string) => name.split("/").pop() ?? name;

/** The job's Seeker part: 做简历 (the draft in the language Seeker shows is for this job, its 备注 as the
 * JD), the résumé under cv/ it uses this time (only that one is shown; the ones used before stay as
 * records, tagged 用过 in the picker; 选择简历… picks any of cv/'s, templates too), and which company of
 * knowledge/ it is (by name, or chosen here). */
export function JobSeekerExports(props: { job: Job; disabled: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const tabs = useTabs();
  const [confirm, dialog] = useConfirm();
  const view = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const resumes = useQuery({ queryKey: seekerJobResumesKey(props.job.id), queryFn: () => seekerApi.jobResumes(props.job.id) });
  const knowledge = useSeekerKnowledge();
  const companies = knowledge.data?.companies.length ?? 0;
  const company = useQuery({ queryKey: seekerJobCompanyKey(props.job.id), queryFn: () => seekerApi.jobCompany(props.job.id), enabled: companies > 0 });
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try { await action(); return true; }
    catch (e) { toast(errorText(e), "error"); return false; }
    finally { setBusy(false); }
  };
  const use = (path: string) => run(async () => {
    qc.setQueryData(SEEKER_KEY, await seekerApi.jobResume(props.job.id, path));
    await qc.invalidateQueries({ queryKey: SEEKER_JOB_RESUMES_KEY });
  });
  const open = (path: string) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
  const lang = pageLang(view.data);
  const make = async () => {
    const jd = (view.data ? deskOf(view.data, lang).draft?.jd.trim() : "") ?? "";
    const note = props.job.note?.trim() ?? "";
    if (jd && note && jd !== note && !(await confirm({ title: "为这个岗位做简历？", message: "Seeker 草稿的目标 JD 会换成这个岗位的备注，正文不动。" }))) return;
    await run(async () => {
      if (!(await flushSeekerDraft(qc))) throw new Error("Seeker 草稿还没保存好，请先打开 Seeker 重试保存");
      qc.setQueryData(SEEKER_KEY, await seekerApi.useJob(props.job.id, lang));
      tabs.open("/seeker/resume");
    });
  };
  const choose = (folder: string) => run(async () => {
    const auto = company.data?.auto ?? "";
    qc.setQueryData(seekerJobCompanyKey(props.job.id), await seekerApi.jobCompanySet(props.job.id, folder === auto ? null : folder));
  });
  const c = company.data;
  const r = resumes.data;
  const now = r?.linked.find((l) => l.current);
  return <>
    {companies > 0 && c && <Field label="公司资料">
      <PickField<string> block ariaLabel="公司资料" title="这个岗位用哪家公司的资料" value={c.chosen ?? c.auto ?? ""} options={[{ value: "", label: "不用" }, ...c.companies.map((x) => ({ value: x.folder, label: x.name, detail: x.folder === c.auto ? "按名字对上" : undefined }))]} onChange={(v) => void choose(v)} />
    </Field>}
    <Field label="Seeker" action={<AssistAction disabled={props.disabled || busy} onClick={() => void make()}>做简历</AssistAction>}><div className="space-y-2">
      {(resumes.isError || r?.error) && <p className="text-sm text-destructive">{resumes.isError ? errorText(resumes.error) : r?.error}</p>}
      {now && <div className="flex items-center gap-1 rounded-control border py-0.5 pr-0.5 pl-1 text-sm">
        <button type="button" title={`打开 ${now.name}`} onClick={() => open(now.path)} className="flex min-w-0 flex-1 items-center gap-2 rounded-control px-1.5 py-1 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring">
          <FileText size={14} className="shrink-0 text-muted-foreground" />
          <span className="min-w-0 flex-1 truncate">{fileName(now.name)}</span>
          {now.lang === "en" && <span className="shrink-0 text-xs text-muted-foreground">English</span>}
        </button>
        <IconButton label="在访达中显示" onClick={() => open(folderOf(now.path))}><FolderOpen size={14} /></IconButton>
      </div>}
      {r?.noFolder ? <p className="text-sm text-muted-foreground">还没选 Seeker 文件夹</p> : <Button className="w-full justify-center" disabled={props.disabled || busy || !r} onClick={() => setPicking(true)}>选择简历…</Button>}
    </div></Field>
    {/* outside the panel's <form> (no nested forms) */}
    {picking && r && createPortal(<ResumePicker job={props.job} files={r.files} linked={r.linked} lang={now?.lang ?? lang} busy={busy} onPick={(path) => void use(path).then((ok) => ok && setPicking(false))} onClose={() => setPicking(false)} />, document.body)}
    {dialog}
  </>;
}

/** 选择简历: cv/'s résumés of one language by series (this job's company first, newest version first). */
function ResumePicker(props: { job: Job; files: SeekerCvFile[]; linked: SeekerLinked[]; lang: SeekerLang; busy: boolean; onPick: (path: string) => void; onClose: () => void }) {
  const [lang, setLang] = useState<SeekerLang>(props.lang);
  const [filter, setFilter] = useState("");
  const current = props.linked.find((l) => l.current)?.path ?? null;
  const [picked, setPicked] = useState<string | null>(current);
  const label = jobLabel(props.job);
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  const list = props.files.filter((f) => f.lang === lang && words.every((w) => f.name.toLowerCase().includes(w)));
  const series = [...new Set(list.map((f) => f.series))].sort((a, b) => Number(list.some((f) => f.series === b && f.jobs.includes(label))) - Number(list.some((f) => f.series === a && f.jobs.includes(label))));
  const tag = (f: SeekerCvFile) => [props.linked.some((l) => l.path === f.path) ? (f.path === current ? "本次使用" : "用过") : "", f.export ? "Pomelo 导出" : ""].filter(Boolean).join(" · ");
  return <Modal size="xl" title={`选择简历 · ${label}`} error={null} onClose={() => { if (!props.busy) props.onClose(); }} submitLabel={props.busy ? "正在关联…" : "用于本次投递"} submitDisabled={props.busy || !picked || picked === current} onSubmit={(e) => { e.preventDefault(); e.stopPropagation(); if (picked) props.onPick(picked); }}>
    <div className="flex items-center gap-3">
      <Segmented<SeekerLang> ariaLabel="简历语言" value={lang} onChange={setLang} options={[{ value: "cn", label: "中文" }, { value: "en", label: "English" }]} />
      <div className="min-w-0 flex-1"><SearchInput aria-label="按文件名找" placeholder="按文件名找" value={filter} onChange={(e) => setFilter(e.target.value)} /></div>
    </div>
    <div role="listbox" aria-label="cv/ 里的简历" className="-m-1 max-h-[52vh] space-y-3 overflow-auto p-1">
      {!list.length && <p className="py-6 text-center text-sm text-muted-foreground">{props.files.some((f) => f.lang === lang) ? "没有名字里有这些字的简历" : "cv/ 里没有这种语言的简历"}</p>}
      {series.map((s) => {
        const versions = list.filter((f) => f.series === s);
        const jobs = [...new Set(versions.flatMap((f) => f.jobs))];
        return <section key={s} className="space-y-1">
          <p className="truncate px-1 text-xs text-muted-foreground" title={s}><span className="font-bold text-foreground">{s}</span>{versions.length > 1 && ` · ${versions.length} 版`}{jobs.length > 0 && ` · 投石问路：${jobs.join("、")}`}</p>
          {versions.map((f) => {
            const on = picked === f.path;
            return <button key={f.path} type="button" role="option" aria-selected={on} onClick={() => setPicked(f.path)} onDoubleClick={() => f.path !== current && props.onPick(f.path)}
              className={`flex w-full items-center gap-2 rounded-control border px-3 py-2 text-left text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring ${on ? "border-primary bg-accent text-accent-foreground" : "border-transparent hover:bg-muted"}`}>
              <FileText size={14} className="shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate" title={f.name}>{fileName(f.name)}</span>
              {tag(f) && <span className="shrink-0 text-xs text-primary">{tag(f)}</span>}
              <span className="shrink-0 text-xs text-muted-foreground">{cvDate(f.modified)}</span>
            </button>;
          })}
        </section>;
      })}
    </div>
  </Modal>;
}
