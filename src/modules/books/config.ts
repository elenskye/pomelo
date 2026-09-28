// 摸鱼读读: labels + Rust command adapter (src-tauri/src/modules/books.rs; `author` ↔ creator).
import { commandApi } from "@/shared/collection/api";
import type { CollectionConfig } from "@/shared/collection/types";

export const booksApi = commandApi("books", "author");

export const BOOKS: CollectionConfig = {
  moduleId: "books",
  itemName: "书",
  newLabel: "新书",
  titleLabel: "书名",
  creatorLabel: "作者",
  statusLabels: { unmarked: "待标记", todo: "想读", doing: "在读", done: "已读" },
  kinds: [
    { value: "fiction", label: "虚构" },
    { value: "nonfiction", label: "非虚构" },
    { value: "comic", label: "漫画" },
    { value: "other", label: "其他" },
  ],
  emptyTitle: "书架空空，去摸会儿鱼吧",
  emptyHint: "也可以在「设置」里从 Excel 转换来的文件导入书单",
};
