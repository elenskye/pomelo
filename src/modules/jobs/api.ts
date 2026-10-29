// Typed wrappers for Rust job_* commands (src-tauri/src/modules/jobs.rs) + React Query hooks.
// Rules (current status from the checked steps, 投递日期, follow-up, 统计) live in Rust.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";

export type Stage = "wish" | "applied" | "active" | "got" | "closed";
export const CHANNELS = "job_channel";

/** count = applications whose current status this is; used = applications with it on their timeline. */
export type Status = { id: number; name: string; stage: Stage; sortOrder: number; count: number; used: number };
/** One status an application goes through; doneOn only when done (may be unknown). */
export type Step = { statusId: number; done: boolean; doneOn: string | null };

export type Job = {
  id: number;
  company: string;
  position: string | null;
  location: string | null;
  link: string | null;
  kindId: number | null; // 渠道
  resume: string | null;
  note: string | null;
  followedOn: string | null;
  createdAt: string;
  updatedAt: string;
  /** In the status list's order. */
  steps: Step[];
  /** Derived: the last checked step, else the first 准备 status (想投). */
  statusId: number | null;
  stage: Stage;
  /** Derived: the earliest checked day. */
  appliedOn: string | null;
  /** Stage 等回音: days since its day / the last follow-up. */
  waitingDays: number | null;
  followUp: boolean;
};
/** 统计 (jobs.rs `stats`; 首页 and 小拾 use the same numbers). */
export type JobStats = {
  today: string;
  applied: number;
  thisWeek: number;
  ongoing: number;
  got: number;
  /** 已投递 → … → Offer: checked / applications that have the step. */
  funnel: { statusId: number; name: string; have: number; done: number }[];
  /** Applications by 投递日期, oldest first: the last 30 days, the last 12 weeks (Monday first). */
  days: Bucket[];
  weeks: Bucket[];
  statuses: Status[];
};
export type Bucket = { from: string; to: string; count: number };
export type JobPatch = Partial<Pick<Job, "company" | "position" | "location" | "link" | "kindId" | "resume" | "note" | "steps">>;

const api = {
  statuses: () => invoke<Status[]>("job_statuses"),
  /** New statuses are middle steps (stage active); stages aren't shown or edited any more. */
  createStatus: (v: { name: string }) => invoke<Status>("job_status_create", v),
  updateStatus: (v: { id: number; name?: string; stage?: Stage }) => invoke<Status>("job_status_update", v),
  deleteStatus: (id: number) => invoke<void>("job_status_delete", { id }),
  reorderStatuses: (ids: number[]) => invoke<void>("job_statuses_reorder", { ids }),
  jobs: () => invoke<Job[]>("jobs_list"),
  stats: () => invoke<JobStats>("job_stats"),
  create: (patch: JobPatch) => invoke<Job>("job_create", { patch }),
  update: (v: { id: number; patch: JobPatch }) => invoke<Job>("job_update", v),
  followUp: (id: number) => invoke<Job>("job_follow_up", { id }),
  remove: (id: number) => invoke<void>("job_delete", { id }),
};

export const JOBS_KEY = ["jobs"];
export const useStatuses = () => useQuery({ queryKey: [...JOBS_KEY, "statuses"], queryFn: api.statuses });
export const useJobs = () => useQuery({ queryKey: [...JOBS_KEY, "list"], queryFn: api.jobs });
export const useJobStats = () => useQuery({ queryKey: [...JOBS_KEY, "stats"], queryFn: api.stats });

export function useJobMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  // channels live in kinds → refresh those too
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: JOBS_KEY });
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    createStatus: useMutation({ mutationFn: api.createStatus, onSuccess }),
    updateStatus: useMutation({ mutationFn: api.updateStatus, onSuccess, onError }),
    deleteStatus: useMutation({ mutationFn: api.deleteStatus, onSuccess, onError }),
    reorderStatuses: useMutation({ mutationFn: api.reorderStatuses, onSuccess, onError }),
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    followUp: useMutation({ mutationFn: api.followUp, onSuccess, onError }),
    remove: useMutation({ mutationFn: api.remove, onSuccess, onError }),
  };
}

/** Badge colors per stage (status tokens). */
export const STAGE_STYLE: Record<Stage, { bg: string; fg: string }> = {
  wish: { bg: "var(--status-unmarked-bg)", fg: "var(--status-unmarked-fg)" },
  applied: { bg: "var(--status-todo-bg)", fg: "var(--status-todo-fg)" },
  active: { bg: "var(--status-doing-bg)", fg: "var(--status-doing-fg)" },
  got: { bg: "var(--status-done-bg)", fg: "var(--status-done-fg)" },
  closed: { bg: "var(--muted)", fg: "var(--muted-foreground)" },
};

export const jobLabel = (j: Pick<Job, "company" | "position">) => (j.position ? `${j.company} · ${j.position}` : j.company);
/** Jobs for a pick dialog (PickField): company · position, then where and when (applied, else added). */
export const jobPicks = (jobs: Job[]) => jobs.map((j) => ({ value: j.id as number | null, label: jobLabel(j), detail: [j.location, (j.appliedOn ?? j.createdAt).slice(0, 10).replace(/-/g, " / ")].filter(Boolean).join(" · ") }));
