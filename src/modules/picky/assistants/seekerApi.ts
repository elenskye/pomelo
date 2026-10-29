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
/** kind: material · profile · fact (category 经历档案: a dossier, `material` / `materialEn` its Chinese / English material) · rule (a title and its points, in order; older text and 个人约定 come as points: Rust catalog.rs `normalize`) · example · application.
 * lang: cn · en · "" (both: rules, 约定, 范例); facts, 档案 and 网申 are both languages' whatever it says. */
export type SeekerRecord = { id: string; kind: string; category: string; title: string; text: string; time: string; role: string; location: string; points: SeekerPoint[]; fields: SeekerHeader[]; stack: string; material: string | null; materialEn?: string | null; lang?: string; source: string | null; revision: number; enabled: boolean };
/** Whether a page in `lang` shows the record (Rust catalog.rs `for_lang`). */
export const recordIn = (r: SeekerRecord, lang: SeekerLang) => ["fact", "application"].includes(r.kind) || r.lang === "" || (r.lang ?? "cn") === lang;
/** lang: the language the page shows ("en"; else Chinese). */
export type SeekerSettings = { selectionModel: string | null; reviewModel: string | null; thinking: boolean; cloudAllowed: boolean; lang?: string };
export type SeekerRevision = { id: number; recordId: string; body: SeekerRecord; savedAt: string };
/** The name a built-in group of 素材 / 规则 shows (Rust catalog.rs `rename_group`); lang "" for 规则. */
export type SeekerGroupName = { kind: "material" | "rule"; lang: string; category: string; label: string };
export type SeekerCatalog = { records: SeekerRecord[]; revisions: SeekerRevision[]; groups: SeekerGroupName[]; settings: SeekerSettings };
/** The name a group shows: the user's for a built-in one, else its own. */
export const groupName = (c: SeekerCatalog | undefined, kind: "material" | "rule", lang: string, category: string) => c?.groups.find((g) => g.kind === kind && g.lang === (kind === "rule" ? "" : lang) && g.category === category)?.label ?? category;
/** One function's 发给模型的说明 (Rust model.rs `PromptView`): the rule groups it reads, its system message. */
export type SeekerPromptView = { key: string; label: string; groups: string[]; system: string };
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
/** A Word export: a record only (Seeker keeps no copy of the résumé); `materials`: what it was made from. */
export type SeekerExport = { id: string; label: string; path: string; lang: SeekerLang; materials: string[]; createdAt: string };
export type SeekerJobSnapshot = { id: number; company: string; position: string | null; location: string | null; link: string | null; note: string | null; createdAt: string };
/** A résumé file a job used (an export's path, or any résumé under cv/); one per job is current (本次使用). */
export type SeekerJobResume = { id: string; path: string; job: SeekerJobSnapshot; jd: string; createdAt: string; current: boolean };
export type SeekerOutput = { template: SeekerTemplate | null; templateEn?: SeekerTemplate | null; exports: SeekerExport[]; links: SeekerJobResume[] };
/** knowledge/ of the Seeker folder: where it is, the 知识库 it is in, its companies (one folder each). */
/** `name`: the user's name for it (设置) or `auto`, the one found in 招聘索引.md or the folder's. */
export type SeekerCompany = { folder: string; name: string; auto: string };
export type SeekerKnowledge = { path: string | null; library: string | null; companies: SeekerCompany[]; error: string | null; cv?: string | null; cvLibrary?: string | null };
/** cv/ and material/ of the Seeker folder (Rust seeker/files.rs); modified: seconds since 1970. */
export type SeekerCvFile = { path: string; name: string; lang: SeekerLang; series: string; version: number | null; modified: number; size: number; template: boolean; export: boolean; exportId: string | null; jobs: string[] };
export type SeekerMaterialFile = { path: string; name: string; group: string; size: number; modified: number };
export type SeekerFiles = { cv: SeekerCvFile[]; material: SeekerMaterialFile[]; transfer: boolean; error: string | null };
export const SEEKER_FILES_KEY = ["picky", "seeker", "files"];
/** A job's résumés still under cv/ (Rust output.rs `job_resumes`) and cv/'s résumés to pick from (templates too). */
export type SeekerLinked = { path: string; name: string; lang: SeekerLang; current: boolean; jd: string; job: SeekerJobSnapshot; createdAt: string; exportId: string | null };
export type SeekerJobResumes = { linked: SeekerLinked[]; files: SeekerCvFile[]; noFolder: boolean; error: string | null };
export const SEEKER_JOB_RESUMES_KEY = ["picky", "seeker", "jobResumes"];
export const seekerJobResumesKey = (jobId: number) => [...SEEKER_JOB_RESUMES_KEY, jobId];
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
  companyNames: (names: Record<string, string>) => invoke<SeekerKnowledge>("picky_seeker_company_names", { names }),
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
  groupRename: (kind: "material" | "rule", lang: string, from: string, to: string) => invoke<SeekerWorkspace>("picky_seeker_group_rename", { kind, lang, from, to }),
  prompts: (lang: SeekerLang) => invoke<SeekerPromptView[]>("picky_seeker_prompts", { lang }),
  reorder: (kind: string, parent: string | null, ids: string[], lang: SeekerLang) => invoke<SeekerWorkspace>("picky_seeker_reorder", { kind, parent, ids, lang }),
  apply: (before: SeekerDraft, after: SeekerDraft) => invoke<SeekerWorkspace>("picky_seeker_apply", { before, after }),
  /** kind: select · review · polish · summary (English: rewrite the Summary for the JD) · material (recordId). */
  suggest: (kind: string, request: string, lang: SeekerLang, recordId: string | null = null) => invoke<SeekerSuggestion>("picky_seeker_suggest", { kind, request, recordId, lang }),
  outputSave: (directory: string, filename: string, jobId: number | null, lang: SeekerLang) => invoke<{ workspace: SeekerWorkspace; export: SeekerExport }>("picky_seeker_output_save", { directory, filename, jobId, lang }),
  /** A résumé under cv/ (by its path) as the one this job uses. */
  jobResume: (jobId: number, path: string) => invoke<SeekerWorkspace>("picky_seeker_job_resume", { jobId, path }),
  jobResumes: (jobId: number) => invoke<SeekerJobResumes>("picky_seeker_job_resumes", { jobId }),
};

/** Several records at once (Rust catalog.rs): the user's order for a group, 启用 / 停用, 删除; each one undo step. */
export const seekerBatch = {
  order: (ids: string[]) => invoke<SeekerWorkspace>("picky_seeker_records_order", { ids }),
  enable: (ids: string[], enabled: boolean) => invoke<SeekerWorkspace>("picky_seeker_records_enable", { ids, enabled }),
  remove: (ids: string[]) => invoke<SeekerWorkspace>("picky_seeker_records_delete", { ids }),
};
