// Rater 卡片 (v0.7 阶段 ⑥; Rust picky/assistants/rater/practice.rs): start a round (自己挑 / 按 JD /
// 随机), practice one big card at a time (click to flip; 不熟 / 会; typing is optional; 追问 puts a
// dashed follow-up card next), then the round's feedback (the program lists 不熟 / 会 / 追问; 小拾
// comments when something was typed or followed up). Every write is one undo step in Rust; after ⌘Z
// the App re-reads every query, so the page follows the round as Rust has it.
import { PickField } from "@/shared/PickField";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, Keyboard, Plus, Sparkles, Trash2, X } from "lucide-react";
import { useJobs, jobPicks } from "@/modules/jobs/api";
import { Dropdown } from "@/shared/Dropdown";
import { errorText, useConfirm } from "@/shared/feedback";
import { Button, EmptyState, IconButton, Segmented, Switch, TextArea, TextInput } from "@/shared/ui";
import { when } from "./SeekerParts";
import { PickRow, SelectBar, SelectButton, useSelect } from "./select";
import type { SeekerRecord } from "./seekerApi";
import { RATER_CATEGORIES, RATER_FOLLOW_MAX, RATER_KEY, RATER_KNOW, RATER_MODES, RATER_PRACTICE_KEY, RATER_RANDOM_COUNT, RATER_UNSURE, raterApi, wantsComment, type RaterBank, type RaterCard, type RaterPick, type RaterPractice, type RaterQuestion, type RaterRound, type RaterTally } from "./raterApi";

type Guarded = (action: () => Promise<void>) => Promise<boolean>;
type Props = { bank: RaterBank; materials: SeekerRecord[]; model: string | null; busy: boolean; guarded: Guarded };
const ALL = "全部";

/** 不熟 last time / 会 twice in a row / never: the words the page shows. */
function tallyText(t: RaterTally | undefined) {
  if (!t) return "没练过";
  if (t.lastMark === RATER_UNSURE) return "不熟";
  return t.streak >= 2 ? `会 ${t.know} 次 · 连着会了` : `会 ${t.know} 次`;
}

export function RaterCards(p: Props) {
  const qc = useQueryClient();
  const practice = useQuery({ queryKey: RATER_PRACTICE_KEY, queryFn: raterApi.practice });
  // the ended round on screen (and whether it just ended here: then 小拾 comments by itself)
  const [viewing, setViewing] = useState<{ id: string; fresh: boolean } | null>(null);
  /** A practice write: the new state, the bank's records re-read; `after` sees the result. */
  const write = (action: () => Promise<RaterPractice>, after?: (out: RaterPractice) => void) => p.guarded(async () => {
    const out = await action();
    qc.setQueryData(RATER_PRACTICE_KEY, out);
    void qc.invalidateQueries({ queryKey: RATER_KEY, exact: true });
    after?.(out);
  });
  // a round started (here, or back after ⌘Z): the feedback on screen before it is left behind
  const runningId = practice.data?.running?.id;
  useEffect(() => { if (runningId) setViewing(null); }, [runningId]);
  if (practice.isError) return <EmptyState title="卡片暂时打不开" hint={errorText(practice.error)} action={<Button onClick={() => void practice.refetch()}>重试</Button>} />;
  if (!practice.data) return <EmptyState title="正在读取…" />;
  const running = practice.data.running;
  // a round that ended (last card, or 停下 with something done) opens its feedback
  const ended = (id: string) => (out: RaterPractice) => { if (!out.running && out.rounds.some((r) => r.id === id)) setViewing({ id, fresh: true }); };
  if (running) return <Practice key={running.id} round={running} {...p} write={write} ended={ended(running.id)} />;
  if (viewing) return <RoundView key={viewing.id} id={viewing.id} fresh={viewing.fresh} {...p} write={write} back={() => setViewing(null)} />;
  return <StartRound {...p} practice={practice.data} write={write} open={(id) => setViewing({ id, fresh: false })} />;
}

type Write = (action: () => Promise<RaterPractice>, after?: (out: RaterPractice) => void) => Promise<boolean>;

/** How a round starts: 自己挑 (filters and ticks), 按 JD (小拾 picks, the user removes / adds), 随机 (N). */
function StartRound(p: Props & { practice: RaterPractice; write: Write; open: (id: string) => void }) {
  const jobs = useJobs();
  const [mode, setMode] = useState<string>(RATER_MODES[0]);
  const [category, setCategory] = useState(ALL);
  const [tag, setTag] = useState(ALL);
  const [unsureOnly, setUnsureOnly] = useState(false);
  const [off, setOff] = useState<Set<string>>(new Set());
  const [job, setJob] = useState<number | null>(null);
  const [pick, setPick] = useState<RaterPick | null>(null);
  const [picking, setPicking] = useState(false);
  const [count, setCount] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const questions = p.bank.questions;
  const tally = (id: string) => p.bank.practice?.[id];
  const tags = [...new Set(questions.flatMap((q) => q.tags))];
  const listed = questions.filter((q) => (category === ALL || q.category === category) && (tag === ALL || q.tags.includes(tag)) && (!unsureOnly || tally(q.id)?.lastMark === RATER_UNSURE));
  const ticked = listed.filter((q) => !off.has(q.id));
  const chosen = mode === "自己挑" ? ticked.map((q) => q.id) : mode === "按 JD" ? pick?.questions.filter((id) => questions.some((q) => q.id === id)) ?? [] : [];
  const size = mode === "随机" ? Math.min(Number(count) || RATER_RANDOM_COUNT, questions.length) : chosen.length;
  const group = (q: RaterQuestion) => (q.category === "简历题" ? p.materials.find((m) => m.id === q.material)?.title : q.category === "突击题" ? q.job?.label : undefined);
  const runPick = async () => {
    if (job == null) return;
    setPicking(true); setError(null);
    try { setPick(await raterApi.pick(job)); }
    catch (e) { setError(errorText(e)); }
    finally { setPicking(false); }
  };
  const start = () => {
    setError(null);
    void p.write(() => raterApi.start({ mode, job: mode === "按 JD" ? job : null, questions: chosen, count: mode === "随机" ? Number(count) || 0 : 0, advice: mode === "按 JD" ? pick?.advice ?? "" : "" }));
  };
  const removeRound = async (id: string) => {
    if (await confirm({ title: "删除这一轮？", message: "这一轮的会 / 不熟也从练习记录里去掉。", danger: true, confirmLabel: "删除" })) void p.write(() => raterApi.removeRound(id));
  };
  const rounds = useSelect(p.practice.rounds.map((r) => r.id));
  const removeRounds = async () => {
    if (await confirm({ title: `删除 ${rounds.count} 轮？`, message: "这几轮的会 / 不熟也从练习记录里去掉。", danger: true, confirmLabel: "删除" })) void p.write(() => raterApi.removeRounds([...rounds.picked]), () => rounds.exit());
  };
  const toggle = (id: string) => setOff((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const row = (q: RaterQuestion, right: ReactNode) => <div key={q.id} className="flex min-w-0 items-center gap-2 rounded-control px-2 py-1.5 text-sm hover:bg-muted/60">
    {right}
  </div>;
  const meta = (q: RaterQuestion) => {
    const t = tally(q.id);
    return <span className="shrink-0 text-xs text-muted-foreground">{[q.category, group(q)].filter(Boolean).join(" · ")} · <span className={t?.lastMark === RATER_UNSURE ? "text-primary" : ""}>{tallyText(t)}</span></span>;
  };
  if (!questions.length) return <EmptyState title="题库还是空的" />;
  return <div className="-m-1 min-h-0 flex-1 space-y-8 overflow-y-auto p-1 pt-5 pb-6">
    <section className="mx-auto max-w-3xl space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <Segmented<string> ariaLabel="出卡方式" value={mode} onChange={(m) => { setMode(m); setError(null); }} options={RATER_MODES.map((m) => ({ value: m, label: m }))} />
        <span className="flex-1" />
        <Button variant="primary" disabled={p.busy || picking || !size} onClick={start}>开始（{size} 张）</Button>
      </div>
      {mode === "自己挑" && <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-2">
          <Dropdown ariaLabel="题目类别" value={category} options={[ALL, ...RATER_CATEGORIES].map((c) => ({ value: c, label: c === ALL ? "全部类别" : c }))} onChange={setCategory} />
          {tags.length > 0 && <Dropdown ariaLabel="标签" value={tag} options={[{ value: ALL, label: "全部标签" }, ...tags.map((t) => ({ value: t, label: t }))]} onChange={setTag} />}
          <label className="flex items-center gap-2 text-sm"><Switch label="只要不熟的" checked={unsureOnly} onChange={setUnsureOnly} />只要不熟的</label>
          <span className="flex-1" />
          {listed.length > 0 && <Button variant="ghost" onClick={() => setOff((s) => { const n = new Set(s); const all = ticked.length === listed.length; for (const q of listed) { if (all) n.add(q.id); else n.delete(q.id); } return n; })}>{ticked.length === listed.length ? "全不选" : "全选"}</Button>}
        </div>
        <div className="space-y-0.5 rounded-card border bg-card p-1">
          {listed.map((q) => <PickRow key={q.id} on={!off.has(q.id)} onClick={() => toggle(q.id)} className="items-center gap-2 px-2 py-1.5"><span className="min-w-0 flex-1 truncate">{q.question}</span>{meta(q)}</PickRow>)}
          {!listed.length && <p className="px-2 py-3 text-sm text-muted-foreground">没有符合的题</p>}
        </div>
      </div>}
      {mode === "按 JD" && <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <PickField<number | null> ariaLabel="岗位" title="按哪个岗位挑卡" value={job} placeholder="选择岗位" options={jobPicks(jobs.data ?? [])} onChange={(j) => { setJob(j); setPick(null); }} />
          <Button disabled={p.busy || picking || job == null || !p.model} onClick={() => void runPick()}><Sparkles size={14} />{picking ? "正在挑…" : pick ? "重新挑" : "让小拾挑"}</Button>
        </div>
        {pick && <div className="space-y-2">
          <p className="text-xs text-muted-foreground">{[pick.model, pick.note].filter(Boolean).join(" · ")}</p>
          {pick.advice && <p className="whitespace-pre-wrap text-sm">{pick.advice}</p>}
          <div className="rounded-card border bg-card p-1">
            {chosen.map((id) => { const q = questions.find((x) => x.id === id)!; return row(q, <><span className="min-w-0 flex-1 truncate">{q.question}</span>{meta(q)}<IconButton label="去掉" onClick={() => setPick({ ...pick, questions: pick.questions.filter((x) => x !== id) })}><X size={14} /></IconButton></>); })}
            {!chosen.length && <p className="px-2 py-3 text-sm text-muted-foreground">一道都没留</p>}
          </div>
          <PickField<string | null> ariaLabel="加一道" title="加一道题" value={null} trigger={<span className="inline-flex items-center gap-1 text-sm text-primary"><Plus size={14} />加一道</span>} options={questions.filter((q) => !chosen.includes(q.id)).map((q) => ({ value: q.id, label: q.question, group: q.category }))} onChange={(id) => { if (id) setPick({ ...pick, questions: [...pick.questions, id] }); }} />
        </div>}
      </div>}
      {mode === "随机" && <div className="flex items-center gap-2 text-sm"><span>几张</span><div className="w-20"><TextInput aria-label="几张" inputMode="numeric" value={count} placeholder={String(RATER_RANDOM_COUNT)} onChange={(e) => setCount(e.target.value.replace(/\D/g, "").slice(0, 2))} /></div></div>}
      {error && <p className="text-sm text-destructive">{error}</p>}
    </section>
    {p.practice.rounds.length > 0 && <section className="mx-auto max-w-3xl space-y-2">
      <div className="flex h-8 items-center gap-2"><h3 className="mr-auto text-sm font-bold">最近几轮</h3><SelectButton select={rounds} /></div>
      <SelectBar select={rounds}><Button variant="ghost-danger" disabled={p.busy || !rounds.count} onClick={() => void removeRounds()}><Trash2 size={14} />删除</Button></SelectBar>
      <div className="space-y-0.5 rounded-card border bg-card p-1">
        {p.practice.rounds.map((r) => <div key={r.id} className={`group flex items-center gap-2 rounded-control border text-sm ${rounds.has(r.id) ? "border-primary bg-accent text-accent-foreground" : "border-transparent hover:bg-muted/60"}`}>
          <button type="button" role={rounds.on ? "checkbox" : undefined} aria-checked={rounds.on ? rounds.has(r.id) : undefined} onClick={rounds.on ? rounds.pick(r.id) : () => p.open(r.id)} className="flex min-w-0 flex-1 items-center gap-3 rounded-control px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <span className="shrink-0 text-muted-foreground">{when(r.startedAt)}</span>
            <span className="min-w-0 flex-1 truncate">{[r.mode, r.job].filter(Boolean).join(" · ")}</span>
            <span className="shrink-0 text-xs text-muted-foreground">{r.cards} 张 · 会 {r.know} · <span className={r.unsure ? "text-primary" : ""}>不熟 {r.unsure}</span>{r.follows > 0 && ` · 追问 ${r.follows}`}{r.left > 0 && ` · 没练 ${r.left}`}</span>
            <ChevronRight size={14} className="shrink-0 text-muted-foreground" />
          </button>
          <div className={`${rounds.on ? "hidden" : ""} pr-1 opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-within:opacity-100`}><IconButton label="删除这一轮" danger disabled={p.busy} onClick={() => void removeRound(r.id)}><Trash2 size={14} /></IconButton></div>
        </div>)}
      </div>
    </section>}
    {confirmDialog}
  </div>;
}

/** What a card says on its front's corner: 类别 · 素材 / 岗位, or 追问. */
function cardLabel(card: RaterCard, bank: RaterBank, materials: SeekerRecord[]) {
  if (card.follow) return "追问";
  const q = bank.questions.find((x) => x.id === card.question);
  const group = q?.category === "简历题" ? materials.find((m) => m.id === q.material)?.title : q?.category === "突击题" ? q.job?.label : undefined;
  return [card.category, group].filter(Boolean).join(" · ");
}

/** The running round: one big card. */
function Practice(p: Props & { round: RaterRound; write: Write; ended: (out: RaterPractice) => void }) {
  const qc = useQueryClient();
  const r = p.round;
  const index = r.cards.findIndex((c) => !c.mark);
  const card = r.cards[index];
  const [flipped, setFlipped] = useState(false);
  const [typing, setTyping] = useState(false);
  const [typed, setTyped] = useState(card?.typed ?? "");
  const [following, setFollowing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // a new card (marked, or back one after ⌘Z): front side, what was typed on it
  useEffect(() => { setFlipped(false); setTyped(r.cards[index]?.typed ?? ""); setError(null); }, [r.id, index]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!card) return <EmptyState title="这一轮练完了" />;
  const q = p.bank.questions.find((x) => x.id === card.question);
  const follows = r.cards.filter((c) => c.follow && c.question === card.question).length;
  const canFollow = !card.asked && follows < RATER_FOLLOW_MAX;
  const mark = (m: string) => { setError(null); void p.write(() => raterApi.mark(r.id, index, m, typing ? typed : ""), p.ended); };
  const follow = async () => {
    setFollowing(true); setError(null);
    try { qc.setQueryData(RATER_PRACTICE_KEY, await raterApi.follow(r.id, index, typing ? typed : "")); }
    catch (e) { setError(errorText(e)); }
    finally { setFollowing(false); }
  };
  const keep = () => void p.guarded(async () => { await raterApi.keep(r.id, index); await qc.invalidateQueries({ queryKey: RATER_KEY }); });
  const answer = card.follow ? card.reference : q ? q.answer : "";
  return <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pt-5 pb-6">
    <div className="mx-auto max-w-2xl space-y-4">
      <div className="flex items-center gap-3 text-sm">
        <span className="min-w-0 flex-1 truncate text-muted-foreground">{[r.mode, r.job?.label].filter(Boolean).join(" · ")}</span>
        <span className="shrink-0 tabular-nums text-muted-foreground">{index + 1} / {r.cards.length}</span>
        <Button disabled={p.busy || following} onClick={() => void p.write(() => raterApi.stop(r.id), p.ended)}>停下</Button>
      </div>
      <button type="button" aria-label={flipped ? "翻回问题" : "翻面看答案"} onClick={() => setFlipped((f) => !f)} className="rater-flip block w-full rounded-card text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <div className={`rater-flip-inner ${flipped ? "flipped" : ""}`}>
          <div className={`rater-flip-face flex min-h-64 flex-col rounded-card border bg-card p-6 shadow-sm ${card.follow ? "border-[1.5px] border-dashed" : ""}`}>
            <span className="text-xs text-muted-foreground">{cardLabel(card, p.bank, p.materials)}</span>
            <span className="flex flex-1 items-center justify-center py-6 text-center text-xl font-medium leading-relaxed">{card.text}</span>
          </div>
          <div className={`rater-flip-face rater-flip-back flex min-h-64 flex-col gap-3 rounded-card border bg-card p-6 shadow-sm ${card.follow ? "border-[1.5px] border-dashed" : ""}`}>
            <span className="text-xs text-muted-foreground">{card.text}</span>
            {card.follow && <span className="text-xs text-muted-foreground">小拾的参考答案</span>}
            <span className="whitespace-pre-wrap leading-relaxed">{q || card.follow ? answer.trim() || "还没写答案" : "这道题已经删除了"}</span>
          </div>
        </div>
      </button>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" aria-pressed={typing} className={typing ? "text-foreground" : "text-muted-foreground"} onClick={() => setTyping((t) => !t)}><Keyboard size={14} />打字</Button>
        {card.follow && (card.saved ? <span className="text-sm text-muted-foreground">已存进题库</span> : <Button variant="ghost" disabled={p.busy} onClick={keep}><Plus size={14} />存进题库</Button>)}
        <span className="flex-1" />
        {flipped && <>
          <Button disabled={p.busy || following || !canFollow || !p.model} onClick={() => void follow()}><Sparkles size={14} />{following ? "正在追问…" : card.asked ? "已追问" : "追问"}</Button>
          <Button disabled={p.busy || following} onClick={() => mark(RATER_UNSURE)}>不熟</Button>
          <Button variant="primary" disabled={p.busy || following} onClick={() => mark(RATER_KNOW)}>会</Button>
        </>}
      </div>
      {typing && <TextArea aria-label="打字" rows={5} value={typed} onChange={(e) => setTyped(e.target.value)} />}
      {error && <p className="text-sm text-destructive">{error}</p>}
    </div>
  </div>;
}

/** An ended round: the counts, 小拾's comment (when wanted), 不熟 / 会, the follow-ups. */
function RoundView(p: Props & { id: string; fresh: boolean; write: Write; back: () => void }) {
  const qc = useQueryClient();
  const key = [...RATER_PRACTICE_KEY, p.id];
  const round = useQuery({ queryKey: key, queryFn: () => raterApi.round(p.id) });
  const [commenting, setCommenting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, confirmDialog] = useConfirm();
  const tried = useRef(false);
  const r = round.data;
  const comment = async () => {
    setCommenting(true); setError(null);
    try { qc.setQueryData(key, await raterApi.comment(p.id)); }
    catch (e) { setError(errorText(e)); }
    finally { setCommenting(false); }
  };
  // a round that just ended here: 小拾 comments by itself, once
  useEffect(() => {
    if (!r || tried.current || !p.fresh || !p.model || r.feedback || !wantsComment(r)) return;
    tried.current = true;
    void comment();
  }, [r, p.model]); // eslint-disable-line react-hooks/exhaustive-deps
  const lists = useMemo(() => {
    const cards = (r?.cards ?? []).map((c, i) => ({ c, i }));
    return { unsure: cards.filter(({ c }) => !c.follow && c.mark === RATER_UNSURE), know: cards.filter(({ c }) => !c.follow && c.mark === RATER_KNOW), follows: cards.filter(({ c }) => c.follow) };
  }, [r]);
  if (round.isError) return <EmptyState title="这一轮打不开了" hint={errorText(round.error)} action={<Button onClick={p.back}>返回</Button>} />;
  if (!r) return <EmptyState title="正在读取…" />;
  const left = r.cards.filter((c) => !c.mark).length;
  const again = [...new Set(lists.unsure.map(({ c }) => c.question))].filter((id) => p.bank.questions.some((q) => q.id === id));
  const note = (i: number) => r.feedback?.notes.find((n) => n.card === i)?.text;
  const keep = (i: number) => void p.guarded(async () => { qc.setQueryData(key, await raterApi.keep(r.id, i)); await qc.invalidateQueries({ queryKey: RATER_KEY, exact: true }); });
  const remove = async () => {
    if (await confirm({ title: "删除这一轮？", message: "这一轮的会 / 不熟也从练习记录里去掉。", danger: true, confirmLabel: "删除" })) void p.write(() => raterApi.removeRound(r.id), () => p.back());
  };
  const line = ({ c, i }: { c: RaterCard; i: number }) => <li key={i} className={`space-y-1 rounded-control border bg-card px-3 py-2 text-sm ${c.follow ? "border-[1.5px] border-dashed" : ""}`}>
    <div className="flex items-start gap-2"><span className="min-w-0 flex-1 font-medium">{c.text}</span>{c.follow && <span className={`shrink-0 text-xs ${c.mark === RATER_UNSURE ? "text-primary" : "text-muted-foreground"}`}>{c.mark ?? "没练到"}</span>}</div>
    {c.follow && <p className="whitespace-pre-wrap text-muted-foreground">{c.reference}</p>}
    {c.typed.trim() && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">我说的：</span>{c.typed}</p>}
    {note(i) && <p className="whitespace-pre-wrap"><span className="text-muted-foreground">小拾：</span>{note(i)}</p>}
    {c.follow && <div>{c.saved ? <span className="text-xs text-muted-foreground">已存进题库</span> : <Button variant="ghost" className="-ml-2" disabled={p.busy} onClick={() => keep(i)}><Plus size={14} />存进题库</Button>}</div>}
  </li>;
  const section = (title: string, rows: { c: RaterCard; i: number }[], tone = "") => rows.length > 0 && <section className="space-y-2">
    <h3 className={`text-sm font-bold ${tone}`}>{title}<span className="ml-2 font-normal text-muted-foreground">{rows.length}</span></h3>
    <ul className="space-y-2">{rows.map(line)}</ul>
  </section>;
  const fb = r.feedback;
  return <div className="-m-1 min-h-0 flex-1 overflow-y-auto p-1 pt-4 pb-6">
    <div className="mx-auto max-w-3xl space-y-6">
      <div className="flex items-center gap-2">
        <IconButton label="返回" onClick={p.back}><ChevronLeft size={16} /></IconButton>
        <span className="min-w-0 flex-1 truncate text-sm"><span className="text-muted-foreground">{when(r.startedAt)}</span> · {[r.mode, r.job?.label].filter(Boolean).join(" · ")}</span>
        <IconButton label="删除这一轮" danger disabled={p.busy} onClick={() => void remove()}><Trash2 size={14} /></IconButton>
      </div>
      <p className="text-sm text-muted-foreground">{r.cards.filter((c) => !c.follow).length} 张 · 会 {lists.know.length} · 不熟 {lists.unsure.length}{lists.follows.length > 0 && ` · 追问 ${lists.follows.length}`}{left > 0 && ` · 还剩 ${left} 张没练`}</p>
      {wantsComment(r) && <section className="space-y-2 rounded-card border bg-card px-4 py-3 text-sm">
        <h3 className="flex items-center gap-1.5 font-bold"><Sparkles size={14} className="text-[var(--module-picky)]" />小拾的点评{fb?.model && <span className="font-normal text-xs text-muted-foreground">{fb.model}</span>}</h3>
        {fb ? <>
          {fb.summary && <p className="whitespace-pre-wrap">{fb.summary}</p>}
          {fb.good.length > 0 && <div><p className="text-xs text-muted-foreground">说得好</p><ul className="list-disc space-y-0.5 pl-5">{fb.good.map((g, i) => <li key={i}>{g}</li>)}</ul></div>}
          {fb.gaps.length > 0 && <div><p className="text-xs text-muted-foreground">讲不清的</p><ul className="list-disc space-y-0.5 pl-5">{fb.gaps.map((g, i) => <li key={i}>{g}</li>)}</ul></div>}
        </> : commenting ? <p className="animate-pulse text-muted-foreground">正在写…</p> : <div className="flex items-center gap-2">{error && <p className="min-w-0 flex-1 text-destructive">{error}</p>}<Button disabled={p.busy || !p.model} onClick={() => void comment()}>{error ? "重试" : "让小拾点评"}</Button></div>}
      </section>}
      {section("不熟", lists.unsure, "text-primary")}
      {section("会", lists.know)}
      {section("追问", lists.follows)}
      {again.length > 0 && <div className="flex justify-end"><Button variant="primary" disabled={p.busy} onClick={() => void p.write(() => raterApi.start({ mode: RATER_MODES[0], job: null, questions: again, count: 0, advice: "" }))}>再练不熟的（{again.length} 张）</Button></div>}
    </div>
    {confirmDialog}
  </div>;
}
