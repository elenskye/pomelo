// 好奇看看: labels + Rust command adapter (src-tauri/src/modules/films.rs; `director` ↔ creator).
import { commandApi } from "@/shared/collection/api";
import type { CollectionConfig } from "@/shared/collection/types";

export const filmsApi = commandApi("films", "director");

export const FILMS: CollectionConfig = {
  moduleId: "films",
  unit: "部",
  itemName: "作品",
  newLabel: "添加",
  titleLabel: "名称",
  creatorLabel: "导演",
  rereadLabel: "重看",
  rereadAdd: "再看一次",
  statusLabels: { unmarked: "待标记", todo: "想看", doing: "在看", done: "已看" },
  emptyTitle: "片单空空，先去泡杯茶",
};
