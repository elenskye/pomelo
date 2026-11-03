// Rater 面试 (v0.7 阶段 ⑦, R8; Rust picky/assistants/rater/interview.rs, record.rs): one 投石问路 job at a
// time — its 准备 (小拾 picks the bank's questions, adds likely follow-ups, writes a one-page 速查卡 the user
// edits and saves as Word; 用这些题练一轮 starts a card round), its 复盘 (记录: a conversation with Rater's
// model fills the table — what was asked and answered, the feedback, the user's own judgement — then
// 继续记录 / 确认; an unfinished one waits under its job) and its 总结 (the top-right button: a 速查卡 and
// some reviews → hits, misses, questions to keep, rule points to adopt; it runs on when the page closes).
// The job panel's 「面试」 opens this page with the job (and the prep / 复盘 dialog) via useOpenRequest.
import { PickField } from "@/shared/PickField";
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronRight, FileDown, Globe, MessageSquare, Pencil, Play, Plus, Sparkles, Trash2, X } from "lucide-react";
import { jobLabel, useJobs, type Job, jobPicks } from "@/modules/jobs/api";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useOpenRequest } from "@/shared/open";
import { Button, EmptyState, Field, IconButton, Segmented, TextArea, TextInput } from "@/shared/ui";
import { MarkdownView } from "../MarkdownView";
import { WEB_SKILL, pickyApi, usePickyStatus, useTemplates, type ChatMessage } from "../api";
import { Composer, toolClass, type ComposerHandle } from "../Composer";
import { openFile } from "../Files";
import { SkillChips, useAskedSkills } from "../Plugins";
import { Transcript, type TranscriptHandle } from "../Transcript";
import { PrepNotices, activeIn, messagesKey, useLiveReply, usePrepare } from "../useChat";
import { PickRow } from "./select";
import { SEEKER_KEY, seekerApi, seekerJobResumesKey } from "./seekerApi";
import { SeekerJobLine, startSeekerJob, useSeekerJobs, seekerJobKey } from "./seekerJobs";
import { RATER_INTERVIEWS_KEY, RATER_INTERVIEW_LANGS, RATER_KEY, RATER_MODES, RATER_PRACTICE_KEY, RATER_RECORDS_KEY, RATER_RESULTS, interviewApi, raterApi, sameJob, type RaterAnalysis, type RaterAsked, type RaterBank, type RaterCan, type RaterFollow, type RaterInterviews, type RaterPrep, type RaterRecord, type RaterReview, type RaterSummary } from "./raterApi";

type Guarded = (action: () => Promise<void>) => Promise<boolean>;
type Props = { bank: RaterBank; can: RaterCan; busy: boolean; guarded: Guarded; toCards: () => void };
type Write = (action: () => Promise<RaterInterviews>, after?: () => void) => Promise<boolean>;
const today = () => { const d = new Date(); const p = (n: number) => String(n).padStart(2, "0"); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`; };
const day = (iso: string) => iso.slice(0, 10).replace(/-/g, " / ");
const open = (path: string, toast: ReturnType<typeof useToast>) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
const folderOf = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";
/** 第 1 轮 … 第 8 轮 (a review's round is text: older ones say 一面…). */
const ROUNDS = Array.from({ length: 8 }, (_, i) => `第 ${i + 1} 轮`);
/** The 总结 job's key (one per job at a time; seekerJobs keeps it while the page is closed). */
const summaryJob = (jobId: number) => ({ kind: "rater-summary", lang: "cn" as const, recordId: String(jobId) });
const reviewTitle = (r: { date: string; round: string; form: string }) => [day(r.date), r.round, r.form].filter(Boolean).join(" · ");

export function RaterInterview(p: Props) {
  const qc = useQueryClient();
  const jobs = useJobs();
  const all = useQuery({ queryKey: RATER_INTERVIEWS_KEY, queryFn: interviewApi.all });
  const records = useQuery({ queryKey: RATER_RECORDS_KEY, queryFn: interviewApi.records });
  const [jobId, setJobId] = useState<number | null>(null);
  const [prepping, setPrepping] = useState(false);
  const [editing, setEditing] = useState<RaterReview | null>(null);
  /** 复盘: the start (choose date, job, round), or a record's conversation. */
  const [recording, setRecording] = useState<{ session: number } | { start: true } | null>(null);
  const [summing, setSumming] = useState(false);
  const running = useSeekerJobs();
  // the job panel's 「面试」: this job, and maybe straight to 准备 / 复盘
  useOpenRequest((hit) => {
    if (hit.module !== "rater") return;
    setJobId(hit.id);
    if (hit.kind === "prep") setPrepping(true);
    if (hit.kind === "review") setRecording({ start: true });
  });
  const data = all.data;
  const list = jobs.data ?? [];
  const drafts = records.data ?? [];
  const of = (j: Job) => ({ preps: data?.preps.filter((x) => sameJob(x.job, j)) ?? [], reviews: data?.reviews.filter((x) => sameJob(x.job, j)) ?? [], summaries: data?.summaries.filter((x) => sameJob(x.job, j)) ?? [], drafts: drafts.filter((x) => sameJob(x.review.job, j)) });
  const count = (n: ReturnType<typeof of>) => n.preps.length + n.reviews.length + n.summaries.length + n.drafts.length;
  const latest = (n: ReturnType<typeof of>) => [...n.preps, ...n.reviews, ...n.summaries, ...n.drafts].map((x) => x.createdAt).sort().at(-1) ?? "";
  // jobs with records, newest first; the first one shown when none is picked
  const withRecords = list.map((j) => ({ j, n: of(j) })).filter((x) => count(x.n)).sort((a, b) => latest(b.n).localeCompare(latest(a.n)));
  useEffect(() => { if (jobId == null && withRecords.length) setJobId(withRecords[0].j.id); }, [withRecords.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const job = list.find((j) => j.id === jobId) ?? null;
  const write: Write = (action, after) => p.guarded(async () => {
    qc.setQueryData(RATER_INTERVIEWS_KEY, await action());
    // 存进题库 changes the bank, 加进规则 Seeker's records
    void qc.invalidateQueries({ queryKey: RATER_KEY, exact: true });
    void qc.invalidateQueries({ queryKey: SEEKER_KEY, exact: true });
    after?.();
  });
  if (all.isError) return <EmptyState title="面试记录暂时打不开" hint={errorText(all.error)} action={<Button onClick={() => void all.refetch()}>重试</Button>} />;
  if (!data) return <EmptyState title="正在读取…" />;
  const mine = job ? of(job) : { preps: [], reviews: [], summaries: [], drafts: [] };
  const sumKey = job ? seekerJobKey(summaryJob(job.id).kind, "cn", summaryJob(job.id).recordId) : "";
  const summary = running.find((x) => x.key === sumKey);
  const discard = (d: RaterRecord) => p.guarded(async () => { qc.setQueryData(RATER_RECORDS_KEY, await interviewApi.recordDiscard(d.session)); });
  return <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pt-4 pb-6">
    <div className="mx-auto max-w-3xl space-y-6">
      <div className="space-y-1">
        <div className="flex flex-wrap items-center gap-2">
          <PickField<number | null> ariaLabel="岗位" title="哪个岗位的面试" value={jobId} placeholder="选择岗位" options={list.map((j, i) => { const n = of(j); const base = jobPicks([j])[0]; return { ...base, detail: count(n) ? `准备 ${n.preps.length} · 复盘 ${n.reviews.length}` : base.detail, group: count(n) ? "有面试记录" : "其他岗位", value: list[i].id as number | null }; }).sort((a, b) => Number(a.group !== "有面试记录") - Number(b.group !== "有面试记录"))} onChange={setJobId} />
          <span className="flex-1" />
          <Button disabled={!job || p.busy || !p.can("prep")} onClick={() => setPrepping(true)}><Sparkles size={14} />准备面试</Button>
          <Button disabled={!job || p.busy || !p.can("record")} onClick={() => setRecording({ start: true })}><MessageSquare size={14} />复盘</Button>
          <Button disabled={!job || p.busy || !p.can("summary") || !mine.reviews.length || summary?.state === "running"} onClick={() => setSumming(true)}><Sparkles size={14} />总结复盘</Button>
        </div>
        {job && summary && <div className="ml-auto max-w-sm"><SeekerJobLine end job={summary} running="正在总结复盘…" failed="没总结成" retry={() => setSumming(true)} /></div>}
      </div>
      {!job ? (withRecords.length ? null : <EmptyState title="还没有面试记录" />) : <>
        {mine.preps.length > 0 && <section className="space-y-2"><h3 className="text-sm font-bold">准备<span className="ml-2 font-normal text-muted-foreground">{mine.preps.length}</span></h3>{mine.preps.map((x, i) => <PrepCard key={x.id} prep={x} first={i === 0} {...p} write={write} />)}</section>}
        {(mine.reviews.length > 0 || mine.drafts.length > 0) && <section className="space-y-2"><h3 className="text-sm font-bold">复盘<span className="ml-2 font-normal text-muted-foreground">{mine.reviews.length}</span></h3>
          {mine.drafts.map((d) => <DraftCard key={d.session} draft={d} busy={p.busy} resume={() => setRecording({ session: d.session })} discard={() => void discard(d)} />)}
          {mine.reviews.map((x, i) => <ReviewCard key={x.id} review={x} first={i === 0 && !mine.drafts.length} {...p} write={write} edit={() => setEditing(structuredClone(x))} />)}
        </section>}
        {mine.summaries.length > 0 && <section className="space-y-2"><h3 className="text-sm font-bold">总结<span className="ml-2 font-normal text-muted-foreground">{mine.summaries.length}</span></h3>{mine.summaries.map((x, i) => <SummaryCard key={x.id} summary={x} first={i === 0} reviews={data.reviews} preps={mine.preps} {...p} write={write} />)}</section>}
        {!count(mine) && <EmptyState title="这个岗位还没有准备和复盘" />}
      </>}
    </div>
    {prepping && job && <PrepDialog job={job} ready={p.can("prep")} close={() => setPrepping(false)} done={(out) => { qc.setQueryData(RATER_INTERVIEWS_KEY, out); setPrepping(false); }} />}
    {editing && job && <ReviewEditor initial={editing} busy={p.busy} close={() => setEditing(null)} save={(r) => write(() => interviewApi.reviewSave(r, r.id ? null : job.id), () => setEditing(null))} />}
    {recording && "start" in recording && <RecordStart jobs={list} job={jobId} next={(j) => { const n = j ? of(j) : null; return ROUNDS[Math.min(ROUNDS.length - 1, n ? n.reviews.length + n.drafts.length : 0)]; }} close={() => setRecording(null)} started={(d) => { setJobId(d.review.job.id); void qc.invalidateQueries({ queryKey: RATER_RECORDS_KEY }); setRecording({ session: d.session }); }} />}
    {recording && "session" in recording && <RecordChat key={recording.session} session={recording.session} close={() => { setRecording(null); void qc.invalidateQueries({ queryKey: RATER_RECORDS_KEY }); }} saved={(out, more) => { qc.setQueryData(RATER_INTERVIEWS_KEY, out); qc.setQueryData<RaterRecord[]>(RATER_RECORDS_KEY, (old) => old?.filter((x) => x.session !== recording.session)); void qc.invalidateQueries({ queryKey: RATER_RECORDS_KEY }); setRecording(more ? { start: true } : null); }} />}
    {summing && job && <SummaryDialog job={job} preps={mine.preps} reviews={mine.reviews} close={() => setSumming(false)} start={(reviews, prep) => { setSumming(false); startSeekerJob(summaryJob(job.id), async () => { const out = await interviewApi.summarize(reviews, prep); qc.setQueryData(RATER_INTERVIEWS_KEY, out); return { message: "总结好了，在下面「总结」里" }; }); }} />}
  </div>;
}

/** 准备面试: language, form, this time's request → Rater's model (it takes a while). */
function PrepDialog(props: { job: Job; ready: boolean; close: () => void; done: (out: RaterInterviews) => void }) {
  const seeker = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const [lang, setLang] = useState<string>(RATER_INTERVIEW_LANGS[0]);
  const [form, setForm] = useState("");
  const [request, setRequest] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // the resume it will read (as Rust picks it): the job's résumé in use still under cv/, else the draft of that language
  const w = seeker.data;
  const used = useQuery({ queryKey: seekerJobResumesKey(props.job.id), queryFn: () => seekerApi.jobResumes(props.job.id) }).data?.linked;
  const link = used?.find((l) => l.current) ?? used?.[0];
  const resume = link ? `${link.name.split("/").pop()}${link.current ? "（本次使用）" : ""}` : lang === "English" ? (w?.en?.draft ? "英文草稿" : "没有英文简历") : w?.draft ? "中文草稿" : "没有中文简历";
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setRunning(true); setError(null);
    try { props.done(await interviewApi.prep({ job: props.job.id, lang, form, request })); }
    catch (err) { setError(errorText(err)); }
    finally { setRunning(false); }
  };
  return <Modal wide title={`准备面试 · ${jobLabel(props.job)}`} error={error} onClose={() => { if (!running) props.close(); }} submitLabel={running ? "正在准备…" : "准备"} submitDisabled={running || !props.ready} onSubmit={submit}>
    <fieldset disabled={running} className="space-y-3">
      <p className="text-sm text-muted-foreground">简历：{resume}</p>
      <Field label="面试语言"><Segmented<string> ariaLabel="面试语言" value={lang} onChange={setLang} options={RATER_INTERVIEW_LANGS.map((l) => ({ value: l, label: l }))} /></Field>
      <Field label="形式"><TextInput aria-label="形式" value={form} maxLength={60} onChange={(e) => setForm(e.target.value)} /></Field>
      <Field label="本次要求"><TextInput aria-label="本次要求" value={request} onChange={(e) => setRequest(e.target.value)} /></Field>
    </fieldset>
  </Modal>;
}

function Card(props: { title: ReactNode; meta?: ReactNode; first: boolean; actions: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(props.first);
  return <article className="group rounded-card border bg-card">
    <div className="flex items-center gap-2 py-1.5 pr-2 pl-3">
      <button type="button" aria-expanded={open} onClick={() => setOpen((o) => !o)} className="flex min-w-0 flex-1 items-center gap-2 rounded-control py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <ChevronRight size={14} className={`shrink-0 text-muted-foreground transition-transform duration-150 ${open ? "rotate-90" : ""}`} />
        <span className="truncate font-medium">{props.title}</span>
        {props.meta && <span className="min-w-0 shrink-0 truncate text-xs text-muted-foreground">{props.meta}</span>}
      </button>
      <div className="flex shrink-0 items-center opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100">{props.actions}</div>
    </div>
    {open && <div className="space-y-4 border-t px-4 py-3 text-sm">{props.children}</div>}
  </article>;
}

function FollowItem(props: { f: RaterFollow; busy: boolean; keep: () => void; label?: string }) {
  return <li className="space-y-1 rounded-control border-[1.5px] border-dashed px-3 py-2">
    <div className="flex items-start gap-2"><span className="min-w-0 flex-1 font-medium">{props.f.question}</span><span className="shrink-0 text-xs text-muted-foreground">{[props.f.category, props.f.about].filter(Boolean).join(" · ")}</span></div>
    {props.f.answer && <p className="whitespace-pre-wrap text-muted-foreground">{props.f.answer}</p>}
    {props.f.saved ? <span className="text-xs text-muted-foreground">已存进题库</span> : <Button variant="ghost" className="-ml-2" disabled={props.busy} onClick={props.keep}><Plus size={14} />{props.label ?? "存进题库"}</Button>}
  </li>;
}

function PrepCard(p: Props & { prep: RaterPrep; first: boolean; write: Write }) {
  const qc = useQueryClient();
  const toast = useToast();
  const x = p.prep;
  const [sheet, setSheet] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirm, dialog] = useConfirm();
  const picks = x.questions.map((id) => p.bank.questions.find((q) => q.id === id)).filter((q) => q != null);
  const practice = () => void p.guarded(async () => {
    qc.setQueryData(RATER_PRACTICE_KEY, await raterApi.start({ mode: RATER_MODES[0], job: null, questions: picks.map((q) => q.id), count: 0, advice: "" }));
    p.toCards();
  });
  const remove = async () => { if (await confirm({ title: "删除这份准备？", message: "速查卡和挑的题都去掉（存进题库的题还在）。", danger: true, confirmLabel: "删除" })) void p.write(() => interviewApi.remove(x.id)); };
  return <Card first={p.first} title={`${day(x.createdAt)} · ${x.lang}${x.form ? ` · ${x.form}` : ""}`} meta={x.model} actions={<>
    <IconButton label="编辑速查卡" disabled={p.busy} onClick={() => setSheet(x.sheet)}><Pencil size={14} /></IconButton>
    <IconButton label="另存 Word" onClick={() => setSaving(true)}><FileDown size={14} /></IconButton>
    <IconButton label="删除" danger disabled={p.busy} onClick={() => void remove()}><Trash2 size={14} /></IconButton>
  </>}>
    <p className="text-xs text-muted-foreground">{[`简历：${x.resume}`, x.note].filter(Boolean).join(" · ")}</p>
    {x.advice && <p className="whitespace-pre-wrap">{x.advice}</p>}
    {sheet != null
      ? <div className="space-y-2"><TextArea aria-label="速查卡" rows={18} className="font-mono text-xs" value={sheet} onChange={(e) => setSheet(e.target.value)} /><div className="flex justify-end gap-2"><Button variant="ghost" onClick={() => setSheet(null)}>取消</Button><Button variant="primary" disabled={p.busy || !sheet.trim()} onClick={() => void p.write(() => interviewApi.prepSave({ ...x, sheet }), () => setSheet(null))}>保存</Button></div></div>
      : <div className="rounded-control border bg-background px-4 py-3"><MarkdownView source={x.sheet} /></div>}
    <div className="flex flex-wrap gap-2"><Button onClick={() => setSaving(true)}><FileDown size={14} />另存 Word</Button>{picks.length > 0 && <Button disabled={p.busy} onClick={practice}><Play size={14} />用这些题练一轮</Button>}</div>
    {picks.length > 0 && <div className="space-y-1"><p className="text-xs text-muted-foreground">挑的题 {picks.length}</p><ol className="list-decimal space-y-0.5 pl-5">{picks.map((q) => <li key={q.id}>{q.question}<span className="ml-2 text-xs text-muted-foreground">{q.category}</span></li>)}</ol></div>}
    {x.follows.length > 0 && <div className="space-y-2"><p className="text-xs text-muted-foreground">可能的追问 {x.follows.length}</p><ul className="space-y-2">{x.follows.map((f, i) => <FollowItem key={i} f={f} busy={p.busy} keep={() => void p.write(() => interviewApi.prepKeep(x.id, i))} />)}</ul></div>}
    {saving && <WordDialog id={x.id} toast={toast} close={() => setSaving(false)} />}
    {dialog}
  </Card>;
}

/** 另存 Word: name and folder (remembered), then 打开 / 在访达中显示. */
function WordDialog(props: { id: string; toast: ReturnType<typeof useToast>; close: () => void }) {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { void interviewApi.wordPlace(props.id).then((w) => { setName(w.name); setFolder(w.folder); }, (e) => setError(errorText(e))); }, [props.id]);
  const pick = async () => { const d = await openDialog({ directory: true, multiple: false, title: "保存到", defaultPath: folder || undefined }); if (typeof d === "string") setFolder(d); };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (saved) return props.close();
    setError(null);
    try { setSaved(await interviewApi.word(props.id, folder, name)); }
    catch (err) { setError(errorText(err)); }
  };
  return <Modal wide title="另存 Word" error={error} onClose={props.close} onSubmit={submit} submitLabel={saved ? "完成" : "保存"} submitDisabled={!saved && (!name.trim() || !folder)}>
    {saved ? <div className="space-y-3 text-sm"><p className="selectable break-all">{saved}</p><div className="flex gap-2"><Button onClick={() => open(saved, props.toast)}>打开</Button><Button onClick={() => open(folderOf(saved), props.toast)}>在访达中显示</Button></div></div> : <>
      <Field label="文件名"><TextInput autoFocus aria-label="文件名" value={name} maxLength={80} onChange={(e) => setName(e.target.value)} /></Field>
      <Field label="保存到"><div className="flex items-center gap-2"><p className="selectable min-w-0 flex-1 truncate text-sm" title={folder}>{folder.replace(/^\/Users\/[^/]+/, "~")}</p><Button onClick={() => void pick()}>选择…</Button></div></Field>
    </>}
  </Modal>;
}


/** What a summary (or a review's own 对照 from before) found: hits, misses, questions to keep, rule points. */
function AnalysisView(props: { a: RaterAnalysis; asked: (m: { review?: string; asked: number }) => string; busy: boolean; keep: (i: number) => void; rule: (i: number) => void }) {
  const a = props.a;
  return <>
    {a.summary && <p className="whitespace-pre-wrap">{a.summary}</p>}
    {a.hits.length > 0 && <div><p className="text-xs text-muted-foreground">准备到了</p><ul className="list-disc space-y-0.5 pl-5">{a.hits.map((h, i) => <li key={i}>{props.asked(h)}<span className="text-muted-foreground"> → {h.text}</span></li>)}</ul></div>}
    {a.misses.length > 0 && <div><p className="text-xs text-primary">没准备到</p><ul className="list-disc space-y-0.5 pl-5">{a.misses.map((h, i) => <li key={i}>{props.asked(h)}<span className="text-muted-foreground"> → {h.text}</span></li>)}</ul></div>}
    {a.questions.length > 0 && <div className="space-y-2"><p className="text-xs text-muted-foreground">值得加进题库</p><ul className="space-y-2">{a.questions.map((f, i) => <FollowItem key={i} f={f} busy={props.busy} keep={() => props.keep(i)} />)}</ul></div>}
    {a.rules.length > 0 && <div className="space-y-2"><p className="text-xs text-muted-foreground">提议的规则</p>{a.rules.map((x, i) => <div key={i} className="space-y-1 rounded-control border px-3 py-2"><p><span className="font-medium">{x.title}</span><span className="ml-2 text-xs text-muted-foreground">{x.use}</span></p><p>{x.text}</p>{x.why && <p className="text-xs text-muted-foreground">{x.why}</p>}{x.adopted ? <span className="text-xs text-muted-foreground">已加进规则</span> : <Button variant="ghost" className="-ml-2" disabled={props.busy} onClick={() => props.rule(i)}><Plus size={14} />加进规则</Button>}</div>)}</div>}
  </>;
}

function ReviewCard(p: Props & { review: RaterReview; first: boolean; write: Write; edit: () => void }) {
  const r = p.review;
  const [confirm, dialog] = useConfirm();
  const remove = async () => { if (await confirm({ title: "删除这次复盘？", message: "记下的问答都去掉（存进题库、加进规则的还在）。", danger: true, confirmLabel: "删除" })) void p.write(() => interviewApi.remove(r.id)); };
  const tone = r.result === "通过" ? "text-green-700 dark:text-green-400" : r.result === "没过" ? "text-destructive" : "text-muted-foreground";
  return <Card first={p.first} title={reviewTitle(r)} meta={<span className={tone}>{r.result}</span>} actions={<>
    <IconButton label="编辑" disabled={p.busy} onClick={p.edit}><Pencil size={14} /></IconButton>
    <IconButton label="删除" danger disabled={p.busy} onClick={() => void remove()}><Trash2 size={14} /></IconButton>
  </>}>
    <ol className="space-y-2">{r.items.map((it: RaterAsked, i) => <li key={i} className="space-y-0.5"><p className="font-medium">{i + 1}. {it.question}</p>{it.answer && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我答：</span>{it.answer}</p>}{it.feedback && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">反应：</span>{it.feedback}</p>}</li>)}</ol>
    {r.feedback && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">面试官的反馈：</span>{r.feedback}</p>}
    {r.thoughts && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我的判断：</span>{r.thoughts}</p>}
    {r.analysis && <section className="space-y-2 rounded-control border bg-background px-3 py-2">
      <h4 className="flex items-center gap-1.5 font-bold"><Sparkles size={14} className="text-[var(--module-picky)]" />对照准备{r.analysis.model && <span className="font-normal text-xs text-muted-foreground">{r.analysis.model}</span>}</h4>
      <AnalysisView a={r.analysis} asked={(m) => r.items[m.asked]?.question ?? ""} busy={p.busy} keep={(i) => void p.write(() => interviewApi.reviewKeep(r.id, i))} rule={(i) => void p.write(() => interviewApi.reviewRule(r.id, i))} />
    </section>}
    {dialog}
  </Card>;
}

/** A 复盘's fields: date, round, form, result; each question asked (my answer, the reaction); the feedback; my own judgement. */
function ReviewFields(props: { v: RaterReview; set: (patch: Partial<RaterReview>) => void; busy: boolean }) {
  const { v, set } = props;
  const setItem = (i: number, patch: Partial<RaterAsked>) => set({ items: v.items.map((it, j) => (j === i ? { ...it, ...patch } : it)) });
  return <fieldset disabled={props.busy} className="-m-1 max-h-[64vh] space-y-3 overflow-y-auto p-1">
    <div className="grid grid-cols-[10rem_1fr_1fr_8rem] gap-3">
      <Field label="日期"><DateInput ariaLabel="日期" value={v.date} onChange={(date) => set({ date })} /></Field>
      <Field label="轮次"><TextInput aria-label="轮次" value={v.round} maxLength={40} onChange={(e) => set({ round: e.target.value })} /></Field>
      <Field label="形式"><TextInput aria-label="形式" value={v.form} maxLength={60} onChange={(e) => set({ form: e.target.value })} /></Field>
      <Field label="结果"><Dropdown block ariaLabel="结果" value={v.result} options={RATER_RESULTS.map((x) => ({ value: x, label: x }))} onChange={(result) => set({ result })} /></Field>
    </div>
    <Field label="问了什么" action={<button type="button" className="text-xs text-primary" onClick={() => set({ items: [...v.items, { question: "", answer: "", feedback: "" }] })}>＋ 一条</button>}>
      <ol className="space-y-3">{v.items.map((it, i) => <li key={i} className="space-y-1.5 rounded-control border p-2">
        <div className="flex items-center gap-2"><span className="text-xs text-muted-foreground">{i + 1}</span><TextInput aria-label={`问题 ${i + 1}`} placeholder="问题" value={it.question} onChange={(e) => setItem(i, { question: e.target.value })} /><IconButton label="去掉这一条" onClick={() => set({ items: v.items.filter((_, j) => j !== i) })}><X size={14} /></IconButton></div>
        <TextArea aria-label={`我的回答 ${i + 1}`} placeholder="我答的" rows={2} value={it.answer} onChange={(e) => setItem(i, { answer: e.target.value })} />
        <TextInput aria-label={`面试官的反应 ${i + 1}`} placeholder="面试官的反应" value={it.feedback} onChange={(e) => setItem(i, { feedback: e.target.value })} />
      </li>)}</ol>
    </Field>
    <Field label="面试官的反馈"><TextArea aria-label="面试官的反馈" rows={3} value={v.feedback} onChange={(e) => set({ feedback: e.target.value })} /></Field>
    <Field label="我的判断"><TextArea aria-label="我的判断" rows={3} value={v.thoughts} onChange={(e) => set({ thoughts: e.target.value })} /></Field>
  </fieldset>;
}

/** Editing a saved 复盘 by hand. */
function ReviewEditor(props: { initial: RaterReview; busy: boolean; close: () => void; save: (r: RaterReview) => Promise<boolean> }) {
  const [v, setV] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  return <Modal size="xl" title="编辑复盘" error={error} onClose={props.close} submitLabel="保存" submitDisabled={props.busy} onSubmit={(e) => { e.preventDefault(); void props.save(v).then((ok) => { if (!ok) setError("没存上，请重试"); }); }}>
    <ReviewFields v={v} set={(patch) => setV((x) => ({ ...x, ...patch }))} busy={props.busy} />
  </Modal>;
}

/** An unfinished record under its job: 接着记 opens its conversation, 放弃 deletes it. */
function DraftCard(props: { draft: RaterRecord; busy: boolean; resume: () => void; discard: () => void }) {
  const r = props.draft.review;
  const [confirm, dialog] = useConfirm();
  return <article className="flex items-center gap-2 rounded-card border border-dashed py-1.5 pr-2 pl-3">
    <button type="button" onClick={props.resume} className="flex min-w-0 flex-1 items-center gap-2 rounded-control py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
      <MessageSquare size={14} className="shrink-0 text-muted-foreground" />
      <span className="truncate font-medium">{reviewTitle(r)}</span>
      <span className="shrink-0 text-xs text-muted-foreground">没记完 · {r.items.length} 个问题</span>
    </button>
    <Button variant="ghost" onClick={props.resume}>接着记</Button>
    <IconButton label="放弃这次记录" danger disabled={props.busy} onClick={() => void confirm({ title: "放弃这次记录？", message: "对话和表都删掉。", danger: true, confirmLabel: "放弃" }).then((ok) => ok && props.discard())}><Trash2 size={14} /></IconButton>
    {dialog}
  </article>;
}

/** 复盘: the date, the job and which round → a new record's conversation. */
function RecordStart(props: { jobs: Job[]; job: number | null; next: (j: Job | null) => string; close: () => void; started: (d: RaterRecord) => void }) {
  const [date, setDate] = useState(today());
  const [jobId, setJobId] = useState(props.job);
  const job = props.jobs.find((j) => j.id === jobId) ?? null;
  const [round, setRound] = useState(() => props.next(job));
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (jobId == null) return;
    setStarting(true); setError(null);
    try { props.started(await interviewApi.recordStart(jobId, date, round)); }
    catch (err) { setError(errorText(err)); }
    finally { setStarting(false); }
  };
  return <Modal wide title="复盘" error={error} onClose={props.close} submitLabel="开始" submitDisabled={starting || jobId == null || !date} onSubmit={submit}>
    <fieldset disabled={starting} className="space-y-3">
      <div className="grid grid-cols-[10rem_1fr] gap-3">
        <Field label="日期"><DateInput ariaLabel="日期" value={date} onChange={setDate} /></Field>
        <Field label="第几轮"><Dropdown block ariaLabel="第几轮" value={round} options={ROUNDS.map((r) => ({ value: r, label: r }))} onChange={setRound} /></Field>
      </div>
      <Field label="岗位"><PickField<number | null> block ariaLabel="岗位" title="复盘哪个岗位的面试" value={jobId} placeholder="选择岗位" options={jobPicks(props.jobs)} onChange={(id) => { setJobId(id); setRound(props.next(props.jobs.find((j) => j.id === id) ?? null)); }} /></Field>
    </fieldset>
  </Modal>;
}

/** A record's conversation (小拾's engine, Rater's model; not in 小拾's list) beside its table. When the model
 * says the table is filled (and after 存成复盘), the table opens to check: 继续记录 / 确认. */
function RecordChat(props: { session: number; close: () => void; saved: (out: RaterInterviews, more: boolean) => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const status = usePickyStatus().data;
  const templates = useTemplates().data ?? [];
  const ref = useRef<HTMLDialogElement>(null);
  const transcript = useRef<TranscriptHandle>(null);
  const composer = useRef<ComposerHandle>(null);
  useEffect(() => { const d = ref.current; if (d && !d.open) d.showModal(); }, []);
  const current = props.session;
  const { live, busyHere, run, shownWith } = useLiveReply({ current, status, onSent: () => {}, onStart: () => transcript.current?.toBottom() });
  const activeHere = activeIn(status, current);
  const messages = useQuery({ queryKey: messagesKey(current), queryFn: () => pickyApi.messages(current), refetchInterval: activeHere && !live ? 1000 : false });
  const writing = busyHere || !!activeHere;
  const record = useQuery({ queryKey: [...RATER_RECORDS_KEY, current], queryFn: () => interviewApi.record(current), refetchInterval: writing ? 1200 : false });
  useEffect(() => { if (!writing) void record.refetch(); }, [writing]); // eslint-disable-line react-hooks/exhaustive-deps
  const draftKey = `rater-record:${current}`;
  const ask = useAskedSkills(draftKey);
  const web = ask.skills.find((s) => s.key === WEB_SKILL);
  const prep = usePrepare({ current, draftProject: null, draftOff: false, projectId: null, knowledge: null, knowledgeOn: true });
  const busy = writing || prep.preparing != null;
  const [confirming, setConfirming] = useState(false);
  const [confirm, confirmDialog] = useConfirm();
  // the model said it's filled: check it once per fill
  const answered = useRef(0);
  const d = record.data;
  useEffect(() => {
    if (d?.ready && !writing && d.fills > answered.current) { answered.current = d.fills; setConfirming(true); }
  }, [d?.ready, d?.fills, writing]); // eslint-disable-line react-hooks/exhaustive-deps

  const send = async (text: string) => {
    prep.clearFail();
    const got = await prep.prepareFor(text, [], false);
    if ("error" in got) { prep.failCard(got, () => composer.current?.send()); return false; }
    const extras = { attachments: [], files: [], passages: got.passages, memories: got.memories };
    const sent = await run("send", text, (on) => pickyApi.send(current, null, null, false, null, null, text, extras, null, ask.asked, null, on), [], null);
    if (sent) ask.clear(draftKey);
    return !!sent;
  };
  const versionRun = async (m: ChatMessage, text: string | null) => {
    const saved = messages.data ?? [];
    const q = text != null ? m : saved[saved.findIndex((x) => x.id === m.id) - 1];
    prep.clearFail();
    const got = await prep.prepareFor(text ?? q?.content ?? "", [], false);
    if ("error" in got) { prep.failCard(got, () => void versionRun(m, text)); return; }
    void run("version", text ?? "", (on) => (text != null ? pickyApi.edit(m.id, text, got.passages, got.memories, on) : pickyApi.regenerate(m.id, got.passages, got.memories, on)));
  };
  const stop = () => { const runId = live?.runId ?? activeHere?.run; if (runId != null) void pickyApi.stop(runId).catch((e) => toast(errorText(e), "error")); };
  const switchVersion = (m: ChatMessage, delta: -1 | 1) => void pickyApi.switchVersion(m.id, delta).then(() => qc.invalidateQueries({ queryKey: messagesKey(current) }), (e) => toast(errorText(e), "error"));
  const back = (r: RaterReview) => { setConfirming(false); void interviewApi.recordEdit(current, r).then((x) => qc.setQueryData([...RATER_RECORDS_KEY, current], x), (e) => toast(errorText(e), "error")); };
  const save = async (r: RaterReview, more: boolean) => props.saved(await interviewApi.recordSave(current, r), more);
  const discard = async () => {
    if (!await confirm({ title: "放弃这次记录？", message: "对话和表都删掉。", danger: true, confirmLabel: "放弃" })) return;
    try { qc.setQueryData(RATER_RECORDS_KEY, await interviewApi.recordDiscard(current)); props.close(); }
    catch (e) { toast(errorText(e), "error"); }
  };
  const r = d?.review;
  const empty = <span className="text-muted-foreground">—</span>;
  return <dialog ref={ref} aria-label="复盘" onCancel={(e) => { e.preventDefault(); props.close(); }} className="m-auto h-[84vh] w-[1100px] max-w-[calc(100vw-48px)] overflow-hidden rounded-card border bg-popover p-0 text-foreground shadow-xl backdrop:bg-black/30">
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-2 border-b px-5 py-3">
        <MessageSquare size={16} className="shrink-0 text-[var(--module-rater)]" />
        <p className="min-w-0 flex-1 truncate font-bold">复盘{r && ` · ${r.job.label} · ${reviewTitle(r)}`}</p>
        <IconButton label="关掉（没记完的留在这个岗位下）" onClick={props.close}><X size={16} /></IconButton>
      </header>
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <Transcript ref={transcript} compact sessionKey={`rater-record-${current}`} messages={shownWith(messages.data ?? [])} busy={busy} empty={<p className="text-base">说说这场面试吧，从第一个问题开始。</p>} onEdit={(m, text) => void versionRun(m, text)} onRegenerate={(m) => void versionRun(m, null)} onSwitch={switchVersion} onOpenFile={(id) => openFile(id, toast)} onStop={stop} />
          <div className="space-y-2 px-4 pt-2 pb-4">
            <PrepNotices prep={prep} />
            <Composer ref={composer} draftKey={draftKey} busy={busy} templates={templates} onManageTemplates={() => toast("模板在小拾页管理")} skills={ask.usable} onAskSkill={ask.toggle}
              tools={web?.enabled && <button type="button" aria-pressed={ask.asked.includes(WEB_SKILL)} onClick={() => ask.toggle(WEB_SKILL)} className={`${toolClass} ${ask.asked.includes(WEB_SKILL) ? "bg-accent text-accent-foreground" : ""}`}><Globe size={14} strokeWidth={1.5} /> 联网</button>}
              chips={ask.asked.length > 0 && <div className="flex flex-wrap gap-1.5 px-3 pt-2.5"><SkillChips skills={ask.skills} asked={ask.asked} onRemove={ask.toggle} /></div>}
              onSend={send} onStop={stop} />
          </div>
        </div>
        <aside aria-label="复盘表" className="flex w-[340px] shrink-0 flex-col border-l bg-card">
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3 text-sm">
            {!r ? <p className="text-muted-foreground">{record.isError ? errorText(record.error) : "正在读取…"}</p> : <>
              <dl className="grid grid-cols-[4rem_1fr] gap-x-2 gap-y-1"><dt className="text-muted-foreground">形式</dt><dd>{r.form || empty}</dd><dt className="text-muted-foreground">结果</dt><dd>{r.result || empty}</dd></dl>
              <div className="space-y-1.5"><p className="text-xs text-muted-foreground">问了什么</p>{r.items.length ? <ol className="space-y-2">{r.items.map((it, i) => <li key={i} className="space-y-0.5"><p className="font-medium">{i + 1}. {it.question}</p>{it.answer && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我答：</span>{it.answer}</p>}{it.feedback && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">反应：</span>{it.feedback}</p>}</li>)}</ol> : empty}</div>
              <div><p className="text-xs text-muted-foreground">面试官的反馈</p><p className="whitespace-pre-wrap">{r.feedback || empty}</p></div>
              <div><p className="text-xs text-muted-foreground">我的判断</p><p className="whitespace-pre-wrap">{r.thoughts || empty}</p></div>
            </>}
          </div>
          <div className="flex items-center gap-2 border-t px-4 py-3">
            <Button variant="ghost-danger" disabled={busy} onClick={() => void discard()}>放弃</Button>
            <span className="flex-1" />
            <Button variant="primary" disabled={busy || !r?.items.length} onClick={() => setConfirming(true)}>存成复盘</Button>
          </div>
        </aside>
      </div>
    </div>
    {confirming && d && <RecordConfirm record={d} back={back} save={save} />}
    {confirmDialog}
  </dialog>;
}

/** The filled table to check (and change) before it is saved: 继续记录 saves it and starts the next one; 确认 saves it. */
function RecordConfirm(props: { record: RaterRecord; back: (r: RaterReview) => void; save: (r: RaterReview, more: boolean) => Promise<void> }) {
  const [v, setV] = useState(props.record.review);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const go = async (more: boolean) => {
    setSaving(true); setError(null);
    try { await props.save(v, more); }
    catch (e) { setError(errorText(e)); setSaving(false); }
  };
  return <Modal size="xl" title="存成复盘" error={error} onClose={() => props.back(v)} cancelLabel="返回对话" submitLabel="确认" submitDisabled={saving || !v.items.some((x) => x.question.trim())} footerLeft={<Button disabled={saving || !v.items.some((x) => x.question.trim())} onClick={() => void go(true)}>继续记录</Button>} onSubmit={(e) => { e.preventDefault(); void go(false); }}>
    <ReviewFields v={v} set={(patch) => setV((x) => ({ ...x, ...patch }))} busy={saving} />
  </Modal>;
}

/** 总结复盘: which 速查卡 (the newest by default; or none) and which reviews (by default those since that
 * 速查卡, else all) → Rater's model, in the background. */
function SummaryDialog(props: { job: Job; preps: RaterPrep[]; reviews: RaterReview[]; close: () => void; start: (reviews: string[], prep: string | null) => void }) {
  const NONE = "";
  const oldest = [...props.reviews].sort((a, b) => a.date.localeCompare(b.date) || a.createdAt.localeCompare(b.createdAt));
  const auto = (pid: string) => { const p = props.preps.find((x) => x.id === pid); const since = oldest.filter((r) => !p || r.date >= p.createdAt.slice(0, 10)); return (since.length ? since : oldest).map((r) => r.id); };
  const [prep, setPrep] = useState<string>(props.preps[0]?.id ?? NONE);
  const [picked, setPicked] = useState<string[]>(() => auto(props.preps[0]?.id ?? NONE));
  const [touched, setTouched] = useState(false);
  const toggle = (id: string) => { setTouched(true); setPicked((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id])); };
  return <Modal wide title={`总结复盘 · ${jobLabel(props.job)}`} error={null} onClose={props.close} submitLabel="总结" submitDisabled={!picked.length} onSubmit={(e) => { e.preventDefault(); props.start(oldest.filter((r) => picked.includes(r.id)).map((r) => r.id), prep || null); }}>
    <div className="space-y-3">
      <Field label="速查卡"><Dropdown<string> block ariaLabel="对照哪份速查卡" value={prep} options={[...props.preps.map((x) => ({ value: x.id, label: `${day(x.createdAt)} · ${x.lang}${x.form ? ` · ${x.form}` : ""}` })), { value: NONE, label: "不对照速查卡（只对照题库）" }]} onChange={(v) => { setPrep(v); if (!touched) setPicked(auto(v)); }} /></Field>
      <Field label="复盘"><div className="-m-1 max-h-[40vh] space-y-1.5 overflow-y-auto p-1">{oldest.map((r) => <PickRow key={r.id} on={picked.includes(r.id)} onClick={() => toggle(r.id)} framed className="px-3 py-2"><span className="min-w-0 flex-1 truncate">{reviewTitle(r)}</span><span className="shrink-0 text-xs text-muted-foreground">{[r.result, `${r.items.length} 个问题`].filter(Boolean).join(" · ")}</span></PickRow>)}</div></Field>
    </div>
  </Modal>;
}

function SummaryCard(p: Props & { summary: RaterSummary; first: boolean; reviews: RaterReview[]; preps: RaterPrep[]; write: Write }) {
  const s = p.summary;
  const [confirm, dialog] = useConfirm();
  const rs = s.reviews.map((id) => p.reviews.find((r) => r.id === id));
  const prep = p.preps.find((x) => x.id === s.prep);
  const name = (r: RaterReview | undefined) => (r ? r.round || day(r.date) : "已删除");
  const asked = (m: { review?: string; asked: number }) => { const r = p.reviews.find((x) => x.id === m.review); const q = r?.items[m.asked]?.question ?? "（这次复盘已删除）"; return s.reviews.length > 1 ? `${name(r)} · ${q}` : q; };
  const remove = async () => { if (await confirm({ title: "删除这份总结？", message: "存进题库、加进规则的还在。", danger: true, confirmLabel: "删除" })) void p.write(() => interviewApi.remove(s.id)); };
  return <Card first={p.first} title={`${day(s.createdAt)} · ${rs.map(name).join("、")}`} meta={s.analysis.model} actions={<IconButton label="删除" danger disabled={p.busy} onClick={() => void remove()}><Trash2 size={14} /></IconButton>}>
    <p className="text-xs text-muted-foreground">{prep ? `对照速查卡：${day(prep.createdAt)} · ${prep.lang}${prep.form ? ` · ${prep.form}` : ""}` : s.prep ? "对照的速查卡已删除" : "没对照速查卡，只对照题库"}</p>
    <AnalysisView a={s.analysis} asked={asked} busy={p.busy} keep={(i) => void p.write(() => interviewApi.reviewKeep(s.id, i))} rule={(i) => void p.write(() => interviewApi.reviewRule(s.id, i))} />
    {dialog}
  </Card>;
}
