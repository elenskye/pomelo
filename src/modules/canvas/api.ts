// Typed wrappers for Rust canvas_* commands (src-tauri/src/canvas.rs) + React Query hooks. 看画布 keeps nothing:
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
export type CanvasRow = { name: string; title: string; kind: "canvas" | "page"; boards: number; pages: Page[] };
export type Board = { file: string; title: string; page: string | null };
export type Detail = { name: string; pages: Page[]; boards: Board[] };

export const canvasApi = {
  overview: () => invoke<Overview>("canvas_overview"),
  start: () => invoke<Overview>("canvas_start"),
  stop: () => invoke<Overview>("canvas_stop"),
  list: () => invoke<CanvasRow[]>("canvas_list"),
  boards: (name: string) => invoke<Detail>("canvas_boards", { name }),
  /** "" = the default folder */
  setFolder: (folder: string) => invoke<Overview>("canvas_set_folder", { folder }),
};

export const CANVAS_KEY = ["canvas"];
export const OVERVIEW_KEY = [...CANVAS_KEY, "overview"];

/** Asked again every few seconds: a bench started or stopped elsewhere shows up by itself. */
export const useCanvasOverview = () => useQuery({ queryKey: OVERVIEW_KEY, queryFn: canvasApi.overview, refetchInterval: 4000 });
export const useCanvasList = (running: boolean) => useQuery({ queryKey: [...CANVAS_KEY, "list"], queryFn: canvasApi.list, enabled: running });
export const useCanvasBoards = (running: boolean, name: string | null) => useQuery({ queryKey: [...CANVAS_KEY, "boards", name], queryFn: () => canvasApi.boards(name!), enabled: running && !!name });

/** Where a canvas (a page of it, one board full size) is in the bench's UI; a 问卷 page is its own page. */
export function addressOf(base: string, row: { name: string; kind: string }, at: { page?: string | null; board?: string | null } = {}) {
  if (row.kind === "page") return `${base}/c/${row.name}/`;
  const p = new URLSearchParams();
  if (at.page) p.set("page", at.page);
  if (at.board) p.set("board", at.board);
  const q = p.toString();
  return `${base}/#/${row.name}${q ? `?${q}` : ""}`;
}
