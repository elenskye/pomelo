// 首页 summary (src-tauri/src/home.rs → home_overview). Rules live in Rust; hidden modules come back as
// null / are left out. 今天的待办 comes from todo_agenda instead.
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import type { Day } from "@/modules/days/api";
import type { Stage } from "@/modules/jobs/api";
import type { Hit } from "@/shared/open";

/** null = module hidden. */
export type Year = { year: string; books: number | null; films: number | null; lessons: number | null; todos: number | null; notes: number | null; jobs: number | null };
/** total in CNY cents. */
export type Point = { day: string; total: number };
export type JobLine = Hit & { status: string; stage: Stage; waitingDays: number | null };
export type JobsCard = { total: number; thisWeek: number; ongoing: number; got: number; follow: JobLine[]; active: JobLine[] };
export type CourseLine = { id: number; name: string; total: number; done: number; doing: number; next: Hit | null };
/** at: UTC timestamp; done: a task checked off (else something added). */
export type Recent = Hit & { at: string; done: boolean };
export type Overview = {
  today: string;
  year: Year;
  assets: Point[] | null;
  jobs: JobsCard | null;
  reading: Hit[] | null;
  drawing: CourseLine[] | null;
  /** 快到的日子 (days.rs soon); null = hidden. */
  days: Day[] | null;
  recent: Recent[];
};

export const HOME_KEY = ["home"];

/** Refetched every time the page opens (other pages don't invalidate it). */
export const useOverview = (hidden: string[]) =>
  useQuery({ queryKey: [...HOME_KEY, hidden], queryFn: () => invoke<Overview>("home_overview", { hidden }), refetchOnMount: "always" });
