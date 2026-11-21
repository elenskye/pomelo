import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";
import { LINKS_KEY } from "@/shared/links/links";
import type { CollectionApi, Item, ItemPatch } from "./types";

/**
 * Adapter for Rust `<prefix>_list/_create/_update/_delete` commands whose rows use
 * `creatorField` (author/director; null: none, 玩物有志) instead of the normalized `creator`, and
 * `rereadField` (玩物有志's replays) instead of `rereads`.
 */
export function commandApi(prefix: string, creatorField: string | null, rereadField = "rereads"): CollectionApi {
  const toItem = (row: Record<string, unknown>): Item => {
    const { [creatorField ?? "\u0000"]: creator, [rereadField]: rereads, ...rest } = row;
    return { ...rest, creator: creator ?? null, rereads: rereads ?? [] } as Item;
  };
  const toPatch = (p: ItemPatch) => {
    const { creator, rereads, ...rest } = p;
    const out: Record<string, unknown> = { ...rest };
    if (creator !== undefined && creatorField) out[creatorField] = creator;
    if (rereads !== undefined) out[rereadField] = rereads;
    return out;
  };
  return {
    key: [prefix],
    list: async () => (await invoke<Record<string, unknown>[]>(`${prefix}_list`)).map(toItem),
    create: (patch) => invoke(`${prefix}_create`, { patch: toPatch(patch) }),
    update: ({ id, patch }) => invoke(`${prefix}_update`, { id, patch: toPatch(patch) }),
    remove: (id) => invoke(`${prefix}_delete`, { id }),
  };
}

export const useItems = (api: CollectionApi) => useQuery({ queryKey: api.key, queryFn: api.list });

export function useItemMutations(api: CollectionApi) {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: api.key });
    void qc.invalidateQueries({ queryKey: KINDS_KEY }); // per-kind counts
    // what links name; a rename rewrites the notes and articles linking to it (links.rs)
    for (const k of [LINKS_KEY, ["notes"], ["articles"]]) void qc.invalidateQueries({ queryKey: k });
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    /** Fire-and-forget inline edits (table cells): errors surface as toasts. */
    quickUpdate: useMutation({ mutationFn: api.update, onSuccess, onError }),
    remove: useMutation({ mutationFn: api.remove, onSuccess }),
  };
}
