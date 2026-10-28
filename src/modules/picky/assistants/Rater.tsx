// Rater (助手, v0.7 阶段 ④–⑦; Rust picky/assistants/rater.rs, rater/qbank.rs, practice.rs, interview.rs):
// 题库 (questions by category: 简历题 by the material they're about, 突击题 by job; each with its practice
// record), 卡片 (RaterCards.tsx), 面试 (RaterInterview.tsx) and 规则 (Seeker records of Rater's 用途,
// the same editor as Seeker's; a SKILL without its own workflow shows the built-in one). The three
// 题库 SKILLs: 「让小拾补题」 (a list to tick, then one undo step) and 润色 (a diff). Rater's own model and
// 思考 sit on the sub-page row (题库, 卡片 and 面试 use them).
// Each edit: fields → 查看差异 → 确认保存, one undo step; lengths come from Rust.
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, History, Pencil, Plus, Sparkles, Trash2 } from "lucide-react";
import { moduleById } from "@/app/modules";
import { useSubPage } from "@/app/subpage";
import { jobLabel, useJobs } from "@/modules/jobs/api";
import { Dropdown } from "@/shared/Dropdown";
import { Modal } from "@/shared/Modal";
import { errorText, useConfirm, useToast } from "@/shared/feedback";
import { Button, EmptyState, Field, IconButton, SearchInput, Segmented, TextArea, TextInput } from "@/shared/ui";
import { useModels } from "../Schedules";
import { RaterCards } from "./RaterCards";
import { RaterInterview } from "./RaterInterview";
import { SeekerRules, type SeekerWrite } from "./SeekerLibrary";
import { SeekerDiff, when } from "./SeekerParts";
import { flushSeekerDraft } from "./seekerDraft";
import { SEEKER_KEY, seekerApi, type SeekerRecord, type SeekerWorkspace } from "./seekerApi";
import { RATER_CATEGORIES, RATER_KEY, RATER_LIMIT, RATER_OPTIONS_KEY, RATER_POLISH, RATER_UNSURE, RATER_USES, raterApi, type RaterAsk, type RaterBank, type RaterOptions, type RaterProposal, type RaterQuestion } from "./raterApi";

type Tab = "bank" | "cards" | "interview" | "rules";
const TABS = (moduleById("rater").subs ?? []) as { value: Tab; label: string }[];
const ALL = "全部";
const blank = (category: string): RaterQuestion => ({ id: crypto.randomUUID(), category, question: "", answer: "", tags: [], material: null, job: null, from: null, revision: 0 });
const splitTags = (text: string) => [...new Set(text.split(/[,，、]/).map((t) => t.trim()).filter(Boolean))];

export function RaterPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const [tab, setTab] = useSubPage<Tab>("rater");
  const bank = useQuery({ queryKey: RATER_KEY, queryFn: raterApi.bank });
  const seeker = useQuery({ queryKey: SEEKER_KEY, queryFn: seekerApi.workspace });
  const defaults = useQuery({ queryKey: [...RATER_KEY, "defaults"], queryFn: raterApi.defaults, staleTime: Infinity });
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const guarded = async (action: () => Promise<void>) => {
    if (lock.current) return false;
    lock.current = true; setBusy(true);
    try { await action(); return true; }
    catch (e) { toast(errorText(e), "error"); return false; }
    finally { lock.current = false; setBusy(false); }
  };
  const writeBank = (action: () => Promise<RaterBank>) => guarded(async () => { qc.setQueryData(RATER_KEY, await action()); });
  // Rater's rules are Seeker records: a pending Seeker draft goes first (undo keeps the order)
  const writeSeeker: SeekerWrite = (action) => guarded(async () => {
    if (!await flushSeekerDraft(qc)) throw new Error("Seeker 的草稿没保存上，请先到 Seeker 重试保存");
    qc.setQueryData(SEEKER_KEY, await action());
  });
  const { options, set } = useRaterOptions();
  const { models, keep } = useModels(options.model, options.thinking);
  const failed = bank.isError ? bank.error : seeker.isError ? seeker.error : null;
  if (failed) return <EmptyState title="Rater 暂时打不开" hint={errorText(failed)} action={<Button onClick={() => { void bank.refetch(); void seeker.refetch(); }}>重试</Button>} />;
  if (!bank.data || !seeker.data) return <EmptyState title="正在读取 Rater…" />;
  const materials = seeker.data.catalog.records.filter((r) => r.kind === "material" && r.enabled && (r.lang ?? "cn") === "cn");
  return <div className="relative flex min-h-0 min-w-0 flex-1 flex-col px-6 pb-4">
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b pb-3">
      <div className="-m-1 overflow-x-auto p-1"><Segmented<Tab> ariaLabel="Rater 子页面" value={tab} onChange={setTab} options={TABS} /></div>
      {tab !== "rules" && <div className="ml-auto flex items-center gap-2">
        <Dropdown<string | null> ariaLabel="Rater 的模型" value={options.model} placeholder="选择模型" options={models.filter((m) => m.value != null)} onChange={(model) => set({ ...options, model })} />
        <label className="flex items-center gap-1.5 text-sm"><input type="checkbox" checked={options.thinking} onChange={(e) => set({ model: keep(options.model, e.target.checked), thinking: e.target.checked })} />思考</label>
      </div>}
    </div>
    {tab === "bank"
      ? <QuestionBank bank={bank.data} seeker={seeker.data} materials={materials} model={options.model} busy={busy} write={writeBank} />
      : tab === "cards"
        ? <RaterCards bank={bank.data} materials={materials} model={options.model} busy={busy} guarded={guarded} />
        : tab === "interview"
          ? <RaterInterview bank={bank.data} model={options.model} busy={busy} guarded={guarded} toCards={() => setTab("cards")} />
          : <SeekerRules lang="cn" uses={RATER_USES} defaults={Object.fromEntries(defaults.data ?? [])} catalog={seeker.data.catalog} busy={busy} write={writeSeeker} aiBusy={false} onPolish={() => {}} />}
  </div>;
}

type BankProps = { bank: RaterBank; seeker: SeekerWorkspace; materials: SeekerRecord[]; model: string | null; busy: boolean; write: (action: () => Promise<RaterBank>) => Promise<boolean> };

/** Rater's model and 思考 for its SKILLs (not undone, like the other assistants' settings). */
function useRaterOptions() {
  const qc = useQueryClient();
  const toast = useToast();
  const options = useQuery({ queryKey: RATER_OPTIONS_KEY, queryFn: raterApi.options });
  const set = (next: RaterOptions) => void raterApi.setOptions(next).then((o) => qc.setQueryData(RATER_OPTIONS_KEY, o), (e) => toast(errorText(e), "error"));
  return { options: options.data ?? { model: null, thinking: false }, set };
}

/** The questions by category; 简历题 by material, 突击题 by job. */
function QuestionBank(p: BankProps) {
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string>(ALL);
  const [tag, setTag] = useState(ALL);
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [editing, setEditing] = useState<RaterQuestion | null>(null);
  const [past, setPast] = useState<string | null>(null);
  const [asking, setAsking] = useState(false);
  const [polish, setPolish] = useState<RaterProposal | null>(null);
  const [polishing, setPolishing] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const questions = p.bank.questions;
  const tags = [...new Set(questions.flatMap((q) => q.tags))];
  const shown = questions.filter((q) => (category === ALL || q.category === category) && (tag === ALL || q.tags.includes(tag)) && (!query.trim() || query.toLowerCase().split(/\s+/).filter(Boolean).every((w) => `${q.question}\n${q.answer}\n${q.tags.join(" ")}`.toLowerCase().includes(w))));
  const materialName = (id: string | null) => p.materials.find((m) => m.id === id)?.title;
  const groupOf = (q: RaterQuestion) => (q.category === "简历题" ? materialName(q.material) ?? "没挂素材" : q.category === "突击题" ? q.job?.label ?? "没关联岗位" : "");
  const remove = async (q: RaterQuestion) => {
    if (await confirm({ title: `删除「${q.question}」？`, message: "它的历史里还留着这一版。", danger: true, confirmLabel: "删除" })) void p.write(() => raterApi.remove(q.id));
  };
  const runPolish = async (q: RaterQuestion) => {
    if (polishing) return;
    setPolishing(q.id);
    try { setPolish(await raterApi.suggest({ kind: RATER_POLISH, materials: [], job: null, count: 0, question: q.id, request: "" })); }
    catch (e) { toast(errorText(e), "error"); }
    finally { setPolishing(null); }
  };
  // the practice record: 会 / 不熟 counts and the last time (不熟 last time in 柚红)
  const practiced = (id: string) => {
    const t = p.bank.practice?.[id];
    if (!t) return null;
    return <p className="text-xs text-muted-foreground">练过：会 {t.know} 次 · <span className={t.lastMark === RATER_UNSURE ? "text-primary" : ""}>不熟 {t.unsure} 次</span>{t.last && ` · 最近 ${when(t.last).slice(0, 14)}`}</p>;
  };
  const toggle = (id: string) => setOpen((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const card = (q: RaterQuestion) => {
    const seconds = p.bank.seconds?.[q.id] ?? 0;
    const from = questions.find((x) => x.id === q.from)?.question;
    return <article key={q.id} className="group rounded-card border bg-card">
      <div className="flex items-center gap-2 py-1.5 pr-2 pl-3">
        <button type="button" aria-expanded={open.has(q.id)} onClick={() => toggle(q.id)} className="flex min-w-0 flex-1 items-center gap-2 rounded-control py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <ChevronRight size={14} className={`shrink-0 text-muted-foreground transition-transform duration-150 ${open.has(q.id) ? "rotate-90" : ""}`} />
          <span className="truncate font-medium">{q.question}</span>
          <span className="min-w-0 shrink-0 truncate text-xs text-muted-foreground"><span className={seconds > RATER_LIMIT ? "text-primary" : ""}>{q.answer.trim() ? `约 ${seconds} 秒` : "还没写答案"}</span>{q.tags.length > 0 && ` · ${q.tags.join("、")}`}{polishing === q.id && " · 正在润色…"}</span>
        </button>
        <div className="flex shrink-0 items-center opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100">
          <IconButton label="润色" disabled={p.busy || !!polishing || !p.model} onClick={() => void runPolish(q)}><Sparkles size={14} /></IconButton>
          <IconButton label="编辑" disabled={p.busy} onClick={() => setEditing(structuredClone(q))}><Pencil size={14} /></IconButton>
          <IconButton label="历史" onClick={() => setPast(q.id)}><History size={14} /></IconButton>
          <IconButton label="删除" danger disabled={p.busy} onClick={() => void remove(q)}><Trash2 size={14} /></IconButton>
        </div>
      </div>
      {open.has(q.id) && <div className="space-y-2 border-t px-4 py-3 text-sm"><p className="whitespace-pre-wrap">{q.answer || "—"}</p>{from && <p className="text-xs text-muted-foreground">改自：{from}</p>}{practiced(q.id)}</div>}
    </article>;
  };
  return <div className="flex min-h-0 flex-1 flex-col gap-4 pt-4">
    <div className="flex flex-wrap items-center gap-2">
      <div className="w-64"><SearchInput aria-label="搜索题目" placeholder="搜索" value={query} onChange={(e) => setQuery(e.target.value)} /></div>
      <Dropdown ariaLabel="题目类别" value={category} options={[ALL, ...RATER_CATEGORIES].map((c) => ({ value: c, label: c }))} onChange={setCategory} />
      {tags.length > 0 && <Dropdown ariaLabel="标签" value={tag} options={[{ value: ALL, label: "全部标签" }, ...tags.map((t) => ({ value: t, label: t }))]} onChange={setTag} />}
      <span className="flex-1" />
      <Button disabled={p.busy || !p.model} onClick={() => setAsking(true)}><Sparkles size={14} />让小拾补题</Button>
      <Button disabled={p.busy} onClick={() => setEditing(blank(category === ALL ? "简历题" : category))}><Plus size={14} />题目</Button>
    </div>
    <div className="-m-1 min-h-0 flex-1 space-y-6 overflow-y-auto p-1 pb-6">
      {!shown.length ? <EmptyState title={questions.length ? "没有找到" : "还没有题目"} /> : RATER_CATEGORIES.filter((c) => category === ALL || c === category).map((c) => {
        const rows = shown.filter((q) => q.category === c);
        if (!rows.length) return null;
        const groups = [...new Set(rows.map(groupOf))];
        return <section key={c} className="space-y-2">
          <h3 className="text-sm font-bold">{c}<span className="ml-2 font-normal text-muted-foreground">{rows.length}</span></h3>
          {groups.map((g) => <div key={g} className="space-y-2">{g && <p className="px-1 pt-1 text-xs text-muted-foreground">{g}</p>}{rows.filter((q) => groupOf(q) === g).map(card)}</div>)}
        </section>;
      })}
    </div>
    {editing && <QuestionEditor initial={editing} questions={questions} materials={p.materials} busy={p.busy} write={p.write} close={() => setEditing(null)} />}
    {past && <QuestionHistory id={past} bank={p.bank} busy={p.busy} edit={(q) => { setEditing(q); setPast(null); }} close={() => setPast(null)} />}
    {asking && <SuggestDialog seeker={p.seeker} materials={p.materials} bank={p.bank} start={category === ALL ? "简历题" : category} busy={p.busy} write={p.write} close={() => setAsking(false)} />}
    {polish?.before && polish.after && <Modal wide title="润色答案" error={null} onClose={() => setPolish(null)} submitLabel="接受修改" submitDisabled={p.busy} onSubmit={(e) => { e.preventDefault(); void p.write(() => raterApi.save(polish.after!)).then((ok) => { if (ok) setPolish(null); }); }}>
      <div className="max-h-[60vh] space-y-3 overflow-auto"><p className="text-xs text-muted-foreground">{[polish.model, polish.note].filter(Boolean).join(" · ")}</p>{polish.advice && <p className="whitespace-pre-wrap text-sm">{polish.advice}</p>}<p className="font-medium">{polish.before.question}</p><SeekerDiff before={polish.before.answer} after={polish.after.answer} /></div>
    </Modal>}
    {confirmDialog}
  </div>;
}

/** 让小拾补题: which SKILL, what about, how many → the proposals to tick → 写进题库 (one undo step). */
function SuggestDialog(props: { seeker: SeekerWorkspace; materials: SeekerRecord[]; bank: RaterBank; start: string; busy: boolean; write: BankProps["write"]; close: () => void }) {
  const jobs = useJobs();
  const [kind, setKind] = useState(props.start);
  const [materials, setMaterials] = useState<string[]>([]);
  const [job, setJob] = useState<number | null>(null);
  const [count, setCount] = useState("");
  const [request, setRequest] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<RaterProposal | null>(null);
  const [off, setOff] = useState<Set<string>>(new Set());
  // 简历题 by default: the experiences in the Chinese draft (as Rust picks when none are chosen)
  const inDraft = new Set((props.seeker.draft?.sections ?? []).flatMap((s) => s.entries).map((e) => e.source?.recordId).filter(Boolean));
  const experiences = props.materials.filter((m) => m.category !== "技能" && m.category !== "教育");
  const run = async () => {
    setRunning(true); setError(null);
    const ask: RaterAsk = { kind, materials: kind === "简历题" ? materials : [], job: kind === "突击题" ? job : null, count: Number(count) || 0, question: null, request };
    try { setResult(await raterApi.suggest(ask)); setOff(new Set()); }
    catch (e) { setError(errorText(e)); }
    finally { setRunning(false); }
  };
  const name = (id: string | null) => props.materials.find((m) => m.id === id)?.title;
  const from = (id: string | null) => props.bank.questions.find((q) => q.id === id)?.question;
  const ticked = result?.questions.filter((q) => !off.has(q.id)) ?? [];
  if (result) return <Modal wide title="小拾出的题" error={error} onClose={props.close} submitLabel={`写进题库（${ticked.length} 道）`} submitDisabled={props.busy || !ticked.length} footerLeft={<Button disabled={props.busy} onClick={() => setResult(null)}>返回</Button>} onSubmit={(e) => { e.preventDefault(); void props.write(() => raterApi.add(ticked)).then((ok) => (ok ? props.close() : setError("没写进去，请重试"))); }}>
    <div className="-m-1 max-h-[62vh] space-y-3 overflow-y-auto p-1">
      <p className="text-xs text-muted-foreground">{[result.model, result.note].filter(Boolean).join(" · ")}</p>
      {result.advice && <p className="whitespace-pre-wrap text-sm">{result.advice}</p>}
      {result.questions.map((q) => <label key={q.id} className="flex items-start gap-2 rounded-control border p-2 text-sm">
        <input type="checkbox" className="mt-1 size-4 shrink-0 accent-[var(--primary)]" checked={!off.has(q.id)} onChange={() => setOff((s) => { const n = new Set(s); if (n.has(q.id)) n.delete(q.id); else n.add(q.id); return n; })} />
        <span className="min-w-0 flex-1 space-y-1"><span className="block font-medium">{q.question}</span><span className="block text-xs text-muted-foreground">{[name(q.material), q.job?.label, from(q.from) && `改自：${from(q.from)}`, q.tags.join("、")].filter(Boolean).join(" · ")}</span><span className="block whitespace-pre-wrap">{q.answer || "（没写答案）"}</span></span>
      </label>)}
    </div>
  </Modal>;
  return <Modal wide title="让小拾补题" error={error} onClose={() => { if (!running) props.close(); }} submitLabel={running ? "正在出题…" : "出题"} submitDisabled={running || (kind === "突击题" && job == null)} onSubmit={(e) => { e.preventDefault(); void run(); }}>
    <fieldset disabled={running} className="-m-1 max-h-[62vh] space-y-3 overflow-y-auto p-1">
      <Segmented<string> ariaLabel="补哪一类" value={kind} onChange={setKind} options={RATER_CATEGORIES.map((c) => ({ value: c, label: c }))} />
      {kind === "简历题" && <Field label="经历"><div className="grid gap-1 sm:grid-cols-2">{experiences.map((m) => <label key={m.id} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={materials.includes(m.id)} onChange={() => setMaterials((s) => (s.includes(m.id) ? s.filter((x) => x !== m.id) : [...s, m.id]))} /><span className="truncate">{m.title}</span>{inDraft.has(m.id) && <span className="shrink-0 text-xs text-muted-foreground">简历上</span>}</label>)}{!experiences.length && <p className="text-sm text-muted-foreground">还没有中文素材</p>}</div></Field>}
      {kind === "突击题" && <Field label="岗位"><Dropdown<number | null> ariaLabel="岗位" value={job} placeholder="选择岗位" options={(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))} onChange={setJob} /></Field>}
      <div className="grid grid-cols-[8rem_1fr] gap-3">
        <Field label="出几道"><TextInput inputMode="numeric" value={count} placeholder="默认" onChange={(e) => setCount(e.target.value.replace(/\D/g, "").slice(0, 2))} /></Field>
        <Field label="本次要求"><TextInput value={request} onChange={(e) => setRequest(e.target.value)} /></Field>
      </div>
    </fieldset>
  </Modal>;
}

/** What a question looks like in a diff. */
function questionText(q: RaterQuestion | null | undefined, name: (id: string | null) => string | undefined, from: (id: string | null) => string | undefined) {
  if (!q) return "";
  return [`【${q.category}】${q.question}`, q.category === "简历题" && q.material ? `素材：${name(q.material) ?? "已删除"}` : "", q.category === "突击题" && q.job ? `岗位：${q.job.label}` : "", q.from ? `改自：${from(q.from) ?? "已删除"}` : "", q.tags.length ? `标签：${q.tags.join("、")}` : "", "", q.answer].filter((l, i) => l || i === 5).join("\n");
}

function QuestionEditor(props: { initial: RaterQuestion; questions: RaterQuestion[]; materials: SeekerRecord[]; busy: boolean; write: BankProps["write"]; close: () => void }) {
  const jobs = useJobs();
  const [v, setV] = useState(props.initial);
  const [tagText, setTagText] = useState(props.initial.tags.join("、"));
  const [review, setReview] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [seconds, setSeconds] = useState<number | null>(null);
  const set = (patch: Partial<RaterQuestion>) => setV((x) => ({ ...x, ...patch }));
  // the length as the bank will show it (Rust's rule), a moment after typing stops
  useEffect(() => {
    if (!v.answer.trim()) { setSeconds(null); return; }
    const timer = window.setTimeout(() => void raterApi.seconds(v.answer).then(setSeconds, () => setSeconds(null)), 250);
    return () => window.clearTimeout(timer);
  }, [v.answer]);
  const value = { ...v, tags: splitTags(tagText) };
  const name = (id: string | null) => props.materials.find((m) => m.id === id)?.title;
  const from = (id: string | null) => props.questions.find((q) => q.id === id)?.question;
  const job = jobs.data?.find((j) => v.job && j.id === v.job.id && j.createdAt === v.job.createdAt);
  return <Modal wide title={review ? "确认修改" : props.initial.revision ? "编辑题目" : "新增题目"} error={error} onClose={props.close} submitDisabled={props.busy || !v.question.trim()} submitLabel={review ? "确认保存" : "查看差异"} footerLeft={review ? <Button disabled={props.busy} onClick={() => setReview(false)}>返回编辑</Button> : undefined} onSubmit={(e) => {
    e.preventDefault();
    if (!review) { setReview(true); return; }
    // only what the category uses goes with it
    const out: RaterQuestion = { ...value, material: value.category === "简历题" ? value.material : null, job: value.category === "突击题" ? value.job : null };
    void props.write(() => raterApi.save(out)).then((ok) => (ok ? props.close() : setError("没存上，请重试或重新打开这道题")));
  }}>
    {review ? <SeekerDiff before={props.initial.revision ? questionText(props.initial, name, from) : ""} after={questionText(value, name, from)} /> : <fieldset disabled={props.busy} className="-m-1 max-h-[62vh] space-y-3 overflow-y-auto p-1">
      <div className="grid grid-cols-[8rem_1fr] gap-3">
        <Field label="类别"><Dropdown ariaLabel="类别" value={v.category} options={RATER_CATEGORIES.map((c) => ({ value: c, label: c }))} onChange={(category) => set({ category })} /></Field>
        <Field label="问题"><TextInput autoFocus value={v.question} onChange={(e) => set({ question: e.target.value })} /></Field>
      </div>
      {v.category === "简历题" && <Field label="素材"><Dropdown<string | null> ariaLabel="素材" value={v.material} placeholder="不挂" options={[{ value: null, label: "不挂" }, ...props.materials.map((m) => ({ value: m.id, label: `${m.category} · ${m.title}` }))]} onChange={(material) => set({ material })} /></Field>}
      {v.category === "突击题" && <Field label="岗位"><Dropdown<number | null> ariaLabel="岗位" value={job?.id ?? null} placeholder={v.job?.label ?? "不关联"} options={[{ value: null, label: "不关联" }, ...(jobs.data ?? []).map((j) => ({ value: j.id, label: jobLabel(j) }))]} onChange={(id) => { const j = jobs.data?.find((x) => x.id === id); set({ job: j ? { id: j.id, createdAt: j.createdAt, label: jobLabel(j) } : null }); }} /></Field>}
      <Field label="答案" action={seconds != null ? <span className={`text-xs ${seconds > RATER_LIMIT ? "text-primary" : "text-muted-foreground"}`}>约 {seconds} 秒</span> : undefined}><TextArea rows={10} value={v.answer} onChange={(e) => set({ answer: e.target.value })} /></Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="标签"><TextInput aria-label="标签" value={tagText} onChange={(e) => setTagText(e.target.value)} /></Field>
        <Field label="改自"><Dropdown<string | null> ariaLabel="改自" value={v.from} placeholder="不是改写的" options={[{ value: null, label: "不是改写的" }, ...props.questions.filter((q) => q.id !== v.id).map((q) => ({ value: q.id, label: `${q.category} · ${q.question}` }))]} onChange={(f) => set({ from: f })} /></Field>
      </div>
    </fieldset>}
  </Modal>;
}

function QuestionHistory(props: { id: string; bank: RaterBank; busy: boolean; edit: (q: RaterQuestion) => void; close: () => void }) {
  const current = props.bank.questions.find((q) => q.id === props.id);
  const past = props.bank.revisions.filter((r) => r.questionId === props.id);
  const text = (q: RaterQuestion | undefined) => (q ? [q.question, q.tags.length ? `标签：${q.tags.join("、")}` : "", "", q.answer].filter((l, i) => l || i === 2).join("\n") : "");
  return <Modal wide title="历史" error={null} onClose={props.close} onSubmit={(e) => { e.preventDefault(); props.close(); }} submitLabel="完成">
    <div className="-m-1 max-h-[58vh] space-y-5 overflow-y-auto p-1">
      {past.map((r) => <div key={r.id} className="space-y-2"><div className="flex items-center gap-2"><span className="mr-auto text-xs text-muted-foreground">{when(r.savedAt)}</span>{current && <Button variant="ghost" disabled={props.busy} onClick={() => props.edit({ ...r.body, revision: current.revision })}>以这一版编辑</Button>}</div><SeekerDiff before={text(r.body)} after={text(current)} /></div>)}
      {!past.length && <p className="text-sm text-muted-foreground">还没有历史</p>}
    </div>
  </Modal>;
}
