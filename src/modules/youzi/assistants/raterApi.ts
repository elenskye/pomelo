import { invoke } from "@tauri-apps/api/core";

/** Rater's question bank (Rust youzi/assistants/rater.rs). */
import type { SeekerPromptView } from "./seekerApi";
export const RATER_CATEGORIES = ["简历题", "通用题", "突击题"] as const;
/** Rater's rules: Seeker records of these 用途 (the last three: each 题库 SKILL's workflow). */
export const RATER_USES = [{ value: "题库维护", label: "题库维护" }, { value: "卡片练习", label: "卡片练习" }, { value: "面试", label: "面试" }, { value: "复盘", label: "复盘" }, ...RATER_CATEGORIES.map((c) => ({ value: c, label: `${c} SKILL` }))];
/** A rule the program brings until the user writes their own (Rust qbank.rs `Builtin`): whole = any rule of the group stands in for it, else one of the same title. */
export type RaterBuiltin = { category: string; title: string; points: string[]; whole: boolean };
/** Answers longer than this (seconds) are flagged. */
export const RATER_LIMIT = 90;
export type RaterJob = { id: number; createdAt: string; label: string };
export type RaterQuestion = { id: string; category: string; question: string; answer: string; tags: string[]; material: string | null; job: RaterJob | null; from: string | null; revision: number };
export type RaterRevision = { id: number; questionId: string; body: RaterQuestion; savedAt: string };
/** A question's practice record (Rust practice.rs `Tally`): streak = 会 in a row up to the last mark. */
export type RaterTally = { know: number; unsure: number; last: string | null; lastMark: string; streak: number };
/** seconds: each question's answer read aloud (400 字 in 90 s); practice: each practiced question's record. */
export type RaterBank = { questions: RaterQuestion[]; revisions: RaterRevision[]; seconds?: Record<string, number>; practice?: Record<string, RaterTally> };
/** What one 补题 / 润色 request is about (Rust rater/qbank.rs `Ask`); kind 润色 rewrites `question`'s answer. */
export type RaterAsk = { kind: string; materials: string[]; job: number | null; count: number; question: string | null; request: string };
/** New questions (revision 0, checked by Rust) to tick, or one question with its new answer. */
export type RaterProposal = { kind: string; model: string; note: string; advice: string; questions: RaterQuestion[]; before: RaterQuestion | null; after: RaterQuestion | null };
/** Rater's models (not undone): the default, and the functions with their own (Rust qbank.rs `Options`). */
export type RaterChoice = { model: string | null; thinking: boolean; effort?: string | null };
export type RaterOptions = RaterChoice & { features: Record<string, RaterChoice> };
/** Rater's AI functions (设置 page rows; Rust qbank.rs `FEATURES`). */
export const RATER_FEATURES = [["suggest", "补题"], ["polish", "润色答案"], ["pick", "卡片练习 · 按 JD 挑卡"], ["follow", "卡片练习 · 追问"], ["comment", "卡片练习 · 点评"], ["prep", "面试 · 准备"], ["record", "复盘 · 记录"], ["summary", "复盘 · 总结"]] as const;
export type RaterFeature = (typeof RATER_FEATURES)[number][0];
/** Whether a function has a model (its own or the default). */
export type RaterCan = (f: RaterFeature) => boolean;
/** The model and 思考 a function uses. */
export const raterChoice = (o: RaterOptions, f: RaterFeature): RaterChoice => o.features[f] ?? { model: o.model, thinking: o.thinking, effort: o.effort };
export const RATER_POLISH = "润色";
/** 卡片练习 (Rust rater/practice.rs). */
export const RATER_MODES = ["自己挑", "按 JD", "随机"] as const;
export const RATER_KNOW = "会";
export const RATER_UNSURE = "不熟";
export const RATER_FOLLOW_MAX = 2;
export const RATER_RANDOM_COUNT = 10;
/** A card: question = the bank question (a follow-up: the one it follows up); follow-ups are dashed with 柚子's reference answer. */
export type RaterCard = { question: string; text: string; category: string; follow: boolean; reference: string; saved: string | null; mark: string | null; markedAt: string | null; typed: string; asked: boolean };
/** notes: card = index in the round. */
export type RaterFeedback = { model: string; summary: string; good: string[]; gaps: string[]; notes: { card: number; text: string }[] };
export type RaterRound = { id: string; mode: string; job: RaterJob | null; advice: string; cards: RaterCard[]; startedAt: string; endedAt: string | null; feedback: RaterFeedback | null };
export type RaterRoundRow = { id: string; mode: string; job: string | null; startedAt: string; endedAt: string | null; cards: number; follows: number; know: number; unsure: number; left: number };
export type RaterPractice = { running: RaterRound | null; rounds: RaterRoundRow[] };
export type RaterStart = { mode: string; job: number | null; questions: string[]; count: number; advice: string };
export type RaterPick = { questions: string[]; advice: string; model: string; note: string };
/** 柚子 should comment: something typed, or a follow-up (Rust `Round::wants_comment`). */
export const wantsComment = (r: RaterRound) => r.cards.some((c) => c.follow || (c.mark != null && c.typed.trim() !== ""));
export const RATER_PRACTICE_KEY = ["youzi", "rater", "practice"];
export const RATER_KEY = ["youzi", "rater"];
export const RATER_OPTIONS_KEY = ["youzi", "rater", "options"];
export const raterApi = {
  bank: () => invoke<RaterBank>("youzi_rater"),
  save: (question: RaterQuestion) => invoke<RaterBank>("youzi_rater_save", { question }),
  remove: (id: string) => invoke<RaterBank>("youzi_rater_delete", { id }),
  removeMany: (ids: string[]) => invoke<RaterBank>("youzi_rater_delete_many", { ids }),
  /** The user's order for a group of questions. */
  order: (ids: string[]) => invoke<RaterBank>("youzi_rater_order", { ids }),
  removeRounds: (ids: string[]) => invoke<RaterPractice>("youzi_rater_rounds_delete", { ids }),
  seconds: (text: string) => invoke<number>("youzi_rater_seconds", { text }),
  /** 规则页 · 发给模型的说明 (Rust rater.rs `previews`). */
  prompts: () => invoke<SeekerPromptView[]>("youzi_rater_prompts"),
  suggest: (ask: RaterAsk) => invoke<RaterProposal>("youzi_rater_suggest", { ask }),
  add: (questions: RaterQuestion[]) => invoke<RaterBank>("youzi_rater_add", { questions }),
  /** The rules the program brings. */
  defaults: () => invoke<RaterBuiltin[]>("youzi_rater_defaults"),
  options: () => invoke<RaterOptions>("youzi_rater_options"),
  setOptions: (options: RaterOptions) => invoke<RaterOptions>("youzi_rater_options_set", { options }),
  practice: () => invoke<RaterPractice>("youzi_rater_practice"),
  round: (id: string) => invoke<RaterRound>("youzi_rater_round", { id }),
  start: (start: RaterStart) => invoke<RaterPractice>("youzi_rater_round_start", { start }),
  mark: (id: string, card: number, mark: string, typed: string) => invoke<RaterPractice>("youzi_rater_mark", { id, card, mark, typed }),
  stop: (id: string) => invoke<RaterPractice>("youzi_rater_stop", { id }),
  removeRound: (id: string) => invoke<RaterPractice>("youzi_rater_round_delete", { id }),
  pick: (job: number) => invoke<RaterPick>("youzi_rater_pick", { job }),
  follow: (id: string, card: number, typed: string) => invoke<RaterPractice>("youzi_rater_follow", { id, card, typed }),
  keep: (id: string, card: number) => invoke<RaterRound>("youzi_rater_keep", { id, card }),
  comment: (id: string) => invoke<RaterRound>("youzi_rater_comment", { id }),
};

/** 面试 (Rust rater/interview.rs): 准备 and 复盘 under a 投石问路 job. */
export const RATER_INTERVIEW_LANGS = ["中文", "English"] as const;
export const RATER_RESULTS = ["待定", "通过", "没过"] as const;
export type RaterFollow = { about: string; category: string; question: string; answer: string; saved: string | null };
export type RaterPrep = { id: string; job: RaterJob; resume: string; lang: string; form: string; request: string; questions: string[]; follows: RaterFollow[]; sheet: string; advice: string; model: string; note: string; createdAt: string };
export type RaterAsked = { question: string; answer: string; feedback: string };
/** asked: index into the review's items (a summary's: of `review`). */
export type RaterMatched = { review?: string; asked: number; text: string };
export type RaterRuleIdea = { use: string; title: string; text: string; why: string; adopted: string | null };
export type RaterAnalysis = { model: string; prep: string | null; summary: string; hits: RaterMatched[]; misses: RaterMatched[]; questions: RaterFollow[]; rules: RaterRuleIdea[] };
export type RaterReview = { id: string; job: RaterJob; date: string; round: string; form: string; result: string; items: RaterAsked[]; feedback: string; thoughts: string; analysis: RaterAnalysis | null; createdAt: string };
/** 总结复盘: some reviews of a job (oldest first) against a prep (none: the bank only). */
export type RaterSummary = { id: string; job: RaterJob; prep: string | null; reviews: string[]; analysis: RaterAnalysis; createdAt: string };
/** Newest first. */
export type RaterInterviews = { preps: RaterPrep[]; reviews: RaterReview[]; summaries: RaterSummary[] };
/** 复盘 · 记录: a review being filled in a conversation (Rust rater/record.rs `Draft`); session = its 柚子 conversation. */
export type RaterRecord = { session: number; review: RaterReview; ready: boolean; fills: number; createdAt: string };
export const RATER_RECORDS_KEY = ["youzi", "rater", "records"];
export type RaterPrepAsk = { job: number; lang: string; form: string; request: string };
export const RATER_INTERVIEWS_KEY = ["youzi", "rater", "interviews"];
export const sameJob = (a: { id: number; createdAt: string }, b: { id: number; createdAt: string }) => a.id === b.id && a.createdAt === b.createdAt;
export const interviewApi = {
  all: () => invoke<RaterInterviews>("youzi_rater_interviews"),
  prep: (ask: RaterPrepAsk) => invoke<RaterInterviews>("youzi_rater_prep", { ask }),
  prepSave: (prep: RaterPrep) => invoke<RaterInterviews>("youzi_rater_prep_save", { prep }),
  prepKeep: (id: string, index: number) => invoke<RaterInterviews>("youzi_rater_prep_keep", { id, index }),
  reviewSave: (review: RaterReview, jobId: number | null) => invoke<RaterInterviews>("youzi_rater_review_save", { review, jobId }),
  summarize: (reviews: string[], prep: string | null) => invoke<RaterInterviews>("youzi_rater_summarize", { reviews, prep }),
  reviewKeep: (id: string, index: number) => invoke<RaterInterviews>("youzi_rater_review_keep", { id, index }),
  reviewRule: (id: string, index: number) => invoke<RaterInterviews>("youzi_rater_review_rule", { id, index }),
  remove: (id: string) => invoke<RaterInterviews>("youzi_rater_interview_delete", { id }),
  wordPlace: (id: string) => invoke<{ folder: string; name: string }>("youzi_rater_prep_word_place", { id }),
  word: (id: string, folder: string, name: string) => invoke<string>("youzi_rater_prep_word", { id, folder, name }),
  records: () => invoke<RaterRecord[]>("youzi_rater_records"),
  record: (session: number) => invoke<RaterRecord>("youzi_rater_record", { session }),
  recordStart: (job: number, date: string, round: string) => invoke<RaterRecord>("youzi_rater_record_start", { job, date, round }),
  /** The user's edits to the table (kept for the conversation; not undone). */
  recordEdit: (session: number, review: RaterReview) => invoke<RaterRecord>("youzi_rater_record_edit", { session, review }),
  /** 确认: the table becomes a review, the conversation goes (one undo step). */
  recordSave: (session: number, review: RaterReview) => invoke<RaterInterviews>("youzi_rater_record_save", { session, review }),
  recordDiscard: (session: number) => invoke<RaterRecord[]>("youzi_rater_record_discard", { session }),
};
