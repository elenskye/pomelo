// User-editable 类型 lists (Rust src-tauri/src/modules/kinds.rs). module = "books" | "films" | "drawing".
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

export type Kind = { id: number; module: string; name: string; sortOrder: number; count: number };

const api = {
  list: (module: string) => invoke<Kind[]>("kinds_list", { module }),
  create: (module: string, name: string) => invoke<Kind>("kinds_create", { module, name }),
  rename: (id: number, name: string) => invoke<Kind>("kinds_rename", { id, name }),
  remove: (id: number) => invoke<void>("kinds_delete", { id }),
  reorder: (module: string, ids: number[]) => invoke<Kind[]>("kinds_reorder", { module, ids }),
};

export const KINDS_KEY = ["kinds"];
export const useKinds = (module: string) => useQuery({ queryKey: [...KINDS_KEY, module], queryFn: () => api.list(module) });

/** Kind edits change item labels / 未分类 state, so item queries (`itemsKey`) refresh too. */
export function useKindMutations(module: string, itemsKey: readonly unknown[]) {
  const qc = useQueryClient();
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
    void qc.invalidateQueries({ queryKey: itemsKey });
  };
  return {
    create: useMutation({ mutationFn: (name: string) => api.create(module, name), onSuccess }),
    rename: useMutation({ mutationFn: (v: { id: number; name: string }) => api.rename(v.id, v.name), onSuccess }),
    remove: useMutation({ mutationFn: api.remove, onSuccess }),
    reorder: useMutation({ mutationFn: (ids: number[]) => api.reorder(module, ids), onSuccess }),
  };
}

export const UNCATEGORIZED = "未分类";
