// Typed wrappers for Rust packing_* commands (src-tauri/src/modules/packing.rs) + React Query hooks.
import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { errorText, useToast } from "@/shared/feedback";
import { KINDS_KEY } from "@/shared/kinds/api";

export type Template = {
  id: number;
  name: string;
  note: string | null;
  sortOrder: number;
  itemCount: number;
  createdAt: string;
  updatedAt: string;
};

export type Trip = {
  id: number;
  name: string;
  templateId: number | null;
  templateName: string | null;
  departOn: string | null; // YYYY-MM-DD
  note: string | null;
  total: number;
  packed: number;
  createdAt: string;
  updatedAt: string;
};

/** Template item or trip item (`packed` only on trip items). */
export type PackItem = {
  id: number;
  parentId: number;
  kindId: number | null; // 类别 (kinds module "packing")
  name: string;
  note: string | null;
  packed?: boolean;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
};

export type ListKind = "template" | "trip";
export type ItemPatch = { name?: string; kindId?: number | null; note?: string | null; packed?: boolean };
type NamePatch = { name?: string; note?: string | null };
export type TripPatch = NamePatch & { departOn?: string | null };

const parentArg = (list: ListKind, parentId: number) => (list === "template" ? { templateId: parentId } : { tripId: parentId });

const api = {
  templates: () => invoke<Template[]>("packing_templates"),
  createTemplate: (patch: NamePatch) => invoke<Template>("packing_template_create", { patch }),
  updateTemplate: (v: { id: number; patch: NamePatch }) => invoke<Template>("packing_template_update", v),
  deleteTemplate: (id: number) => invoke<void>("packing_template_delete", { id }),
  trips: () => invoke<Trip[]>("packing_trips"),
  createTrip: (v: { patch: TripPatch; templateId: number | null }) => invoke<Trip>("packing_trip_create", v),
  updateTrip: (v: { id: number; patch: TripPatch }) => invoke<Trip>("packing_trip_update", v),
  deleteTrip: (id: number) => invoke<void>("packing_trip_delete", { id }),
  items: (list: ListKind, parentId: number) => invoke<PackItem[]>(`packing_${list}_items`, parentArg(list, parentId)),
  createItem: (v: { list: ListKind; parentId: number; patch: ItemPatch }) =>
    invoke<PackItem>(`packing_${v.list}_item_create`, { ...parentArg(v.list, v.parentId), patch: v.patch }),
  updateItem: (v: { list: ListKind; id: number; patch: ItemPatch }) =>
    invoke<PackItem>(`packing_${v.list}_item_update`, { id: v.id, patch: v.patch }),
  deleteItem: (v: { list: ListKind; id: number }) => invoke<void>(`packing_${v.list}_item_delete`, { id: v.id }),
};

export const PACKING_KEY = ["packing"];
export const useTemplates = () => useQuery({ queryKey: [...PACKING_KEY, "templates"], queryFn: api.templates });
export const useTrips = () => useQuery({ queryKey: [...PACKING_KEY, "trips"], queryFn: api.trips });
export const usePackItems = (list: ListKind, parentId: number | null) =>
  useQuery({
    queryKey: [...PACKING_KEY, "items", list, parentId],
    queryFn: () => api.items(list, parentId!),
    enabled: parentId !== null,
  });

export function usePackingMutations() {
  const qc = useQueryClient();
  const toast = useToast();
  const onSuccess = () => {
    void qc.invalidateQueries({ queryKey: PACKING_KEY }); // lists + counts
    void qc.invalidateQueries({ queryKey: KINDS_KEY });
  };
  const onError = (e: unknown) => toast(errorText(e), "error");
  return {
    createTemplate: useMutation({ mutationFn: api.createTemplate, onSuccess }),
    updateTemplate: useMutation({ mutationFn: api.updateTemplate, onSuccess }),
    deleteTemplate: useMutation({ mutationFn: api.deleteTemplate, onSuccess }),
    createTrip: useMutation({ mutationFn: api.createTrip, onSuccess }),
    updateTrip: useMutation({ mutationFn: api.updateTrip, onSuccess }),
    deleteTrip: useMutation({ mutationFn: api.deleteTrip, onSuccess }),
    createItem: useMutation({ mutationFn: api.createItem, onSuccess }),
    updateItem: useMutation({ mutationFn: api.updateItem, onSuccess }),
    /** Fire-and-forget (checkbox toggles, row deletes): errors surface as toasts. */
    quickUpdateItem: useMutation({ mutationFn: api.updateItem, onSuccess, onError }),
    quickDeleteItem: useMutation({ mutationFn: api.deleteItem, onSuccess, onError }),
  };
}

/** "2026-10-01" → "10 月 1 日"（not this year: "2025 年 10 月 1 日"）. */
export function formatDay(d: string | null): string | null {
  if (!d) return null;
  const [y, m, day] = d.split("-").map(Number);
  const md = `${m} 月 ${day} 日`;
  return y === new Date().getFullYear() ? md : `${y} 年 ${md}`;
}
