// 玩物有志: labels + Rust command adapter (src-tauri/src/modules/games.rs: no creator, 平台, `replays` ↔ rereads).
import { commandApi } from "@/shared/collection/api";
import type { CollectionConfig } from "@/shared/collection/types";

export const gamesApi = commandApi("games", null, "replays");

export const GAMES: CollectionConfig = {
  moduleId: "games",
  unit: "个",
  itemName: "游戏",
  newLabel: "新游戏",
  titleLabel: "名称",
  creatorLabel: "平台",
  rereadLabel: "周目",
  rereadAdd: "再玩一周目",
  statusLabels: { unmarked: "待标记", todo: "想玩", doing: "在玩", done: "通关" },
  emptyTitle: "还没有游戏，先去玩一会儿",
  record: { kind: "game", label: "游戏" },
  statuses: ["todo", "doing", "done"],
  noCreator: true,
  platforms: { module: "game_platform", noun: "平台" },
  runs: true,
  kindTags: true,
};
