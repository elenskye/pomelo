// Typed wrappers for Rust canvas_* commands (src-tauri/src/canvas.rs) + React Query hooks. 有板有眼 keeps nothing:
// Rust starts, asks and stops visual-atelier's bench; the page shows the bench's own UI in a frame.
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type Overview = {
  folder: string;
  isDefault: boolean;
  /** why it can't be started */
  problem: string | null;
  state: "stopped" | "starting" | "running";
  /** started by Pomelo */
  ours: boolean;
  /** the bench's address, no slash at the end */
  url: string;
  /** why the last start failed */
  error: string | null;
};
export type Page = { id: string; name: string };
/** ds: a Design System · motion: a scheme's Motion Deck · canvas: an Explore Deck (boards on pages) · page: a Questionnaire ·
 *  checks: `npm run all`'s results */
export type Kind = "ds" | "motion" | "canvas" | "page" | "checks";
/** `schemeTitle`: a Design System or Motion Deck named in the bench — the scheme's own name (null when it has that).
 *  `route`: where the bench's UI shows it (after `#`), for Checks. */
export type CanvasRow = { name: string; title: string; kind: Kind; sub: string; boards: number; pages: Page[]; schemeTitle?: string | null; route?: string | null };
/** The bench's lists, always in this order: Design System, Motion Deck, Explore Deck, Questionnaire, and Checks when the
 *  bench has it (api 5). */
export type Section = { id: "ds" | "motion" | "explore" | "questionnaire" | "checks"; title: string; items: CanvasRow[] };
/** A board's 批准状态 (only marked ones have one; `changed`: the board changed after it was marked). */
export type Review = { status: "proposed" | "approved" | "rejected"; note: string | null; changed: boolean };
export type Board = { file: string; title: string; page: string | null; review?: Review | null };
/** After a rename, delete or undo: `id` is what the undo of a delete needs (the bench's trash); `file` a board. */
type Changed = { id: string | null; canvas: string; page: string | null; file: string | null; title: string };
export type Detail = { name: string; pages: Page[]; boards: Board[] };

export const canvasApi = {
  overview: () => invoke<Overview>("canvas_overview"),
  start: () => invoke<Overview>("canvas_start"),
  stop: () => invoke<Overview>("canvas_stop"),
  list: () => invoke<Section[]>("canvas_list"),
  boards: (name: string) => invoke<Detail>("canvas_boards", { name }),
  /** "" = the default folder */
  setFolder: (folder: string) => invoke<Overview>("canvas_set_folder", { folder }),
  /** New title for a canvas (or a page of it, with `page`; a board, with `board`). */
  rename: (name: string, title: string, page: string | null = null, board: string | null = null) => invoke<Changed>("canvas_rename", { name, title, page, board }),
  /** Into visual-atelier's trash; `restore(id)` puts it back. */
  remove: (name: string, page: string | null = null, board: string | null = null) => invoke<Changed>("canvas_delete", { name, page, board }),
  /** A Design System's or Motion Deck's name in the bench only; null = its own name again. */
  nameScheme: (kind: "ds" | "motion", name: string, title: string | null) => invoke<Changed>("canvas_name", { kind, name, title }),
  restore: (id: string) => invoke<Changed>("canvas_restore", { id }),
};

/** A `reviews` message from the bench (`{ 文件: { status, note, at, by, changed } }`), as Rust's `reviews_of` reads the same. */
export function reviewsOf(v: unknown): Record<string, Review> {
  const out: Record<string, Review> = {};
  if (!v || typeof v !== "object") return out;
  for (const [file, r] of Object.entries(v as Record<string, { status?: unknown; note?: unknown; changed?: unknown } | null>)) {
    const status = r?.status;
    if (status !== "proposed" && status !== "approved" && status !== "rejected") continue;
    const note = typeof r?.note === "string" && r.note.trim() ? r.note.trim() : null;
    out[file] = { status, note, changed: r?.changed === true };
  }
  return out;
}

export const CANVAS_KEY = ["canvas"];
export const OVERVIEW_KEY = [...CANVAS_KEY, "overview"];

/** Asked again every few seconds: a bench started or stopped elsewhere shows up by itself. */
export const useCanvasOverview = () => useQuery({ queryKey: OVERVIEW_KEY, queryFn: canvasApi.overview, refetchInterval: 4000 });
export const useCanvasList = (running: boolean) => useQuery({ queryKey: [...CANVAS_KEY, "list"], queryFn: canvasApi.list, enabled: running });
export const useCanvasBoards = (running: boolean, name: string | null) => useQuery({ queryKey: [...CANVAS_KEY, "boards", name], queryFn: () => canvasApi.boards(name!), enabled: running && !!name });

/** Where a thing is in the bench's UI (a canvas: a page of it, one board full size; Checks: its route); a 问卷 page is its
 *  own page. */
export function addressOf(base: string, row: { name: string; kind: string; route?: string | null }, at: { page?: string | null; board?: string | null } = {}) {
  if (row.kind === "page") return `${base}/c/${row.name}/`;
  if (row.kind === "ds") return `${base}/#/ds/${row.name}`;
  if (row.kind === "motion") return `${base}/#/motion/${row.name}`;
  if (row.kind === "checks") return `${base}/#${row.route ?? "/qa/all"}`;
  const p = new URLSearchParams();
  if (at.page) p.set("page", at.page);
  if (at.board) p.set("board", at.board);
  const q = p.toString();
  return `${base}/#/${row.name}${q ? `?${q}` : ""}`;
}
