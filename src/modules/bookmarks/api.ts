// 一网打尽 (Rust modules/bookmarks.rs, icons.rs): typed wrappers + React Query hooks. The rules (what may go
// where, the order, the URL check) live in Rust; the page builds the tree from the flat list.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";

export type Bookmark = {
  id: number;
  /** null: the top level. */
  parentId: number | null;
  kind: "folder" | "link";
  title: string;
  /** "" for a folder. */
  url: string;
  onHome: boolean;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};
export type NewBookmark = { parentId: number | null; kind: Bookmark["kind"]; title: string; url?: string; onHome?: boolean };
export type BookmarkPatch = Partial<{ title: string; url: string; onHome: boolean }>;

const api = {
  list: () => invoke<Bookmark[]>("bookmarks_list"),
  create: (input: NewBookmark) => invoke<Bookmark>("bookmarks_create", { input }),
  update: (v: { id: number; patch: BookmarkPatch }) => invoke<Bookmark>("bookmarks_update", v),
  /** Puts `ids` into `parentId` (null: the top level) before `before` (one of its rows), or last. */
  place: (v: { ids: number[]; parentId: number | null; before: number | null }) => invoke<void>("bookmarks_place", v),
  remove: (ids: number[]) => invoke<number>("bookmarks_delete", { ids }),
  icons: () => invoke<Record<string, string>>("bookmarks_icons"),
};

export const BOOKMARKS_KEY = ["bookmarks"];
const ICONS_KEY = [...BOOKMARKS_KEY, "icons"];
export const useBookmarks = () => useQuery({ queryKey: [...BOOKMARKS_KEY, "list"], queryFn: api.list });

/** Site icons (file name of the host → data URL); asking also starts fetching the missing ones (the real app
 *  only), and Rust says when more arrived. */
export function useSiteIcons(): Record<string, string> {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ICONS_KEY, queryFn: api.icons });
  useEffect(() => {
    const off = listen("bookmark-icons", () => void qc.invalidateQueries({ queryKey: ICONS_KEY }));
    return () => void off.then((f) => f());
  }, [qc]);
  return q.data ?? {};
}

export function useBookmarkMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => void qc.invalidateQueries({ queryKey: BOOKMARKS_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess, onError }),
    update: useMutation({ mutationFn: api.update, onSuccess, onError }),
    place: useMutation({ mutationFn: api.place, onSuccess, onError }),
    remove: useMutation({ mutationFn: api.remove, onSuccess, onError }),
  };
}

/** The host of an address, as Rust's host_of (lowercase, no user or port). */
export function hostOf(url: string): string {
  const rest = url.includes("://") ? url.slice(url.indexOf("://") + 3) : url;
  const authority = rest.split(/[/?#]/)[0] ?? "";
  const host = authority.slice(authority.lastIndexOf("@") + 1);
  return (host.startsWith("[") ? host.slice(0, host.indexOf("]") + 1) : host.split(":")[0]).toLowerCase();
}

/** The icon's key for a host (icons.rs file_stem). */
export const iconKey = (host: string) => host.replace(/[^a-z0-9.-]/gi, "_");

export type Node = Bookmark & { kids: Node[] };

/** The tree: top-level rows, every node by id; siblings in their order. */
export function buildTree(list: Bookmark[]): { roots: Node[]; byId: Map<number, Node> } {
  const byId = new Map<number, Node>(list.map((b) => [b.id, { ...b, kids: [] }]));
  const roots: Node[] = [];
  for (const n of byId.values()) (n.parentId != null ? (byId.get(n.parentId)?.kids ?? roots) : roots).push(n);
  const sort = (ns: Node[]) => {
    ns.sort((a, b) => a.sortOrder - b.sortOrder || a.id - b.id);
    ns.forEach((n) => sort(n.kids));
  };
  sort(roots);
  return { roots, byId };
}

/** The folders from the top down to `id` (itself included when it's a folder). */
export function pathTo(byId: Map<number, Node>, id: number | null): Node[] {
  const out: Node[] = [];
  for (let at = id == null ? undefined : byId.get(id); at; at = at.parentId == null ? undefined : byId.get(at.parentId)) if (at.kind === "folder") out.unshift(at);
  return out;
}

/** Links inside (all the way down). */
export const linksIn = (n: Node): number => n.kids.reduce((s, k) => s + (k.kind === "link" ? 1 : linksIn(k)), 0);
