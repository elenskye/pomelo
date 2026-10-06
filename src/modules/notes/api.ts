// Typed wrappers for Rust note_* commands (src-tauri/src/modules/notes.rs) + React Query hooks.
// Tags are parsed from the text by Rust (the renderer mirrors the rule in ./NoteBody.tsx).
import { invoke } from "@tauri-apps/api/core";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";

export type Brief = { id: number; createdAt: string; excerpt: string };
export type Note = {
  id: number;
  content: string;
  pinned: boolean;
  createdAt: string;
  updatedAt: string;
  tags: string[];
  refs: Brief[];
  backrefs: Brief[];
};
export type NoteFilter = { tag?: string; day?: string; q?: string; id?: number };
export type NotePatch = { content?: string; pinned?: boolean; refs?: number[] };
export type Overview = { total: number; days: { day: string; count: number }[]; tags: { tag: string; count: number }[] };

const api = {
  list: (filter: NoteFilter) => invoke<Note[]>("notes_list", { filter }),
  overview: () => invoke<Overview>("notes_overview"),
  create: (patch: NotePatch) => invoke<Note>("note_create", { patch }),
  update: (v: { id: number; patch: NotePatch }) => invoke<Note>("note_update", v),
  remove: (id: number) => invoke<void>("note_delete", { id }),
  renameTag: (v: { from: string; to: string }) => invoke<number>("notes_tag_rename", v),
};

export const NOTES_KEY = ["notes"];
export const useNotes = (filter: NoteFilter) =>
  useQuery({ queryKey: [...NOTES_KEY, "list", filter], queryFn: () => api.list(filter), placeholderData: keepPreviousData });
export const useOverview = () => useQuery({ queryKey: [...NOTES_KEY, "overview"], queryFn: api.overview });

export function useNoteMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => void qc.invalidateQueries({ queryKey: NOTES_KEY });
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    create: useMutation({ mutationFn: api.create, onSuccess }),
    update: useMutation({ mutationFn: api.update, onSuccess }),
    remove: useMutation({ mutationFn: api.remove, onSuccess, onError }),
    renameTag: useMutation({ mutationFn: api.renameTag, onSuccess }),
  };
}

const pad = (n: number) => String(n).padStart(2, "0");
/** UTC stamp → local "2026 / 02 / 23 14:39". */
export function noteTime(stamp: string): string {
  const d = new Date(stamp);
  return `${d.getFullYear()} / ${pad(d.getMonth() + 1)} / ${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
