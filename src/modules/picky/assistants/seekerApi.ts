import { invoke } from "@tauri-apps/api/core";
export type SeekerHeader = { id: string; label: string; value: string; visible: boolean };
export type SeekerPoint = { id: string; text: string };
export type SeekerSource = { recordId: string; revision: number; pointIds: string[] };
export type SeekerEntry = { id: string; time: string; name: string; role: string; location: string; note: string; points: SeekerPoint[]; source?: SeekerSource | null };
export type SeekerSection = { id: string; title: string; entries: SeekerEntry[]; category?: string };
export type SeekerDraft = { name: string; header: SeekerHeader[]; sections: SeekerSection[]; jd: string };
export type SeekerRecord = { id: string; kind: string; category: string; title: string; text: string; time: string; role: string; location: string; points: SeekerPoint[]; fields: SeekerHeader[]; source: string | null; revision: number; enabled: boolean };
export type SeekerSettings = { selectionModel: string | null; reviewModel: string | null; thinking: boolean; cloudAllowed: boolean; developerRole: boolean };
export type SeekerRevision = { id: number; recordId: string; body: SeekerRecord; savedAt: string };
export type SeekerVersion = { id: number; label: string; body: SeekerDraft; createdAt: string };
export type SeekerCatalog = { records: SeekerRecord[]; revisions: SeekerRevision[]; versions: SeekerVersion[]; settings: SeekerSettings };
export type SeekerWorkspace = { folder: string; draft: SeekerDraft | null; updatedAt: string | null; baseline: SeekerDraft | null; catalog: SeekerCatalog; output: SeekerOutput; warnings?: string[]; choices?: SeekerSelection[] };
/** Template paragraph for each part of the resume (education, note: optional). */
export type SeekerSlots = { name: number; contact: number; section: number; entry: number; point: number; education: number | null; note: number | null };
export type SeekerTemplate = { path: string; slots: SeekerSlots; paragraphs: (string | null)[] };
export type SeekerExport = { id: string; label: string; draft: SeekerDraft; path: string; pages: number; renderer: string; template: SeekerTemplate; createdAt: string };
export type SeekerJobSnapshot = { id: number; company: string; position: string | null; location: string | null; link: string | null; note: string | null; createdAt: string };
export type SeekerJobExport = { id: string; exportId: string; job: SeekerJobSnapshot; jd: string; createdAt: string; current: boolean };
export type SeekerOutput = { template: SeekerTemplate | null; exports: SeekerExport[]; links: SeekerJobExport[] };
export type SeekerOutputPreview = { ticket: string; pages: number; pdfPath: string; defaultFolder: string; renderer: string };
export type SeekerSelection = { recordId: string; pointIds: string[] };
export type SeekerImport = { items: { incoming: SeekerRecord; current: SeekerRecord | null }[]; skipped: string[] };
export type SourceChange = { entryId: string; title: string; before: SeekerEntry; after: SeekerEntry };
export type SeekerSuggestion = { kind: string; model: string; note: string; advice: string; selections: SeekerSelection[]; beforeDraft: SeekerDraft | null; afterDraft: SeekerDraft | null; beforeRecord: SeekerRecord | null; afterRecord: SeekerRecord | null };
export type SeekerEdit = { title: string; fields: { label: string; value: string }[]; apply: (draft: SeekerDraft, values: string[]) => void };
export const SEEKER_KEY = ["picky", "seeker"];
export const seekerApi = {
  workspace: () => invoke<SeekerWorkspace>("picky_seeker"),
  folder: (folder: string) => invoke<SeekerWorkspace>("picky_seeker_folder", { folder }),
  newDraft: () => invoke<SeekerWorkspace>("picky_seeker_new"),
  save: (draft: SeekerDraft) => invoke<SeekerWorkspace>("picky_seeker_save", { draft }),
  deleteBlocks: (ids: string[]) => invoke<SeekerWorkspace>("picky_seeker_delete_blocks", { ids }),
  recordSave: (record: SeekerRecord) => invoke<SeekerWorkspace>("picky_seeker_record_save", { record }),
  recordDelete: (id: string) => invoke<SeekerWorkspace>("picky_seeker_record_delete", { id }),
  settings: (settings: SeekerSettings) => invoke<SeekerWorkspace>("picky_seeker_settings", { settings }),
  previewImport: () => invoke<SeekerImport>("picky_seeker_import_preview"),
  import: (records: SeekerRecord[]) => invoke<SeekerWorkspace>("picky_seeker_import_confirm", { records }),
  assemble: (selections: SeekerSelection[], replace: boolean) => invoke<SeekerWorkspace>("picky_seeker_assemble", { selections, replace }),
  sources: () => invoke<SourceChange[]>("picky_seeker_sources"),
  sourceApply: (change: SourceChange) => invoke<SeekerWorkspace>("picky_seeker_source_apply", { change }),
  confirm: (label: string) => invoke<SeekerWorkspace>("picky_seeker_confirm", { label }),
  restore: (versionId: number) => invoke<SeekerWorkspace>("picky_seeker_restore", { versionId }),
  reorder: (kind: string, parent: string | null, ids: string[]) => invoke<SeekerWorkspace>("picky_seeker_reorder", { kind, parent, ids }),
  apply: (before: SeekerDraft, after: SeekerDraft) => invoke<SeekerWorkspace>("picky_seeker_apply", { before, after }),
  suggest: (kind: string, request: string, recordId: string | null = null) => invoke<SeekerSuggestion>("picky_seeker_suggest", { kind, request, recordId }),
  templateRead: (path: string) => invoke<(string | null)[]>("picky_seeker_template_read", { path }),
  template: (template: SeekerTemplate | null) => invoke<SeekerWorkspace>("picky_seeker_template", { template }),
  outputPreview: () => invoke<SeekerOutputPreview>("picky_seeker_output_preview"),
  outputSave: (ticket: string, directory: string, filename: string, jobId: number | null) => invoke<{ workspace: SeekerWorkspace; export: SeekerExport }>("picky_seeker_output_save", { ticket, directory, filename, jobId }),
  outputDiscard: (ticket: string) => invoke<void>("picky_seeker_output_discard", { ticket }),
  jobLink: (exportId: string, jobId: number) => invoke<SeekerWorkspace>("picky_seeker_job_link", { exportId, jobId }),
  exportRestore: (exportId: string) => invoke<SeekerWorkspace>("picky_seeker_export_restore", { exportId }),
};
