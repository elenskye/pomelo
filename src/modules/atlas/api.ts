// Typed wrappers for Rust atlas_* commands (src-tauri/src/youzi/atlas.rs; atlas_run is in schedule.rs) + React Query
// hooks. Atlas keeps every 年度总结 run as history; each row says which design it was made for.
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type Entry = {
  id: number;
  year: string;
  /** written up to that day (the year wasn't over); null = the whole year */
  until: string | null;
  createdAt: string;
  /** `<id>@<version>` */
  design: string;
  /** scenes told (0: that step failed, `error`) */
  scenes: number;
  error: string | null;
  modelName: string;
  /** its conversation under 定时 (null once deleted) */
  sessionId: number | null;
};
export type YearCount = { year: string; records: number };
export type Overview = {
  entries: Entry[];
  years: YearCount[];
  /** the year being written right now */
  writing: string | null;
  today: string;
  /** the design new runs are made for */
  design: string;
  /** the run 首页's envelope opens (the new year's first week) */
  envelope: number | null;
};
/** One run in full, as a design draws it (shape: visual-atelier schemes/pomelo/atlas/README.md「数据」). */
export type Full = { id: number; year: string; until: string | null; createdAt: string; design: string; modelName: string; letter: string; snapshot: unknown; scenes: unknown[] | null; error: string | null };

export const atlasApi = {
  overview: () => invoke<Overview>("atlas_overview"),
  get: (id: number) => invoke<Full>("atlas_get", { id }),
  remove: (id: number) => invoke<void>("atlas_delete", { id }),
  /** null = this year up to today; a past year = 补做 */
  run: (year: string | null) => invoke<void>("atlas_run", { year }),
};

export const ATLAS_KEY = ["atlas"];
export const OVERVIEW_KEY = [...ATLAS_KEY, "overview"];
/** Asked again while a year is being written (and now and then: the 12 月 31 日 run writes by itself). */
export const useAtlasOverview = (fast = false) => useQuery({ queryKey: OVERVIEW_KEY, queryFn: atlasApi.overview, refetchInterval: (q) => (fast || q.state.data?.writing ? 2000 : 20000) });
export const useAtlasRun = (id: number | null) => useQuery({ queryKey: [...ATLAS_KEY, "run", id], queryFn: () => atlasApi.get(id!), enabled: id !== null, staleTime: Infinity });
