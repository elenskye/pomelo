// Rater 面试 (v0.7 阶段 ⑦; Rust picky/assistants/rater/interview.rs): one 投石问路 job at a time — its 准备
// (小拾 picks the bank's questions, adds likely follow-ups, writes a one-page 速查卡 the user edits and
// saves as Word; 用这些题练一轮 starts a card round) and its 复盘 (what was asked and answered, the
// feedback, the user's own judgement; 对照准备 → hits, misses, questions to keep, rule points to adopt).
// The job panel's 「面试」 opens this page with the job (and the prep / review dialog) via useOpenRequest.
import { PickField } from "@/shared/PickField";
import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronRight, FileDown, Pencil, Play, Plus, Sparkles, Trash2, X } from "lucide-react";
import { jobLabel, useJobs, type Job, jobPicks } from "@/modules/jobs/api";
import { DateInput } from "@/shared/DateInput";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { api as ipc } from "@/shared/ipc";
import { useOpenRequest } from "@/shared/open";
import { Button, EmptyState, Field, IconButton, Segmented, TextArea, TextInput } from "@/shared/ui";
import { MarkdownView } from "../MarkdownView";
import { SEEKER_KEY, seekerApi, seekerJobResumesKey } from "./seekerApi";
import { RATER_INTERVIEWS_KEY, RATER_INTERVIEW_LANGS, RATER_KEY, RATER_MODES, RATER_PRACTICE_KEY, RATER_RESULTS, interviewApi, raterApi, sameJob, type RaterAsked, type RaterBank, type RaterFollow, type RaterInterviews, type RaterPrep, type RaterReview } from "./raterApi";

type Guarded = (action: () => Promise<void>) => Promise<boolean>;
type Props = { bank: RaterBank; model: string | null; busy: boolean; guarded: Guarded; toCards: () => void };
const today = () => { const d = new Date(); const p = (n: number) => String(n).padStart(2, "0"); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`; };
const day = (iso: string) => iso.slice(0, 10).replace(/-/g, " / ");
const blankReview = (): RaterReview => ({ id: "", job: { id: 0, createdAt: "", label: "" }, date: today(), round: "", form: "", result: "待定", items: [{ question: "", answer: "", feedback: "" }], feedback: "", thoughts: "", analysis: null, createdAt: "" });
const open = (path: string, toast: ReturnType<typeof useToast>) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
const folderOf = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";

export function RaterInterview(p: Props) {
  const qc = useQueryClient();
  const jobs = useJobs();
  const all = useQuery({ queryKey: RATER_INTERVIEWS_KEY, queryFn: interviewApi.all });
  const [jobId, setJobId] = useState<number | null>(null);
  const [prepping, setPrepping] = useState(false);
  const [editing, setEditing] = useState<RaterReview | null>(null);
  // the job panel's 「面试」: this job, and maybe straight to 准备 / 复盘
  useOpenRequest((hit) => {
    if (hit.module !== "rater") return;
    setJobId(hit.id);
    if (hit.kind === "prep") setPrepping(true);
    if (hit.kind === "review") setEditing(blankReview());
  });
  const data = all.data;
  const list = jobs.data ?? [];
  const of = (j: Job) => ({ preps: data?.preps.filter((x) => sameJob(x.job, j)) ?? [], reviews: data?.reviews.filter((x) => sameJob(x.job, j)) ?? [] });
  // jobs with records, newest first; the first one shown when none is picked
  const withRecords = list.map((j) => ({ j, ...of(j) })).filter((x) => x.preps.length || x.reviews.length).sort((a, b) => [...b.preps, ...b.reviews].map((x) => x.createdAt).sort().at(-1)!.localeCompare([...a.preps, ...a.reviews].map((x) => x.createdAt).sort().at(-1)!));
  useEffect(() => { if (jobId == null && withRecords.length) setJobId(withRecords[0].j.id); }, [withRecords.length]); // eslint-disable-line react-hooks/exhaustive-deps
  const job = list.find((j) => j.id === jobId) ?? null;
  const write = (action: () => Promise<RaterInterviews>, after?: () => void) => p.guarded(async () => {
    qc.setQueryData(RATER_INTERVIEWS_KEY, await action());
    void qc.invalidateQueries({ queryKey: RATER_KEY, exact: true });
    after?.();
  });
  if (all.isError) return <EmptyState title="面试记录暂时打不开" hint={errorText(all.error)} action={<Button onClick={() => void all.refetch()}>重试</Button>} />;
  if (!data) return <EmptyState title="正在读取…" />;
  const mine = job ? of(job) : { preps: [], reviews: [] };
  return <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pt-4 pb-6">
    <div className="mx-auto max-w-3xl space-y-6">
      <div className="flex flex-wrap items-center gap-2">
        <PickField<number | null> ariaLabel="岗位" title="哪个岗位的面试" value={jobId} placeholder="选择岗位" options={list.map((j, i) => { const n = of(j); const base = jobPicks([j])[0]; return { ...base, detail: n.preps.length + n.reviews.length ? `准备 ${n.preps.length} · 复盘 ${n.reviews.length}` : base.detail, group: n.preps.length + n.reviews.length ? "有面试记录" : "其他岗位", value: list[i].id as number | null }; }).sort((a, b) => Number(a.group !== "有面试记录") - Number(b.group !== "有面试记录"))} onChange={setJobId} />
        <span className="flex-1" />
        <Button disabled={!job || p.busy || !p.model} onClick={() => setPrepping(true)}><Sparkles size={14} />准备面试</Button>
        <Button disabled={!job || p.busy} onClick={() => setEditing(blankReview())}><Plus size={14} />复盘</Button>
      </div>
      {!job ? (withRecords.length ? null : <EmptyState title="还没有面试记录" />) : <>
        {mine.preps.length > 0 && <section className="space-y-2"><h3 className="text-sm font-bold">准备<span className="ml-2 font-normal text-muted-foreground">{mine.preps.length}</span></h3>{mine.preps.map((x, i) => <PrepCard key={x.id} prep={x} first={i === 0} {...p} write={write} />)}</section>}
        {mine.reviews.length > 0 && <section className="space-y-2"><h3 className="text-sm font-bold">复盘<span className="ml-2 font-normal text-muted-foreground">{mine.reviews.length}</span></h3>{mine.reviews.map((x, i) => <ReviewCard key={x.id} review={x} first={i === 0} preps={mine.preps} {...p} write={write} edit={() => setEditing(structuredClone(x))} />)}</section>}
        {!mine.preps.length && !mine.reviews.length && <EmptyState title="这个岗位还没有准备和复盘" />}
      </>}
    </div>
    {prepping && job && <PrepDialog job={job} model={p.model} close={() => setPrepping(false)} done={(out) => { qc.setQueryData(RATER_INTERVIEWS_KEY, out); setPrepping(false); }} />}
    {editing && job && <ReviewEditor initial={editing} busy={p.busy} close={() => setEditing(null)} save={(r) => write(() => interviewApi.reviewSave(r, r.id ? null : job.id), () => setEditing(null))} />}
  </div>;
}

/** 准备面试: language, form, this time's request → Rater's model (it takes a while). */
function PrepDialog(props: { job: Job; model: string | null; close: () => void; done: (out: RaterInterviews) => void }) {
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
  return <Modal wide title={`准备面试 · ${jobLabel(props.job)}`} error={error} onClose={() => { if (!running) props.close(); }} submitLabel={running ? "正在准备…" : "准备"} submitDisabled={running || !props.model} onSubmit={submit}>
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

function PrepCard(p: Props & { prep: RaterPrep; first: boolean; write: (action: () => Promise<RaterInterviews>, after?: () => void) => Promise<boolean> }) {
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

function ReviewCard(p: Props & { review: RaterReview; first: boolean; preps: RaterPrep[]; write: (action: () => Promise<RaterInterviews>, after?: () => void) => Promise<boolean>; edit: () => void }) {
  const qc = useQueryClient();
  const r = p.review;
  const [prep, setPrep] = useState<string | null>(p.preps[0]?.id ?? null);
  const [comparing, setComparing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, dialog] = useConfirm();
  const a = r.analysis;
  const compare = async () => {
    setComparing(true); setError(null);
    try { qc.setQueryData(RATER_INTERVIEWS_KEY, await interviewApi.compare(r.id, prep)); }
    catch (e) { setError(errorText(e)); }
    finally { setComparing(false); }
  };
  const remove = async () => { if (await confirm({ title: "删除这次复盘？", message: "记下的问答和对照都去掉（存进题库、加进规则的还在）。", danger: true, confirmLabel: "删除" })) void p.write(() => interviewApi.remove(r.id)); };
  const asked = (i: number) => r.items[i]?.question ?? "";
  const tone = r.result === "通过" ? "text-green-700 dark:text-green-400" : r.result === "没过" ? "text-destructive" : "text-muted-foreground";
  return <Card first={p.first} title={`${day(r.date)}${r.round ? ` · ${r.round}` : ""}${r.form ? ` · ${r.form}` : ""}`} meta={<span className={tone}>{r.result}</span>} actions={<>
    <IconButton label="编辑" disabled={p.busy} onClick={p.edit}><Pencil size={14} /></IconButton>
    <IconButton label="删除" danger disabled={p.busy} onClick={() => void remove()}><Trash2 size={14} /></IconButton>
  </>}>
    <ol className="space-y-2">{r.items.map((it: RaterAsked, i) => <li key={i} className="space-y-0.5"><p className="font-medium">{i + 1}. {it.question}</p>{it.answer && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我答：</span>{it.answer}</p>}{it.feedback && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">反应：</span>{it.feedback}</p>}</li>)}</ol>
    {r.feedback && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">面试官的反馈：</span>{r.feedback}</p>}
    {r.thoughts && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我的判断：</span>{r.thoughts}</p>}
    <section className="space-y-2 rounded-control border bg-background px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <h4 className="flex items-center gap-1.5 font-bold"><Sparkles size={14} className="text-[var(--module-picky)]" />对照准备</h4>
        {a?.model && <span className="text-xs text-muted-foreground">{a.model}</span>}
        <span className="flex-1" />
        {p.preps.length > 1 && <Dropdown<string | null> ariaLabel="对照哪份准备" value={prep} options={p.preps.map((x) => ({ value: x.id, label: `${day(x.createdAt)} · ${x.lang}${x.form ? ` · ${x.form}` : ""}` }))} onChange={setPrep} />}
        <Button disabled={p.busy || comparing || !p.model || !r.items.length} onClick={() => void compare()}>{comparing ? "正在对照…" : a ? "重新对照" : "对照"}</Button>
      </div>
      {error && <p className="text-destructive">{error}</p>}
      {a && <>
        {a.summary && <p className="whitespace-pre-wrap">{a.summary}</p>}
        {a.hits.length > 0 && <div><p className="text-xs text-muted-foreground">准备到了</p><ul className="list-disc space-y-0.5 pl-5">{a.hits.map((h, i) => <li key={i}>{asked(h.asked)}<span className="text-muted-foreground"> → {h.text}</span></li>)}</ul></div>}
        {a.misses.length > 0 && <div><p className="text-xs text-primary">没准备到</p><ul className="list-disc space-y-0.5 pl-5">{a.misses.map((h, i) => <li key={i}>{asked(h.asked)}<span className="text-muted-foreground"> → {h.text}</span></li>)}</ul></div>}
        {a.questions.length > 0 && <div className="space-y-2"><p className="text-xs text-muted-foreground">值得加进题库</p><ul className="space-y-2">{a.questions.map((f, i) => <FollowItem key={i} f={f} busy={p.busy} keep={() => void p.write(() => interviewApi.reviewKeep(r.id, i))} />)}</ul></div>}
        {a.rules.length > 0 && <div className="space-y-2"><p className="text-xs text-muted-foreground">提议的规则</p>{a.rules.map((x, i) => <div key={i} className="space-y-1 rounded-control border px-3 py-2"><p><span className="font-medium">{x.title}</span><span className="ml-2 text-xs text-muted-foreground">{x.use}</span></p><p>{x.text}</p>{x.why && <p className="text-xs text-muted-foreground">{x.why}</p>}{x.adopted ? <span className="text-xs text-muted-foreground">已加进规则</span> : <Button variant="ghost" className="-ml-2" disabled={p.busy} onClick={() => void p.write(() => interviewApi.reviewRule(r.id, i))}><Plus size={14} />加进规则</Button>}</div>)}</div>}
      </>}
    </section>
    {dialog}
  </Card>;
}

/** A 复盘: date, round, form, result; each question asked (my answer, the reaction); the feedback; my own judgement. */
function ReviewEditor(props: { initial: RaterReview; busy: boolean; close: () => void; save: (r: RaterReview) => Promise<boolean> }) {
  const [v, setV] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<RaterReview>) => setV((x) => ({ ...x, ...patch }));
  const setItem = (i: number, patch: Partial<RaterAsked>) => set({ items: v.items.map((it, j) => (j === i ? { ...it, ...patch } : it)) });
  return <Modal size="xl" title={props.initial.id ? "编辑复盘" : "复盘"} error={error} onClose={props.close} submitLabel="保存" submitDisabled={props.busy} onSubmit={(e) => { e.preventDefault(); void props.save(v).then((ok) => { if (!ok) setError("没存上，请重试"); }); }}>
    <fieldset disabled={props.busy} className="-m-1 max-h-[64vh] space-y-3 overflow-y-auto p-1">
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
    </fieldset>
  </Modal>;
}
