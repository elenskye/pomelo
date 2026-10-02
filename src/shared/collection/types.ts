// Shared "collection" UI (books, films): a list of titled items with creator/country/kind/status/rating.
// Each module supplies a CollectionConfig (labels) and a CollectionApi (Rust commands adapter).
// 类型 lists are user-editable (shared/kinds, keyed by moduleId).
import type { StatusCode, StatusLabels } from "@/shared/status";

/** Normalized item; module adapters map `creator` ↔ author/director. */
export type Item = {
  id: number;
  title: string;
  creator: string | null;
  country: string | null;
  kindId: number | null; // kinds.id; null = 未分类
  status: StatusCode;
  rating: number | null;
  finishedMonth: string | null;
  note: string | null;
  createdAt: string;
  updatedAt: string;
};

/** Omitted key = unchanged; null = clear. Rust auto-fills finishedMonth on → done only if the key is omitted. */
export type ItemPatch = Partial<Omit<Item, "id" | "createdAt" | "updatedAt">>;

export type CollectionApi = {
  key: string[];
  list: () => Promise<Item[]>;
  create: (patch: ItemPatch) => Promise<unknown>;
  update: (v: { id: number; patch: ItemPatch }) => Promise<unknown>;
  remove: (id: number) => Promise<unknown>;
};

export type CollectionConfig = {
  moduleId: string; // app/modules.ts id (icon, color, title) + kinds module key
  unit: string; // 本 / 部 (counts in 管理类型)
  itemName: string; // "书" → 「没有找到符合条件的书」
  newLabel: string; // toolbar button + panel title in create mode
  titleLabel: string; // "书名" | "名称"
  creatorLabel: string; // "作者" | "导演"
  statusLabels: StatusLabels;
  emptyTitle: string;
};
