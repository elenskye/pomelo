// Typed wrappers for Rust books_* commands (src-tauri/src/modules/books.rs) + React Query hooks.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import type { StatusCode, StatusLabels } from "@/shared/status";

export type Book = {
  id: number;
  title: string;
  author: string | null;
  country: string | null;
  status: StatusCode;
  rating: number | null;
  finishedMonth: string | null;
  note: string | null;
  createdAt: string;
  updatedAt: string;
};

/** Omitted key = unchanged; null = clear. Rust auto-fills finishedMonth on → done only if the key is omitted. */
export type BookPatch = Partial<Omit<Book, "id" | "createdAt" | "updatedAt">>;

export const BOOK_STATUS: StatusLabels = { unmarked: "待标记", todo: "想读", doing: "在读", done: "已读" };

const api = {
  list: () => invoke<Book[]>("books_list"),
  create: (patch: BookPatch) => invoke<Book>("books_create", { patch }),
  update: (v: { id: number; patch: BookPatch }) => invoke<Book>("books_update", v),
  remove: (id: number) => invoke<void>("books_delete", { id }),
};

export const BOOKS_KEY = ["books"];

export const useBooks = () => useQuery({ queryKey: BOOKS_KEY, queryFn: api.list });

export function useBookMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => qc.invalidateQueries({ queryKey: BOOKS_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    /** Fire-and-forget inline edits (table cells): errors surface as toasts. */
    quickUpdate: useMutation({ mutationFn: api.update, onSuccess, onError }),
    remove: useMutation({ mutationFn: api.remove, onSuccess }),
  };
}
