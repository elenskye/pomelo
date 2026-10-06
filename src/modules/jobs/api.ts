// Typed wrappers for Rust job_* commands (src-tauri/src/modules/jobs.rs) + React Query hooks.
// Rules (stage stamps, follow-up, reached, the linked 一步一步 task) live in Rust.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";

export type Stage = "wish" | "applied" | "active" | "got" | "closed";
export const STAGES: Record<Stage, string> = { wish: "准备", applied: "等回音", active: "进行中", got: "拿到", closed: "结束" };
export const CHANNELS = "job_channel";

export type Status = { id: number; name: string; stage: Stage; sortOrder: number; count: number };

export type Job = {
  id: number;
  company: string;
  position: string | null;
  location: string | null;
  link: string | null;
  kindId: number | null; // 渠道
  resume: string | null;
  coverLetter: string | null;
  deadline: string | null;
  salary: string | null;
  note: string | null;
  statusId: number;
  appliedOn: string | null;
  stepOn: string | null;
  followedOn: string | null;
  reachedId: number | null;
  taskId: number | null;
  createdAt: string;
  updatedAt: string;
  stage: Stage;
  /** Stage 等回音: days since applied / last followed up. */
  waitingDays: number | null;
  followUp: boolean;
};
export type JobPatch = Partial<
  Pick<Job, "company" | "position" | "location" | "link" | "kindId" | "resume" | "coverLetter" | "deadline" | "salary" | "note" | "statusId" | "appliedOn" | "stepOn">
>;

const api = {
  statuses: () => invoke<Status[]>("job_statuses"),
  createStatus: (v: { name: string; stage: Stage }) => invoke<Status>("job_status_create", v),
  updateStatus: (v: { id: number; name?: string; stage?: Stage }) => invoke<Status>("job_status_update", v),
  deleteStatus: (id: number) => invoke<void>("job_status_delete", { id }),
  reorderStatuses: (ids: number[]) => invoke<void>("job_statuses_reorder", { ids }),
  jobs: () => invoke<Job[]>("jobs_list"),
  create: (patch: JobPatch) => invoke<Job>("job_create", { patch }),
  update: (v: { id: number; patch: JobPatch }) => invoke<Job>("job_update", v),
  followUp: (id: number) => invoke<Job>("job_follow_up", { id }),
  remove: (id: number) => invoke<void>("job_delete", { id }),
};

export const JOBS_KEY = ["jobs"];
export const useStatuses = () => useQuery({ queryKey: [...JOBS_KEY, "statuses"], queryFn: api.statuses });
export const useJobs = () => useQuery({ queryKey: [...JOBS_KEY, "list"], queryFn: api.jobs });

export function useJobMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  // linked tasks live in 一步一步 and channels in kinds → refresh those too
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: JOBS_KEY });
    void qc.invalidateQueries({ queryKey: ["todo"] });
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
