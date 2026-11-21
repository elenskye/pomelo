// Opening one record from elsewhere (全局搜索, 首页): navigate(`/<module>`, { state: { open: hit } }); the
// module page picks it up with useOpenRequest, then the state is cleared so a reload doesn't reopen it.
import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLocation, useNavigate } from "react-router";
import { errorText, type useToast } from "./feedback";
import { api as ipc } from "./ipc";

/** One 全局搜索 result (src-tauri/src/search.rs). */
export type Hit = {
  module: string;
  /** group · goal · task · book · film · course · lesson · trip · template · trip_item · template_item · entry · fixed · account · job · note;
   * 首页 also sends assets (钱来钱往 → 资产, id unused). */
  kind: string;
  id: number;
  title: string;
  detail: string | null;
  /** The containing record to select first (course, trip, template, todo group; todo goal as -id). */
  parent: number | null;
  day: string | null;
  amount: number | null;
  currency: string | null;
  flow: string | null;
};

export const searchAll = (q: string) => invoke<Hit[]>("search_all", { q });
/** 意思相近 (v0.4, search/query.rs): by meaning, may start the embedding service. */
export const searchSimilar = (q: string) => invoke<Hit[]>("search_similar", { q });

export function useOpenRequest(onOpen: (hit: Hit) => void) {
  const location = useLocation();
  const navigate = useNavigate();
  const hit = (location.state as { open?: Hit } | null)?.open;
  useEffect(() => {
    if (!hit) return;
    // cleared first: opening may move to another of the module's pages (`/finance/assets`)
    void navigate(location.pathname, { replace: true, state: null });
    onOpen(hit);
  }, [hit]); // eslint-disable-line react-hooks/exhaustive-deps
}

export const folderOf = (path: string) => path.slice(0, path.lastIndexOf("/")) || "/";
/** Open a file or folder in its app (Finder for a folder); a failure is a toast. */
export const openPath = (path: string, toast: ReturnType<typeof useToast>) => void ipc.openLink(path).catch((e) => toast(errorText(e), "error"));
