-- 玩物有志 (v1.0 Phase 5): games, a sibling of 摸鱼读读 / 好奇看看. Rules in modules/games.rs.
-- 类型 and 平台 are two kinds lists (kinds.module 'games' / 'game_platform'); deleting one leaves 未分类.
-- 状态: 想玩 todo · 在玩 doing · 通关 done (no 待标记). The first time through is the game's own
-- 完成时间 and 尝尝咸淡; 二周目 on are game_replays, like book_rereads.
CREATE TABLE games (
  id            INTEGER PRIMARY KEY,
  title         TEXT NOT NULL,
  platform_id   INTEGER REFERENCES kinds (id) ON DELETE SET NULL,
  country       TEXT,
  kind_id       INTEGER REFERENCES kinds (id) ON DELETE SET NULL,
  status        TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'doing', 'done')),
  rating        INTEGER CHECK (rating BETWEEN 1 AND 5),
  finished_year TEXT,
  note          TEXT,
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX games_kind ON games (kind_id);
CREATE INDEX games_platform ON games (platform_id);

CREATE TABLE game_replays (
  id         INTEGER PRIMARY KEY,
  game_id    INTEGER NOT NULL REFERENCES games (id) ON DELETE CASCADE,
  year       TEXT,
  rating     INTEGER CHECK (rating BETWEEN 1 AND 5),
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX game_replays_by_game ON game_replays (game_id, sort_order);

INSERT INTO kinds (module, name, sort_order) VALUES
  ('games', '动作', 1), ('games', '冒险', 2), ('games', '角色扮演', 3), ('games', '解谜', 4), ('games', '剧情', 5), ('games', '模拟经营', 6), ('games', '休闲', 7),
  ('game_platform', 'Switch', 1), ('game_platform', 'PS5', 2), ('game_platform', 'PC', 3), ('game_platform', '手机', 4);
