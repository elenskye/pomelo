import { useSyncExternalStore } from "react";
import { X } from "lucide-react";
import { errorText } from "@/shared/feedback";
import { Button, IconButton } from "@/shared/ui";
import type { SeekerLang, SeekerSuggestion } from "./seekerApi";

/** A model call of Seeker's (选材, 审查, 润色…), kept here and not in the page: it runs to the end while
 * the page is closed and its result waits (审查 / 润色要点 / Summary: Rust has already saved the 批注).
 * One per kind and language (润色一条素材: per material); gone when the app reloads. */
export type SeekerJob = { key: string; kind: string; lang: SeekerLang; recordId: string | null; state: "running" | "done" | "error"; error?: string; message?: string; suggestion?: SeekerSuggestion };

let jobs: SeekerJob[] = [];
const listeners = new Set<() => void>();
const subscribe = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };
const set = (next: SeekerJob[]) => { jobs = next; listeners.forEach((l) => l()); };
const put = (job: SeekerJob) => set([...jobs.filter((j) => j.key !== job.key), job]);

export const seekerJobKey = (kind: string, lang: SeekerLang, recordId: string | null = null) => `${kind}:${recordId ?? lang}`;
export const useSeekerJobs = () => useSyncExternalStore(subscribe, () => jobs);
export const dropSeekerJob = (key: string) => set(jobs.filter((j) => j.key !== key));

/** Starts one unless the same one is running; `work` gives what the line shows when it is done. */
export function startSeekerJob(job: { kind: string; lang: SeekerLang; recordId: string | null }, work: () => Promise<{ message?: string; suggestion?: SeekerSuggestion }>) {
  const key = seekerJobKey(job.kind, job.lang, job.recordId);
  if (jobs.some((j) => j.key === key && j.state === "running")) return;
  put({ ...job, key, state: "running" });
  work().then((done) => put({ ...job, key, state: "done", ...done }), (e) => put({ ...job, key, state: "error", error: errorText(e) }));
}

/** The line under a button or on a card: 跳动的淡墨「正在…」, then what came of it (查看 when there is
 * something to look at) or a red line with 重试. `end`: under a button at the right end of a row — the
 * words and their buttons kept together at the right (else they start at the left of the line). */
export function SeekerJobLine(props: { job: SeekerJob | undefined; running: string; failed: string; view?: () => void; retry?: () => void; end?: boolean }) {
  const j = props.job;
  if (!j) return null;
  const words = props.end ? "min-w-0 break-words text-right" : "min-w-0 flex-1 break-words";
  const row = props.end ? "justify-end" : "";
  if (j.state === "running") return <p role="status" className={`animate-pulse text-xs text-muted-foreground ${props.end ? "text-right" : ""}`}>{props.running}</p>;
  const close = <IconButton label="关掉" onClick={() => dropSeekerJob(j.key)}><X size={12} /></IconButton>;
  if (j.state === "error") return <div role="alert" className={`flex items-start gap-1 text-xs text-destructive ${row}`}><p className={`${words} pt-1`}>{props.failed}：{j.error}</p>{props.retry && <Button variant="ghost" className="shrink-0 px-1.5 py-0.5 text-xs" onClick={props.retry}>重试</Button>}{close}</div>;
  return <div role="status" className={`flex items-center gap-1 text-xs text-muted-foreground ${row}`}><p className={words}>{j.message}</p>{props.view && j.suggestion && <Button variant="ghost" className="shrink-0 px-1.5 py-0.5 text-xs" onClick={props.view}>查看</Button>}{close}</div>;
}
