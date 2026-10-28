import { invoke } from "@tauri-apps/api/core";

/** Rater's question bank (Rust picky/assistants/rater.rs). */
export const RATER_CATEGORIES = ["简历题", "通用题", "突击题"] as const;
/** Rater's rules: Seeker records of these 用途 (the last three: each 题库 SKILL's workflow). */
export const RATER_USES = [{ value: "题库维护", label: "题库维护" }, { value: "面试", label: "面试" }, ...RATER_CATEGORIES.map((c) => ({ value: c, label: `${c} SKILL` }))];
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
/** Rater's own model for its SKILLs (not undone). */
export type RaterOptions = { model: string | null; thinking: boolean };
export const RATER_POLISH = "润色";
/** 卡片练习 (Rust rater/practice.rs). */
export const RATER_MODES = ["自己挑", "按 JD", "随机"] as const;
export const RATER_KNOW = "会";
export const RATER_UNSURE = "不熟";
export const RATER_FOLLOW_MAX = 2;
export const RATER_RANDOM_COUNT = 10;
/** A card: question = the bank question (a follow-up: the one it follows up); follow-ups are dashed with 小拾's reference answer. */
export type RaterCard = { question: string; text: string; category: string; follow: boolean; reference: string; saved: string | null; mark: string | null; markedAt: string | null; typed: string; asked: boolean };
/** notes: card = index in the round. */
export type RaterFeedback = { model: string; summary: string; good: string[]; gaps: string[]; notes: { card: number; text: string }[] };
export type RaterRound = { id: string; mode: string; job: RaterJob | null; advice: string; cards: RaterCard[]; startedAt: string; endedAt: string | null; feedback: RaterFeedback | null };
export type RaterRoundRow = { id: string; mode: string; job: string | null; startedAt: string; endedAt: string | null; cards: number; follows: number; know: number; unsure: number; left: number };
export type RaterPractice = { running: RaterRound | null; rounds: RaterRoundRow[] };
export type RaterStart = { mode: string; job: number | null; questions: string[]; count: number; advice: string };
export type RaterPick = { questions: string[]; advice: string; model: string; note: string };
/** 小拾 should comment: something typed, or a follow-up (Rust `Round::wants_comment`). */
export const wantsComment = (r: RaterRound) => r.cards.some((c) => c.follow || (c.mark != null && c.typed.trim() !== ""));
export const RATER_PRACTICE_KEY = ["picky", "rater", "practice"];
export const RATER_KEY = ["picky", "rater"];
export const RATER_OPTIONS_KEY = ["picky", "rater", "options"];
export const raterApi = {
  bank: () => invoke<RaterBank>("picky_rater"),
  save: (question: RaterQuestion) => invoke<RaterBank>("picky_rater_save", { question }),
  remove: (id: string) => invoke<RaterBank>("picky_rater_delete", { id }),
  seconds: (text: string) => invoke<number>("picky_rater_seconds", { text }),
  suggest: (ask: RaterAsk) => invoke<RaterProposal>("picky_rater_suggest", { ask }),
  add: (questions: RaterQuestion[]) => invoke<RaterBank>("picky_rater_add", { questions }),
  /** Each SKILL's built-in workflow: [category, text]. */
  defaults: () => invoke<[string, string][]>("picky_rater_defaults"),
  options: () => invoke<RaterOptions>("picky_rater_options"),
  setOptions: (options: RaterOptions) => invoke<RaterOptions>("picky_rater_options_set", { options }),
  practice: () => invoke<RaterPractice>("picky_rater_practice"),
  round: (id: string) => invoke<RaterRound>("picky_rater_round", { id }),
  start: (start: RaterStart) => invoke<RaterPractice>("picky_rater_round_start", { start }),
  mark: (id: string, card: number, mark: string, typed: string) => invoke<RaterPractice>("picky_rater_mark", { id, card, mark, typed }),
  stop: (id: string) => invoke<RaterPractice>("picky_rater_stop", { id }),
  removeRound: (id: string) => invoke<RaterPractice>("picky_rater_round_delete", { id }),
  pick: (job: number) => invoke<RaterPick>("picky_rater_pick", { job }),
  follow: (id: string, card: number, typed: string) => invoke<RaterPractice>("picky_rater_follow", { id, card, typed }),
  keep: (id: string, card: number) => invoke<RaterRound>("picky_rater_keep", { id, card }),
  comment: (id: string) => invoke<RaterRound>("picky_rater_comment", { id }),
};

/** 面试 (Rust rater/interview.rs): 准备 and 复盘 under a 投石问路 job. */
export const RATER_INTERVIEW_LANGS = ["中文", "English"] as const;
export const RATER_RESULTS = ["待定", "通过", "没过"] as const;
export type RaterFollow = { about: string; category: string; question: string; answer: string; saved: string | null };
export type RaterPrep = { id: string; job: RaterJob; resume: string; lang: string; form: string; request: string; questions: string[]; follows: RaterFollow[]; sheet: string; advice: string; model: string; note: string; createdAt: string };
export type RaterAsked = { question: string; answer: string; feedback: string };
/** asked: index into the review's items. */
export type RaterMatched = { asked: number; text: string };
export type RaterRuleIdea = { use: string; title: string; text: string; why: string; adopted: string | null };
export type RaterAnalysis = { model: string; prep: string | null; summary: string; hits: RaterMatched[]; misses: RaterMatched[]; questions: RaterFollow[]; rules: RaterRuleIdea[] };
export type RaterReview = { id: string; job: RaterJob; date: string; round: string; form: string; result: string; items: RaterAsked[]; feedback: string; thoughts: string; analysis: RaterAnalysis | null; createdAt: string };
/** Newest first. */
export type RaterInterviews = { preps: RaterPrep[]; reviews: RaterReview[] };
export type RaterPrepAsk = { job: number; lang: string; form: string; request: string };
export const RATER_INTERVIEWS_KEY = ["picky", "rater", "interviews"];
export const sameJob = (a: { id: number; createdAt: string }, b: { id: number; createdAt: string }) => a.id === b.id && a.createdAt === b.createdAt;
export const interviewApi = {
  all: () => invoke<RaterInterviews>("picky_rater_interviews"),
  prep: (ask: RaterPrepAsk) => invoke<RaterInterviews>("picky_rater_prep", { ask }),
  prepSave: (prep: RaterPrep) => invoke<RaterInterviews>("picky_rater_prep_save", { prep }),
  prepKeep: (id: string, index: number) => invoke<RaterInterviews>("picky_rater_prep_keep", { id, index }),
  reviewSave: (review: RaterReview, jobId: number | null) => invoke<RaterInterviews>("picky_rater_review_save", { review, jobId }),
  compare: (id: string, prep: string | null) => invoke<RaterInterviews>("picky_rater_review_compare", { id, prep }),
  reviewKeep: (id: string, index: number) => invoke<RaterInterviews>("picky_rater_review_keep", { id, index }),
  reviewRule: (id: string, index: number) => invoke<RaterInterviews>("picky_rater_review_rule", { id, index }),
  remove: (id: string) => invoke<RaterInterviews>("picky_rater_interview_delete", { id }),
  wordPlace: (id: string) => invoke<{ folder: string; name: string }>("picky_rater_prep_word_place", { id }),
  word: (id: string, folder: string, name: string) => invoke<string>("picky_rater_prep_word", { id, folder, name }),
};
