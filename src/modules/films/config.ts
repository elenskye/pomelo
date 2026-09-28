// 好奇看看: labels + Rust command adapter (src-tauri/src/modules/films.rs; `director` ↔ creator).
import { commandApi } from "@/shared/collection/api";
import type { CollectionConfig } from "@/shared/collection/types";

export const filmsApi = commandApi("films", "director");

export const FILMS: CollectionConfig = {
  moduleId: "films",
  itemName: "作品",
  newLabel: "添加",
  titleLabel: "名称",
  creatorLabel: "导演",
  statusLabels: { unmarked: "待标记", todo: "想看", doing: "在看", done: "已看" },
  kinds: [
    { value: "movie", label: "电影" },
    { value: "series", label: "剧集" },
    { value: "anime", label: "动漫" },
  ],
  emptyTitle: "片单空空，先去泡杯茶",
  emptyHint: "也可以在「设置」里从 Excel 转换来的文件导入影单",
};
