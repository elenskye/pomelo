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
/** ds: a Design System · motion: a scheme's Motion Deck · canvas: an Explore Deck (boards on pages) · page: a Questionnaire */
export type Kind = "ds" | "motion" | "canvas" | "page";
export type CanvasRow = { name: string; title: string; kind: Kind; sub: string; boards: number; pages: Page[] };
/** The bench's four lists, always in this order: Design System, Motion Deck, Explore Deck, Questionnaire. */
export type Section = { id: "ds" | "motion" | "explore" | "questionnaire"; title: string; items: CanvasRow[] };
export type Board = { file: string; title: string; page: string | null };
/** After a rename, delete or undo: `id` is what the undo of a delete needs (the bench's trash). */
export type Changed = { id: string | null; canvas: string; page: string | null; title: string };
export type Detail = { name: string; pages: Page[]; boards: Board[] };

export const canvasApi = {
  overview: () => invoke<Overview>("canvas_overview"),
  start: () => invoke<Overview>("canvas_start"),
  stop: () => invoke<Overview>("canvas_stop"),
  list: () => invoke<Section[]>("canvas_list"),
  boards: (name: string) => invoke<Detail>("canvas_boards", { name }),
  /** "" = the default folder */
  setFolder: (folder: string) => invoke<Overview>("canvas_set_folder", { folder }),
  /** New title for a canvas (or a page of it, with `page`). */
  rename: (name: string, title: string, page: string | null = null) => invoke<Changed>("canvas_rename", { name, title, page }),
  /** Into visual-atelier's trash; `restore(id)` puts it back. */
  remove: (name: string, page: string | null = null) => invoke<Changed>("canvas_delete", { name, page }),
  restore: (id: string) => invoke<Changed>("canvas_restore", { id }),
};

export const CANVAS_KEY = ["canvas"];
export const OVERVIEW_KEY = [...CANVAS_KEY, "overview"];

/** Asked again every few seconds: a bench started or stopped elsewhere shows up by itself. */
export const useCanvasOverview = () => useQuery({ queryKey: OVERVIEW_KEY, queryFn: canvasApi.overview, refetchInterval: 4000 });
export const useCanvasList = (running: boolean) => useQuery({ queryKey: [...CANVAS_KEY, "list"], queryFn: canvasApi.list, enabled: running });
export const useCanvasBoards = (running: boolean, name: string | null) => useQuery({ queryKey: [...CANVAS_KEY, "boards", name], queryFn: () => canvasApi.boards(name!), enabled: running && !!name });

/** Where a thing is in the bench's UI (a canvas: a page of it, one board full size); a 问卷 page is its own page. */
export function addressOf(base: string, row: { name: string; kind: string }, at: { page?: string | null; board?: string | null } = {}) {
  if (row.kind === "page") return `${base}/c/${row.name}/`;
  if (row.kind === "ds") return `${base}/#/ds/${row.name}`;
  if (row.kind === "motion") return `${base}/#/motion/${row.name}`;
  const p = new URLSearchParams();
  if (at.page) p.set("page", at.page);
  if (at.board) p.set("board", at.board);
  const q = p.toString();
  return `${base}/#/${row.name}${q ? `?${q}` : ""}`;
}
