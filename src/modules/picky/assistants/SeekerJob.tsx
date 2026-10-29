// The draft's 投石问路 job (岗位与建议) and knowledge/ (模板与规则). Rules in Rust: seeker.rs
// `use_job`, seeker/company.rs.
import { PickField } from "@/shared/PickField";
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useJobs, jobPicks } from "@/modules/jobs/api";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Modal } from "@/shared/Modal";
import { Button, TextInput } from "@/shared/ui";
import { Path, Row, Section } from "@/settings/layout";
import { SEEKER_KNOWLEDGE_KEY, draftLang, jobCompanyFolder, seekerApi, seekerJobCompanyKey, type SeekerCompany, type SeekerDraft, type SeekerLang, type SeekerWorkspace } from "./seekerApi";

export const useSeekerKnowledge = () => useQuery({ queryKey: SEEKER_KNOWLEDGE_KEY, queryFn: seekerApi.knowledge });

/** Which job the draft is for: its 备注 becomes the JD (asked first when that replaces another). */
export function SeekerJobPicker(props: { lang: SeekerLang; draft: SeekerDraft | null; busy: boolean; write: (action: () => Promise<SeekerWorkspace>) => Promise<boolean> }) {
  const jobs = useJobs();
  const [confirm, dialog] = useConfirm();
  const ref = props.draft?.job;
  const job = jobs.data?.find((j) => ref && j.id === ref.id && j.createdAt === ref.createdAt) ?? null;
  const knowledge = useSeekerKnowledge();
  const company = useQuery({ queryKey: seekerJobCompanyKey(job?.id ?? 0), queryFn: () => seekerApi.jobCompany(job!.id), enabled: !!job && !!knowledge.data?.companies.length });
  const folder = jobCompanyFolder(company.data);
  const pick = async (id: number | null) => {
    const next = jobs.data?.find((j) => j.id === id);
    const jd = props.draft?.jd.trim() ?? "";
    const note = next?.note?.trim() ?? "";
    if (next && note && jd && note !== jd && !(await confirm({ title: "换成这个岗位？", message: "目标 JD 会换成这个岗位的备注。" }))) return;
    await props.write(() => seekerApi.useJob(id, props.draft ? draftLang(props.draft) : props.lang));
  };
  return (
    <>
    <fieldset disabled={props.busy} className="flex flex-col gap-1.5">
      <span className="text-xs text-muted-foreground">岗位</span>
      <PickField<number | null> block ariaLabel="简历的岗位" title="这份简历的岗位" value={job?.id ?? null} options={[{ value: null, label: "不关联岗位" }, ...jobPicks(jobs.data ?? [])]} onChange={(id) => void pick(id)} />
      {job && !!knowledge.data?.companies.length && company.data && (
        <p className="text-xs text-muted-foreground">公司资料：{folder ? (company.data.companies.find((c) => c.folder === folder)?.name ?? folder) : "没对上"}</p>
      )}
    </fieldset>
    {dialog}
    </>
  );
}

/** knowledge/ in 设置: in a 知识库 or not, and its companies (名称… names them: folder ↔ name). */
export function SeekerKnowledgeSettings(props: { folder: string; busy: boolean }) {
  const qc = useQueryClient();
  const toast = useToast();
  const k = useSeekerKnowledge();
  const [naming, setNaming] = useState(false);
  const add = async (part: "knowledge" | "cv" = "knowledge") => {
    try {
      qc.setQueryData(SEEKER_KNOWLEDGE_KEY, await seekerApi.knowledgeAdd(part));
      toast("已加进知识库");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  if (!props.folder) return null;
  const d = k.data;
  const problem = k.isError ? errorText(k.error) : d?.error;
  return (
    <Section title="知识库">
      <Row label="位置">{problem ? <span className="text-destructive">{problem}</span> : d?.path ? <Path value={d.path} /> : <span className="text-muted-foreground">Seeker 文件夹里没有 knowledge/</span>}</Row>
      {d?.path && (
        <>
          <Row label="知识库" actions={!d.library && <Button disabled={props.busy} onClick={() => void add()}>加进知识库</Button>}>{d.library ? `在「${d.library}」里` : <span className="text-muted-foreground">还没加</span>}</Row>
          <Row label={`公司 ${d.companies.length}`} actions={d.companies.length > 0 && <Button disabled={props.busy} onClick={() => setNaming(true)}>名称…</Button>}>{d.companies.map((c) => c.name).join("、") || <span className="text-muted-foreground">没有</span>}</Row>
        </>
      )}
      {d?.cv && <Row label="简历（cv/）" actions={!d.cvLibrary && <Button disabled={props.busy} onClick={() => void add("cv")}>加进知识库</Button>}>{d.cvLibrary ? `在「${d.cvLibrary}」里` : <span className="text-muted-foreground">还没加</span>}</Row>}
      {naming && d && <CompanyNames companies={d.companies} onClose={() => setNaming(false)} />}
    </Section>
  );
}

/** A name for each company folder of knowledge/ (blank: the one found in 招聘索引.md or the folder's). */
function CompanyNames(props: { companies: SeekerCompany[]; onClose: () => void }) {
  const qc = useQueryClient();
  const [names, setNames] = useState<Record<string, string>>(() => Object.fromEntries(props.companies.map((c) => [c.folder, c.name === c.auto ? "" : c.name])));
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const save = async () => {
    setSaving(true);
    try {
      qc.setQueryData(SEEKER_KNOWLEDGE_KEY, await seekerApi.companyNames(names));
      await qc.invalidateQueries({ queryKey: ["picky", "seeker", "company"] });
      props.onClose();
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Modal wide title="公司名称" error={error} onClose={() => { if (!saving) props.onClose(); }} submitLabel={saving ? "正在保存…" : "保存"} submitDisabled={saving} onSubmit={(e) => { e.preventDefault(); void save(); }}>
      <div className="-m-1 max-h-[55vh] space-y-2 overflow-auto p-1">
        {props.companies.map((c, i) => (
          <label key={c.folder} className="flex items-center gap-3 text-sm">
            <span className="w-40 shrink-0 truncate font-mono text-xs text-muted-foreground" title={`knowledge/${c.folder}`}>{c.folder}</span>
            <TextInput autoFocus={i === 0} aria-label={`${c.folder} 的名称`} placeholder={c.auto} value={names[c.folder] ?? ""} onChange={(e) => setNames({ ...names, [c.folder]: e.target.value })} />
          </label>
        ))}
      </div>
    </Modal>
  );
}
