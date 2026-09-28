import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import type { CollectionApi, Item, ItemPatch } from "./types";

/**
 * Adapter for Rust `<prefix>_list/_create/_update/_delete` commands whose rows use
 * `creatorField` (author/director) instead of the normalized `creator`.
 */
export function commandApi(prefix: string, creatorField: string): CollectionApi {
  const toItem = (row: Record<string, unknown>): Item => {
    const { [creatorField]: creator, ...rest } = row;
    return { ...rest, creator } as Item;
  };
  const toPatch = (p: ItemPatch) => {
    const { creator, ...rest } = p;
    return creator === undefined ? rest : { ...rest, [creatorField]: creator };
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
  const onSuccess = () => qc.invalidateQueries({ queryKey: api.key });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    /** Fire-and-forget inline edits (table cells): errors surface as toasts. */
    quickUpdate: useMutation({ mutationFn: api.update, onSuccess, onError }),
    remove: useMutation({ mutationFn: api.remove, onSuccess }),
  };
}
