// Typed wrappers for Rust articles_* / article_* commands (src-tauri/src/modules/articles.rs) + React Query hooks.
// Titles, tags, previews, the trash and the locking rules are Rust's; the page only shows and asks.
import { invoke } from "@tauri-apps/api/core";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { FileInfo } from "@/shared/attachments/api";
import { errorText, useToast } from "@/shared/feedback";
import type { TagCount } from "@/modules/notes/api";
import { linksChanged, type Backlink } from "@/shared/links/links";

export type View = "all" | "pinned" | "untagged" | "archived" | "trash" | "locked";
export type Summary = {
  id: number;
  title: string;
  preview: string;
  pinned: boolean;
  archived: boolean;
  trashed: boolean;
  locked: boolean;
  tags: string[];
  /** The first picture the text shows (attachment id). */
  thumb: number | null;
  createdAt: string;
  updatedAt: string;
};
export type Link = { id: number; title: string };
export type Article = {
  id: number;
  title: string;
  /** '' while locked and not opened. */
  content: string;
  pinned: boolean;
  archived: boolean;
  trashedAt: string | null;
  locked: boolean;
  /** The content is here (not locked, or 芝麻开门 is open). */
  open: boolean;
  tags: string[];
  attachments: FileInfo[];
  /** 柚子链接: articles and notes linking here, newest first (links.rs). */
  backlinks: Backlink[];
  createdAt: string;
  updatedAt: string;
};
export type Filter = { view?: View; tag?: string; q?: string; id?: number };
export type Patch = { content?: string; pinned?: boolean; archived?: boolean; attachments?: number[] };
export type Counts = Record<View, number>;
export type Overview = { counts: Counts; tags: TagCount[]; pinned: string[] };

export type ExportTarget = { ids?: number[]; tag?: string; all?: boolean };
export type ExportFormat = "md" | "pdf" | "docx";
export type ExportStyle = "pomelo" | "print";
export type ExportOptions = { format: ExportFormat; style: ExportStyle; toc: boolean; header: boolean; meta: boolean; dest: string };
export type Left = { title: string; what: string };
export type ExportReport = { written: number; folder: string; file: string | null; left: Left[] };
export type ImportReport = { imported: number; files: number; tags: number; left: Left[]; fixes: string[] };
export type BearPreview = { notes: number; archived: number; trashed: number; locked: number; tags: number; files: number; here: number };

export const articlesApi = {
  list: (filter: Filter) => invoke<Summary[]>("articles_list", { filter }),
  /** 意思相近 below the word results (search/query.rs). */
  similar: (filter: Filter) => invoke<Summary[]>("search_articles_similar", { filter }),
  overview: () => invoke<Overview>("articles_overview"),
  titles: () => invoke<Link[]>("articles_titles"),
  get: (id: number) => invoke<Article>("article_get", { id }),
  create: (patch: Patch) => invoke<Article>("article_create", { patch }),
  update: (v: { id: number; patch: Patch }) => invoke<Article>("article_update", v),
  trash: (id: number) => invoke<void>("article_trash", { id }),
  restore: (id: number) => invoke<void>("article_restore", { id }),
  remove: (id: number) => invoke<void>("article_delete", { id }),
  emptyTrash: () => invoke<number>("articles_empty_trash"),
  lock: (v: { id: number; lock: boolean }) => invoke<Article>("article_lock", v),
  renameTag: (v: { from: string; to: string }) => invoke<number>("articles_tag_rename", v),
  setTag: (v: { tag: string; patch: { pinned?: boolean; icon?: string | null } }) => invoke<void>("articles_tag_set", v),
  placeTag: (v: { tag: string; parent: string | null; order: string[] }) => invoke<number>("articles_tag_place", v),
  orderPins: (v: { order: string[] }) => invoke<void>("articles_tag_pins", v),
  /** → [articles changed or moved to the trash, 0]. */
  deleteTag: (v: { tag: string; notes: boolean }) => invoke<[number, number]>("articles_tag_delete", { tag: v.tag, articles: v.notes }),
  /** articles/export.rs: Markdown here, PDF / Word through the print page (print.rs). */
  export: (v: { target: ExportTarget; options: ExportOptions }) => invoke<ExportReport>("articles_export", v),
  exportDir: () => invoke<string>("articles_export_dir"),
  reveal: (path: string) => invoke<void>("articles_export_reveal", { path }),
  /** articles/import.rs: Bear (once, read only) and Markdown / TextBundle files and folders. */
  bearPreview: () => invoke<BearPreview>("articles_bear_preview"),
  bearImport: () => invoke<ImportReport>("articles_bear_import"),
  importFiles: (paths: string[]) => invoke<ImportReport>("articles_import_files", { paths }),
  /** 柚子 润色 / 续写 (youzi/assist.rs): proposed text only; the page shows it on a card first. */
  assist: (v: { id: number; mode: "polish" | "continue"; text: string; after?: string | null }) => invoke<{ text: string }>("article_assist", v),
};

export const ARTICLES_KEY = ["articles"];
export const useArticles = (filter: Filter) => useQuery({ queryKey: [...ARTICLES_KEY, "list", filter], queryFn: () => articlesApi.list(filter), placeholderData: keepPreviousData });
export const useSimilarArticles = (filter: Filter, enabled: boolean) =>
  useQuery({ queryKey: [...ARTICLES_KEY, "similar", filter], queryFn: () => articlesApi.similar(filter), enabled, retry: false, staleTime: 30_000 });
export const useArticlesOverview = (enabled = true) => useQuery({ queryKey: [...ARTICLES_KEY, "overview"], queryFn: articlesApi.overview, enabled });
export const useTitles = () => useQuery({ queryKey: [...ARTICLES_KEY, "titles"], queryFn: articlesApi.titles });
export const useArticle = (id: number | null) => useQuery({ queryKey: [...ARTICLES_KEY, "one", id], queryFn: () => articlesApi.get(id!), enabled: id !== null, gcTime: 0 });

export function useArticleMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: ARTICLES_KEY });
    linksChanged(qc);
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    trash: useMutation({ mutationFn: articlesApi.trash, onSuccess, onError }),
    restore: useMutation({ mutationFn: articlesApi.restore, onSuccess, onError }),
    remove: useMutation({ mutationFn: articlesApi.remove, onSuccess, onError }),
    emptyTrash: useMutation({ mutationFn: articlesApi.emptyTrash, onSuccess, onError }),
    update: useMutation({ mutationFn: articlesApi.update, onSuccess, onError }),
    lock: useMutation({ mutationFn: articlesApi.lock, onSuccess, onError }),
    renameTag: useMutation({ mutationFn: articlesApi.renameTag, onSuccess }),
    setTag: useMutation({ mutationFn: articlesApi.setTag, onSuccess, onError }),
    placeTag: useMutation({ mutationFn: articlesApi.placeTag, onSuccess, onError }),
    orderPins: useMutation({ mutationFn: articlesApi.orderPins, onSuccess, onError }),
    deleteTag: useMutation({ mutationFn: articlesApi.deleteTag, onSuccess }),
  };
}

const pad = (n: number) => String(n).padStart(2, "0");
/** The list's date: this year「11 / 06」, before「2025 / 11 / 06」. */
export function listDate(stamp: string): string {
  const d = new Date(stamp);
  const md = `${pad(d.getMonth() + 1)} / ${pad(d.getDate())}`;
  return d.getFullYear() === new Date().getFullYear() ? md : `${d.getFullYear()} / ${md}`;
}
