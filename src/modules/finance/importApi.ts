import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { FIN_KEY, type Entry } from "./api";
import type { Currency } from "./money";

export type ImportDraft = { day: string; amount: number | null; currency: Currency; treatment: "out" | "in" | "refund" | "neutral"; category: string | null; note: string; refundOf: number | null };
export type SourceRow = { sheet: string; rowNumber: number; raw: [string, string][]; tradeId: string; time: string; status: string; draft: ImportDraft; issues: string[]; region?: [number, number, number, number] | null };
export type ImportRow = { pendingCandidates?: { id: number; batchId: number; name: string; draft: ImportDraft }[]; ai?: { model: string; noteError?: string | null; settled?: boolean; why?: string | null; duplicate: string; reason: string; duplicateRow?: number | null; target?: string | null; myCategory?: boolean } | null; fileName?: string | null; id: number; source: SourceRow; draft: ImportDraft; state: string; entryId: number | null; resolution: "new" | "linked" | null; candidates: { entry: Entry; sources: string[] }[] };
export type BatchSummary = { id: number; name: string; title: string | null; sortOrder: number; source: string; createdAt: string; count: number; pending: number };
export type ImportBatch = { summary: BatchSummary; rows: ImportRow[] };
/** Importer (youzi/assistants/importer.rs): the 提示词清单 and the model. */
export type ImporterPoint = { text: string; on: boolean };
export type ImporterSettings = { model: string | null; level: string; points: ImporterPoint[] };
export type ImporterSummary = { tidied: number; linked: number; review: number; errors: string[] };
export const importerApi = {
  settings: () => invoke<ImporterSettings>("youzi_importer"),
  save: (settings: ImporterSettings) => invoke<ImporterSettings>("youzi_importer_save", { settings }),
  /** One file → one batch. */
  extract: (path: string, model: string) => invoke<{ batch: ImportBatch; preview: string | null; errors: string[] }>("youzi_importer_extract", { path, model }),
  match: (ids: number[], model: string) => invoke<ImporterSummary>("youzi_importer_match", { ids, model }),
  /** `rows`: only these (ticked) rows. */
  tidy: (ids: number[], rows: number[] | null, model: string) => invoke<ImporterSummary>("youzi_importer_tidy", { ids, rows, model }),
};
export const importApi = {
  linkPending: (id: number, target: number) => invoke<void>("fin_import_link_pending", { id, target }),
  preview: (id: number, path: string) => invoke<string>("fin_import_preview", { id, path }),
  add: (id: number) => invoke<number>("fin_import_add", { id }),
  ignore: (id: number, ignored: boolean) => invoke<void>("fin_import_ignore", { id, ignored }),
  batches: () => invoke<BatchSummary[]>("fin_imports"),
  rename: (id: number, title: string) => invoke<void>("fin_import_rename", { id, title }),
  remove: (id: number) => invoke<void>("fin_import_delete", { id }),
  reorder: (ids: number[]) => invoke<void>("fin_imports_reorder", { ids }),
  batch: (id: number) => invoke<ImportBatch>("fin_import_batch", { id }),
  save: (id: number, draft: ImportDraft) => invoke<void>("fin_import_save", { id, draft }),
  setCategory: (ids: number[], category: string | null) => invoke<number>("fin_import_set_category", { ids, category }),
  commit: (id: number, ids: number[]) => invoke<{ posted: number; neutral: number }>("fin_import_commit", { id, ids }),
  resolve: (id: number, target: number | null) => invoke<void>("fin_import_resolve", { id, target }),
  unlink: (id: number) => invoke<void>("fin_import_unlink", { id }),
  sources: (id: number) => invoke<ImportRow[]>("fin_entry_sources", { id }),
  candidates: (currency: Currency, day: string) => invoke<Entry[]>("fin_refund_candidates", { currency, day }),
};
export const useSources = (id?: number) => useQuery({ queryKey: [...FIN_KEY, "sources", id], queryFn: () => importApi.sources(id!), enabled: id != null });
export const useRefundCandidates = (currency: Currency, day: string, enabled = true) => useQuery({ queryKey: [...FIN_KEY, "refund-candidates", currency, day], queryFn: () => importApi.candidates(currency, day), enabled: enabled && /^\d{4}-\d{2}-\d{2}$/.test(day) });
export const TREATMENTS = [{ value: "out", label: "支出" }, { value: "in", label: "收入" }, { value: "refund", label: "退款" }, { value: "neutral", label: "不计收支" }] as const;
