// Typed wrappers for Rust site_* commands (src-tauri/src/site/) + React Query hooks. Git, the check, the
// push and Vercel's result all happen in Rust; the page polls the overview while a publish runs.
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type Problem = { kind: "no-sandbox" | "missing" | "not-git" | "not-main" | "git-busy" | "no-remote" | "real-remote"; text: string };
export type ServerView = { state: "stopped" | "starting" | "running"; ours: boolean; url: string; error: string | null };
export type Job = {
  id: number;
  step: "check" | "push" | "deploy";
  state: "running" | "done" | "failed";
  /** unix ms the step began */
  since: number;
  subject: string;
  sha: string | null;
  error: string | null;
  log: string[];
  deployUrl: string | null;
  /** Vercel never said */
  unknown: boolean;
};
export type Overview = {
  mode: "real" | "sandbox";
  folder: string;
  isDefault: boolean;
  problem: Problem | null;
  siteUrl: string | null;
  githubUrl: string | null;
  server: ServerView;
  job: Job | null;
};
export type Change = {
  path: string;
  from: string | null;
  action: "add" | "modify" | "delete" | "rename";
  kind: "content" | "code";
  label: string;
  size: number | null;
  blocked: string | null;
};
export type Commit = { sha: string; subject: string; time: number };
export type Changes = { files: Change[]; ahead: Commit[]; token: string };
export type DiffLine = { t: "ctx" | "add" | "del" | "hunk"; s: string };
export type Diff =
  | { type: "text"; lines: DiffLine[]; truncated: boolean }
  | { type: "image"; before: string | null; after: string | null }
  | { type: "binary"; before: number | null; after: number | null };
export type Published = { sha: string; subject: string; time: number; state: "success" | "pending" | "failure" | "error" | null; url: string | null };
export type History = { items: Published[]; vercelUrl: string | null };

export const siteApi = {
  overview: () => invoke<Overview>("site_overview"),
  changes: () => invoke<Changes>("site_changes"),
  diff: (path: string) => invoke<Diff>("site_diff", { path }),
  /** fetch + fast-forward, then the list to confirm */
  prepare: () => invoke<Changes>("site_prepare"),
  publish: (message: string, token: string) => invoke<Overview>("site_publish", { message, token }),
  dismiss: () => invoke<Overview>("site_dismiss"),
  history: () => invoke<History>("site_history"),
  /** waits until the page answers */
  serverStart: () => invoke<ServerView>("site_server_start"),
  serverStop: () => invoke<ServerView>("site_server_stop"),
  sandbox: () => invoke<Overview>("site_sandbox"),
  setFolder: (folder: string | null) => invoke<Overview>("site_set_folder", { folder }),
};

export const SITE_KEY = ["site"];
export const OVERVIEW_KEY = [...SITE_KEY, "overview"];

export function useOverview() {
  return useQuery({
    queryKey: OVERVIEW_KEY,
    queryFn: siteApi.overview,
    refetchIntervalInBackground: true,
    refetchInterval: (q) => (q.state.data?.job?.state === "running" || q.state.data?.server.state === "starting" ? 1000 : 5000),
  });
}

export const useChanges = (enabled: boolean) => useQuery({ queryKey: [...SITE_KEY, "changes"], queryFn: siteApi.changes, enabled, refetchOnWindowFocus: true });
export const useHistory = (enabled: boolean) => useQuery({ queryKey: [...SITE_KEY, "history"], queryFn: siteApi.history, enabled });

export const ACTION_LABEL: Record<Change["action"], string> = { add: "新增", modify: "修改", delete: "删除", rename: "改名" };

export function sizeText(n: number | null) {
  if (n === null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** 10/20 21:05 (this year), else 2025/10/20 */
export function whenText(unix: number) {
  const d = new Date(unix * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  const day = d.getFullYear() === new Date().getFullYear() ? `${p(d.getMonth() + 1)}/${p(d.getDate())}` : `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())}`;
  return `${day} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

// ---------- 作品 (site/wander.rs) ----------

export type Lang = "zh" | "en";
export type Term = { slug: string; zh: string; en: string };
export type Taxonomy = { categories: Term[]; topics: Term[] };
export type WorkRow = { slug: string; title: string; month: string; category: string; topics: string[]; langs: Lang[] };
export type Works = { works: WorkRow[]; taxonomy: Taxonomy; broken: string[] };
export type Version = { title: string; summary: string; body: string; englishTitle: string; englishSummary: string };
/** `image`: the stored path ("public/assets/images/…"). */
export type GalleryItem = { image: string; altZh: string; captionZh: string; altEn: string; captionEn: string };
export type Prints = { zh: string | null; en: string | null };
export type Work = { slug: string; category: string; topics: string[]; month: string; cover: string | null; gallery: GalleryItem[]; zh: Version | null; en: Version | null; prints: Prints };
export type AssetKind = "image" | "document";
/** `alt`: an image's Alt or a document's description. */
export type Asset = { slug: string; title: string; path: string; url: string; alt: string; usage: string };

export const workApi = {
  list: () => invoke<Works>("site_works"),
  load: (slug: string) => invoke<Work>("site_work", { slug }),
  save: (work: Work) => invoke<Work>("site_work_save", { work }),
  create: (slug: string, lang: Lang, title: string) => invoke<Work>("site_work_create", { slug, lang, title }),
  addVersion: (slug: string, lang: Lang, prints: Prints) => invoke<Work>("site_work_add_version", { slug, lang, prints }),
  rename: (slug: string, to: string, prints: Prints) => invoke<Work>("site_work_rename", { slug, to, prints }),
  remove: (slug: string, prints: Prints) => invoke<void>("site_work_delete", { slug, prints }),
  /** Files into the library (registered) by where they're added (wander:<slug> | now | about | career | portraits |
   * artworks | cv:zh | cv:en | asset:image | asset:document) → their URLs ("/assets/images/x.jpg"). */
  addFiles: (target: string, paths: string[]) => invoke<string[]>("site_files_add", { target, paths }),
  addFileTo: async (target: string, file: File) =>
    invoke<string>("site_image_bytes", new Uint8Array(await file.arrayBuffer()), { headers: { "x-name": encodeURIComponent(file.name || "image.png"), "x-target": encodeURIComponent(target) } }),
  assets: (kind: AssetKind) => invoke<Asset[]>("site_assets", { kind }),
  preview: (path: string) => invoke<void>("site_preview", { path }),
};

export const WORKS_KEY = [...SITE_KEY, "works"];
export const useWorks = (enabled: boolean) => useQuery({ queryKey: WORKS_KEY, queryFn: workApi.list, enabled, refetchOnWindowFocus: true });
/** Read afresh each time a picker opens (the library changes from other pages). */
export const useAssets = (enabled: boolean, kind: AssetKind = "image") => useQuery({ queryKey: [...SITE_KEY, "assets", kind], queryFn: () => workApi.assets(kind), enabled, staleTime: 0, refetchOnMount: "always" });

/** A stored path ("public/…") or site URL ("/…") → where the page loads it (lib.rs serves the site's public/). */
export function siteSrc(stored: string): string {
  const url = stored.startsWith("public/") ? stored.slice("public".length) : stored;
  const mock = (window as unknown as { __pomeloSiteSrc?: (url: string) => string }).__pomeloSiteSrc;
  return mock ? mock(url) : `pomelo-site://localhost${encodeURI(url)}`;
}

/** The work's page on the site. */
export const pagePath = (slug: string, lang: Lang) => (lang === "en" ? `/en/wander/${slug}/` : `/wander/${slug}/`);

// ---------- 小拾 in the editor (picky/assist.rs, picky/assistants/site.rs) ----------

export type SiteEnglish = { title: string | null; summary: string | null };
/** 按中文起草英文版 (in memory; the page takes it into the English version and clears it). */
export type Draft = { slug: string | null; running: boolean; title: string; summary: string; body: string; status: string | null; error: string | null; modelName: string | null; note: string | null };

export const siteAssist = {
  /** The English stub's title and summary from the Chinese. */
  english: (title: string, summary: string, body: string) => invoke<SiteEnglish>("picky_site_english", { title, summary, body }),
  draft: () => invoke<Draft>("picky_site_draft"),
  draftRun: (slug: string, title: string, summary: string, body: string) => invoke<Draft>("picky_site_draft_run", { slug, title, summary, body }),
  draftStop: () => invoke<Draft>("picky_site_draft_stop"),
  draftClear: () => invoke<Draft>("picky_site_draft_clear"),
};

// ---------- Now、关于我、职业页、资源库、站点 (site/now.rs, about.rs, career.rs, registry.rs, terms.rs) ----------

export type NowRow = { month: string; langs: Lang[] };
export type Status = { zh: string; en: string; print: string | null };
export type NowList = { months: NowRow[]; status: Status };
/** zh / en: the bodies (null: no file in that language). */
export type NowMonth = { month: string; zh: string | null; en: string | null; prints: Prints };

export type Entry = { role: string; org: string; period: string; description: string };
export type AboutVersion = { contextIntro: string; timeline: Entry[]; story: string };
export type AboutImages = { portraits: string[]; artworks: string[]; print: string | null };
export type About = { zh: AboutVersion | null; en: AboutVersion | null; images: AboutImages; prints: Prints };

export type Resume = { label: string; file: string };
export type Simple = { heading: string; items: string };
export type Detailed = { period: string; role: string; org: string; bullets: string[] };
export type Section = { title: string; mode: "simple" | "detailed"; simple: Simple[]; detailed: Detailed[] };
export type CareerVersion = { email: string; summary: string; resumes: Resume[]; sections: Section[]; extra: string };
export type Career = { zh: CareerVersion | null; en: CareerVersion | null; prints: Prints };

/** `size` in bytes; `used`: something on the site refers to it. */
export type RegEntry = { kind: AssetKind; slug: string; title: string; file: string; note: string; usage: string; print: string | null; size: number; used: boolean };
export type Citation = { path: string; role: string | null };

export type TermIn = { slug: string; zh: string; en: string; used: number };
export type Link = { key: string; label: string; href: string };
export type SiteData = { terms: { categories: TermIn[]; topics: TermIn[] }; links: Link[]; termsPrint: string | null; linksPrint: string | null };

export const pageApi = {
  nowList: () => invoke<NowList>("site_now_list"),
  now: (month: string) => invoke<NowMonth>("site_now", { month }),
  nowSave: (month: NowMonth) => invoke<NowMonth>("site_now_save", { month }),
  nowCreate: (month: string, lang: Lang) => invoke<NowMonth>("site_now_create", { month, lang }),
  nowDelete: (month: string, prints: Prints) => invoke<void>("site_now_delete", { month, prints }),
  statusSave: (status: Status) => invoke<Status>("site_status_save", { status }),
  about: () => invoke<About>("site_about"),
  aboutSave: (about: About) => invoke<About>("site_about_save", { about }),
  career: () => invoke<Career>("site_career"),
  careerSave: (career: Career) => invoke<Career>("site_career_save", { career }),
  registry: () => invoke<RegEntry[]>("site_registry"),
  registrySave: (entry: RegEntry) => invoke<RegEntry>("site_registry_save", { entry }),
  citations: (kind: RegEntry["kind"], slug: string) => invoke<Citation[]>("site_registry_citations", { kind, slug }),
  registryDelete: (kind: RegEntry["kind"], slug: string, print: string | null, withFile: boolean) => invoke<void>("site_registry_delete", { kind, slug, print, withFile }),
  unregistered: (kind: AssetKind) => invoke<string[]>("site_registry_unregistered", { kind }),
  /** Registers the library files put there by hand → how many. */
  adopt: (kind: AssetKind) => invoke<number>("site_registry_adopt", { kind }),
  terms: () => invoke<SiteData>("site_terms"),
  termsSave: (terms: SiteData["terms"], print: string | null) => invoke<SiteData>("site_terms_save", { terms, print }),
  linksSave: (links: Link[], print: string | null) => invoke<SiteData>("site_links_save", { links, print }),
};
