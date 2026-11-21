// Shared "collection" UI (books, films, games): a list of titled items with creator/country/kind/status/rating
// (玩物有志: 平台 instead of a creator, and 周目).
// Each module supplies a CollectionConfig (labels) and a CollectionApi (Rust commands adapter).
// 类型 lists are user-editable (shared/kinds, keyed by moduleId).
import type { StatusCode, StatusLabels } from "@/shared/status";

/** 重读 / 重看: a later time (the first is the item's own finishedYear + rating). */
export type Reread = { year: string | null; rating: number | null };

/** Normalized item; module adapters map `creator` ↔ author/director. */
export type Item = {
  id: number;
  title: string;
  creator: string | null;
  country: string | null;
  kindId: number | null; // kinds.id; null = 未分类
  status: StatusCode;
  rating: number | null;
  finishedYear: string | null; // YYYY
  note: string | null;
  rereads: Reread[];
  /** 玩物有志: kinds.id in 平台. */
  platformId?: number | null;
  /** 玩物有志: how many times through (Rust games.rs). */
  runs?: number;
  createdAt: string;
  updatedAt: string;
};

/** Omitted key = unchanged; null = clear. Rust auto-fills finishedYear on → done only if the key is omitted. */
export type ItemPatch = Partial<Omit<Item, "id" | "createdAt" | "updatedAt" | "runs">>;

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
  rereadLabel: string; // "重读" | "重看"
  rereadAdd: string; // "再读一次" | "再看一次"
  statusLabels: StatusLabels;
  emptyTitle: string;
  /** Its empty page's drawing (visual-atelier marks/empty/). */
  emptyArt?: import("@/shared/ui").EmptyArt;
  /** A record's kind for 柚子 and the links (book / film / game), and what to call it there. */
  record: { kind: string; label: string };
  /** The states it has, in order (玩物有志 has no 待标记). */
  statuses?: StatusCode[];
  /** 玩物有志: no 作者 / 导演; 平台 (its own kinds list) instead; the 完成时间 and 再玩 rows are 周目. */
  noCreator?: boolean;
  platforms?: { module: string; noun: string };
  runs?: boolean;
  /** 类型 as colored tags in the table (one click to change). */
  kindTags?: boolean;
};

/** 一周目, 二周目… (then 第 N 周目). */
export const runName = (n: number) => (n >= 1 && n <= 10 ? `${"一二三四五六七八九十"[n - 1]}周目` : `第 ${n} 周目`);

/** The most recent 完成时间: the item's own or a later 重读 / 重看. */
export const latestYear = (it: Pick<Item, "finishedYear" | "rereads">) =>
  [it.finishedYear, ...it.rereads.map((r) => r.year)].filter((y): y is string => !!y).sort().pop() ?? null;
