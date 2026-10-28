import { invoke } from "@tauri-apps/api/core";
export type SeekerHeader = { id: string; label: string; value: string; visible: boolean };
export type SeekerPoint = { id: string; text: string };
export type SeekerSource = { recordId: string; revision: number; pointIds: string[] };
export type SeekerEntry = { id: string; time: string; name: string; role: string; location: string; note: string; points: SeekerPoint[]; source?: SeekerSource | null };
export type SeekerSection = { id: string; title: string; entries: SeekerEntry[]; category?: string };
export type SeekerJobRef = { id: number; createdAt: string };
/** A resume's language: each has its own materials, rules, draft, template and exports. */
export type SeekerLang = "cn" | "en";
/** An English draft is a Resume or a CV (only the reminders differ). */
export type SeekerForm = "resume" | "cv";
export type SeekerDraft = { lang?: SeekerLang; form?: SeekerForm | ""; name: string; header: SeekerHeader[]; sections: SeekerSection[]; jd: string; job?: SeekerJobRef | null };
export const draftLang = (d: SeekerDraft | null | undefined): SeekerLang => (d?.lang === "en" ? "en" : "cn");
/** kind: material · profile · fact (category 经历档案: a dossier, `material` / `materialEn` its Chinese / English material) · rule · decision (有效个人约定) · example · application.
 * lang: cn · en · "" (both: rules, 约定, 范例); facts, 档案 and 网申 are both languages' whatever it says. */
export type SeekerRecord = { id: string; kind: string; category: string; title: string; text: string; time: string; role: string; location: string; points: SeekerPoint[]; fields: SeekerHeader[]; stack: string; material: string | null; materialEn?: string | null; lang?: string; source: string | null; revision: number; enabled: boolean };
/** Whether a page in `lang` shows the record (Rust catalog.rs `for_lang`). */
export const recordIn = (r: SeekerRecord, lang: SeekerLang) => ["fact", "application"].includes(r.kind) || r.lang === "" || (r.lang ?? "cn") === lang;
/** lang: the language the page shows ("en"; else Chinese). */
export type SeekerSettings = { selectionModel: string | null; reviewModel: string | null; thinking: boolean; cloudAllowed: boolean; developerRole: boolean; lang?: string };
export type SeekerRevision = { id: number; recordId: string; body: SeekerRecord; savedAt: string };
export type SeekerVersion = { id: number; label: string; body: SeekerDraft; createdAt: string };
export type SeekerCatalog = { records: SeekerRecord[]; revisions: SeekerRevision[]; versions: SeekerVersion[]; settings: SeekerSettings };
export type SeekerChoice = { jobId: number; jobCreatedAt: string; folder: string };
/** One language's draft, its baseline and the reminders about it. */
export type SeekerDesk = { draft: SeekerDraft | null; updatedAt: string | null; baseline: SeekerDraft | null; warnings?: string[] };
/** The Chinese desk at the top (its names from before English), the English one in `en`. */
export type SeekerWorkspace = SeekerDesk & { folder: string; en: SeekerDesk; catalog: SeekerCatalog; output: SeekerOutput; jobCompanies?: SeekerChoice[]; choices?: SeekerSelection[] };
export const deskOf = (w: SeekerWorkspace, lang: SeekerLang): SeekerDesk => (lang === "en" ? w.en : w);
export const pageLang = (w: SeekerWorkspace | undefined): SeekerLang => (w?.catalog.settings.lang === "en" ? "en" : "cn");
/** Template paragraph for each part of the resume (contact2, education, note, summary, blank: optional; blank: an empty paragraph). */
export type SeekerSlots = { name: number; contact: number; contact2?: number | null; section: number; entry: number; point: number; education: number | null; note: number | null; summary?: number | null; blank?: number | null };
export type SeekerTemplate = { path: string; slots: SeekerSlots; paragraphs: (string | null)[] };
export type SeekerExport = { id: string; label: string; draft: SeekerDraft; path: string; template: SeekerTemplate; createdAt: string };
export type SeekerJobSnapshot = { id: number; company: string; position: string | null; location: string | null; link: string | null; note: string | null; createdAt: string };
export type SeekerJobExport = { id: string; exportId: string; job: SeekerJobSnapshot; jd: string; createdAt: string; current: boolean };
export type SeekerOutput = { template: SeekerTemplate | null; templateEn?: SeekerTemplate | null; exports: SeekerExport[]; links: SeekerJobExport[] };
export const templateOf = (o: SeekerOutput, lang: SeekerLang) => (lang === "en" ? o.templateEn ?? null : o.template);
/** knowledge/ of the Seeker folder: where it is, the 知识库 it is in, its companies (one folder each). */
export type SeekerCompany = { folder: string; name: string };
export type SeekerKnowledge = { path: string | null; library: string | null; companies: SeekerCompany[]; error: string | null; cv?: string | null; cvLibrary?: string | null };
/** cv/ and material/ of the Seeker folder (Rust seeker/files.rs); modified: seconds since 1970. */
export type SeekerCvFile = { path: string; name: string; lang: SeekerLang; series: string; version: number | null; modified: number; size: number; template: boolean; export: boolean; jobs: string[] };
export type SeekerMaterialFile = { path: string; name: string; group: string; size: number; modified: number };
export type SeekerFiles = { cv: SeekerCvFile[]; material: SeekerMaterialFile[]; transfer: boolean; error: string | null };
export const SEEKER_FILES_KEY = ["picky", "seeker", "files"];
/** A job's company folder: by name (auto), or the user's choice ("" = none). */
export type SeekerJobCompany = { companies: SeekerCompany[]; auto: string | null; chosen: string | null };
export const jobCompanyFolder = (c: SeekerJobCompany | undefined) => (c ? (c.chosen ?? c.auto) || null : null);
export type SeekerSelection = { recordId: string; pointIds: string[] };
export type SeekerImport = { items: { incoming: SeekerRecord; current: SeekerRecord | null }[]; gone: SeekerRecord[]; skipped: string[] };
export type SourceChange = { entryId: string; title: string; before: SeekerEntry; after: SeekerEntry };
export type SeekerSuggestion = { kind: string; model: string; note: string; advice: string; selections: SeekerSelection[]; beforeDraft: SeekerDraft | null; afterDraft: SeekerDraft | null; beforeRecord: SeekerRecord | null; afterRecord: SeekerRecord | null };
export type SeekerEdit = { title: string; fields: { label: string; value: string }[]; apply: (draft: SeekerDraft, values: string[]) => void };
export const SEEKER_KEY = ["picky", "seeker"];
export const SEEKER_KNOWLEDGE_KEY = ["picky", "seeker", "knowledge"];
export const seekerJobCompanyKey = (jobId: number) => ["picky", "seeker", "company", jobId];
export const seekerApi = {
  workspace: () => invoke<SeekerWorkspace>("picky_seeker"),
  folder: (folder: string) => invoke<SeekerWorkspace>("picky_seeker_folder", { folder }),
  newDraft: (lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_new", { lang }),
  save: (draft: SeekerDraft) => invoke<SeekerWorkspace>("picky_seeker_save", { draft }),
  deleteBlocks: (ids: string[], lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_delete_blocks", { ids, lang }),
  useJob: (jobId: number | null, lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_use_job", { jobId, lang }),
  knowledge: () => invoke<SeekerKnowledge>("picky_seeker_knowledge"),
  knowledgeAdd: (part: "knowledge" | "cv" = "knowledge") => invoke<SeekerKnowledge>("picky_seeker_knowledge_add", { part }),
  files: () => invoke<SeekerFiles>("picky_seeker_files"),
  cvText: (path: string) => invoke<string>("picky_seeker_cv_text", { path }),
  jobCompany: (jobId: number) => invoke<SeekerJobCompany>("picky_seeker_job_company", { jobId }),
  jobCompanySet: (jobId: number, folder: string | null) => invoke<SeekerJobCompany>("picky_seeker_job_company_set", { jobId, folder }),
  recordSave: (record: SeekerRecord) => invoke<SeekerWorkspace>("picky_seeker_record_save", { record }),
  recordDelete: (id: string) => invoke<SeekerWorkspace>("picky_seeker_record_delete", { id }),
  settings: (settings: SeekerSettings) => invoke<SeekerWorkspace>("picky_seeker_settings", { settings }),
  previewImport: () => invoke<SeekerImport>("picky_seeker_import_preview"),
  import: (records: SeekerRecord[], remove: string[]) => invoke<SeekerWorkspace>("picky_seeker_import_confirm", { records, remove }),
  assemble: (selections: SeekerSelection[], replace: boolean, lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_assemble", { selections, replace, lang }),
  sources: (lang: SeekerLang) => invoke<SourceChange[]>("picky_seeker_sources", { lang }),
  sourceApply: (change: SourceChange, lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_source_apply", { change, lang }),
  confirm: (label: string, lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_confirm", { label, lang }),
  restore: (versionId: number) => invoke<SeekerWorkspace>("picky_seeker_restore", { versionId }),
  reorder: (kind: string, parent: string | null, ids: string[], lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_reorder", { kind, parent, ids, lang }),
  apply: (before: SeekerDraft, after: SeekerDraft) => invoke<SeekerWorkspace>("picky_seeker_apply", { before, after }),
  /** kind: select · review · polish · summary (English: rewrite the Summary for the JD) · material (recordId). */
  suggest: (kind: string, request: string, lang: SeekerLang, recordId: string | null = null) => invoke<SeekerSuggestion>("picky_seeker_suggest", { kind, request, recordId, lang }),
  templateRead: (path: string) => invoke<(string | null)[]>("picky_seeker_template_read", { path }),
  template: (template: SeekerTemplate | null, lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_template", { template, lang }),
  outputSave: (directory: string, filename: string, jobId: number | null, lang: SeekerLang) => invoke<{ workspace: SeekerWorkspace; export: SeekerExport }>("picky_seeker_output_save", { directory, filename, jobId, lang }),
  jobLink: (exportId: string, jobId: number) => invoke<SeekerWorkspace>("picky_seeker_job_link", { exportId, jobId }),
  exportRestore: (exportId: string) => invoke<SeekerWorkspace>("picky_seeker_export_restore", { exportId }),
};
