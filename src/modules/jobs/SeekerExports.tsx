import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTabs } from "@/app/tabs";
import { AssistAction } from "@/modules/picky/Assist";
import { SEEKER_KEY, deskOf, draftLang, pageLang, seekerApi, seekerJobCompanyKey } from "@/modules/picky/assistants/seekerApi";
import { flushSeekerDraft } from "@/modules/picky/assistants/seekerDraft";
import { useSeekerKnowledge } from "@/modules/picky/assistants/SeekerJob";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { Dropdown } from "@/shared/Dropdown";
import { Button, Field } from "@/shared/ui";
import type { Job } from "./api";

/** The job's Seeker part: 做简历 (the draft in the language Seeker shows is for this job, its 备注 as the
 * JD), its exports, and which company of knowledge/ it is (by name, or chosen here). */
export function JobSeekerExports(props: { job: Job; disabled: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const tabs = useTabs();
  const [confirm, dialog] = useConfirm();
  const view = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const knowledge = useSeekerKnowledge();
  const companies = knowledge.data?.companies.length ?? 0;
  const company = useQuery({ queryKey: seekerJobCompanyKey(props.job.id), queryFn: () => seekerApi.jobCompany(props.job.id), enabled: companies > 0 });
  const [id, setId] = useState("");
  const [busy, setBusy] = useState(false);
  const output = view.data?.output;
  const links = output?.links.filter((l) => l.job.id === props.job.id && l.job.createdAt === props.job.createdAt) ?? [];
  const free = (output?.exports ?? []).filter((e) => !links.some((l) => l.exportId === e.id));
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try { await action(); }
    catch (e) { toast(errorText(e), "error"); }
    finally { setBusy(false); }
  };
  const link = (exportId: string) => run(async () => { qc.setQueryData(SEEKER_KEY, await seekerApi.jobLink(exportId, props.job.id)); setId(""); });
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
  return <>
    {companies > 0 && c && <Field label="公司资料">
      <Dropdown<string> ariaLabel="公司资料" value={c.chosen ?? c.auto ?? ""} options={[...c.companies.map((x) => ({ value: x.folder, label: x.folder === c.auto ? `${x.name}（按名字对上）` : x.name })), { value: "", label: "不用" }]} onChange={(v) => void choose(v)} />
    </Field>}
    <Field label="Seeker" action={<AssistAction disabled={props.disabled || busy} onClick={() => void make()}>做简历</AssistAction>}><div className="space-y-2">
      {view.isError && <p className="text-sm text-destructive">{errorText(view.error)}</p>}
      {links.map((l) => { const item = output?.exports.find((e) => e.id === l.exportId); return <div key={l.id} className="space-y-2 rounded-control border p-2 text-sm"><p className="break-all">{item?.label ?? "导出版本缺失"}{item && draftLang(item.draft) === "en" && <span className="ml-2 text-xs text-muted-foreground">English</span>}{l.current && <strong className="ml-2 text-primary">本次使用</strong>}</p><div className="flex flex-wrap gap-2"><Button disabled={!item} onClick={() => item && void ipc.openLink(item.path).catch((e) => toast(errorText(e), "error"))}>打开 Word</Button>{!l.current && <Button disabled={props.disabled || busy || !item} onClick={() => void link(l.exportId)}>用于本次投递</Button>}</div><details><summary>关联时的岗位与 JD</summary><p className="whitespace-pre-wrap pt-2">{[l.job.company, l.job.position, l.job.location, l.job.link, l.job.note, l.jd].filter(Boolean).join("\n")}</p></details></div>; })}
      {!view.isLoading && !view.isError && !links.length && !free.length && <p className="text-sm text-muted-foreground">还没有导出过</p>}
      {free.length > 0 && <div className="flex flex-wrap gap-2"><Dropdown ariaLabel="选择已导出的简历" value={id} placeholder="选择导出的简历" options={free.map((e) => ({ value: e.id, label: e.label }))} onChange={setId} /><Button disabled={props.disabled || busy || !id} onClick={() => void link(id)}>关联</Button></div>}
    </div></Field>
    {dialog}
  </>;
}
