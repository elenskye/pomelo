// Importer's work on files and rows, kept here and not in the page: it runs to the end while 导入账单 is closed and
// what came of it waits there (the rows are already saved by Rust). One at a time; gone when the app reloads.
import { useSyncExternalStore } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { errorText } from "@/shared/feedback";
import { FIN_KEY } from "./api";
import { importerApi, type ImporterSummary } from "./importApi";

export type ImportJob = {
  doing: string | null;
  error: string | null;
  /** What came of it, once. */
  notice: string | null;
  /** The batch to show. */
  show: number | null;
  /** Screenshots as read this session, by batch. */
  previews: Record<number, string>;
};

let job: ImportJob = { doing: null, error: null, notice: null, show: null, previews: {} };
const listeners = new Set<() => void>();
const set = (change: Partial<ImportJob>) => {
  job = { ...job, ...change };
  listeners.forEach((l) => l());
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};
export const useImportJob = () => useSyncExternalStore(subscribe, () => job);
/** What came of it and the batch to show, once. */
export const importJobTake = () => {
  const { notice, show } = job;
  if (notice != null || show != null) set({ notice: null, show: null });
  return { notice, show };
};
export const importJobPreview = (id: number, preview: string) => set({ previews: { ...job.previews, [id]: preview } });

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const said = (s: ImporterSummary, first: string) => [first, s.linked && `合并 ${s.linked}`, s.review && `待看 ${s.review}`].filter(Boolean).join(" · ");

async function run(qc: QueryClient, doing: string, work: (errors: string[]) => Promise<string | null>) {
  if (job.doing) return;
  set({ doing, error: null, notice: null });
  const errors: string[] = [];
  let notice: string | null = null;
  try {
    notice = await work(errors);
  } catch (e) {
    errors.push(errorText(e));
  }
  set({ doing: null, notice, error: errors.length ? errors.join("；") : null });
  await qc.invalidateQueries({ queryKey: FIN_KEY });
}

/** The files read at once (Rust keeps to four requests), then their rows against the ledger. */
export const runImport = (qc: QueryClient, paths: string[], model: string) =>
  run(qc, paths.length > 1 ? `读 ${paths.length} 个文件…` : "读账单…", async (errors) => {
    const ids: number[] = [];
    let read = 0;
    await Promise.all(
      paths.map((path) =>
        importerApi.extract(path, model).then(
          (r) => {
            ids.push(r.batch.summary.id);
            read += r.batch.rows.length;
            errors.push(...r.errors.map((e) => `${fileName(path)}：${e}`));
            set({ show: r.batch.summary.id, ...(r.preview ? { previews: { ...job.previews, [r.batch.summary.id]: r.preview } } : {}) });
            return qc.invalidateQueries({ queryKey: FIN_KEY });
          },
          (e) => void errors.push(`${fileName(path)}：${errorText(e)}`),
        ),
      ),
    );
    if (!ids.length) return null;
    set({ doing: "查重…" });
    const s = await importerApi.match(ids.sort((a, b) => a - b), model);
    errors.push(...s.errors);
    return said(s, `读出 ${read} 笔`);
  });

/** 再整理: under the 清单 as it is now. */
export const runTidy = (qc: QueryClient, id: number, rows: number[] | null, model: string) =>
  run(qc, "再整理…", async (errors) => {
    const s = await importerApi.tidy([id], rows, model);
    errors.push(...s.errors);
    return said(s, `整理 ${s.tidied} 笔`);
  });
