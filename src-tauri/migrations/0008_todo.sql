-- 一步一步 (todo, modeled on GoalMap): groups → goals → tasks → multi-level subtasks.
-- Every task carries group_id (and goal_id when under a goal); subtasks inherit both from their root.
-- Goal status: active|paused|done|abandoned (validated in Rust). Dates: 'YYYY-MM-DD'.
CREATE TABLE todo_groups (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE, -- may start with an emoji
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE todo_goals (
  id         INTEGER PRIMARY KEY,
  group_id   INTEGER NOT NULL REFERENCES todo_groups(id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  status     TEXT NOT NULL DEFAULT 'active',
  start_date TEXT,
  end_date   TEXT,
  note       TEXT,
  review     TEXT, -- 复盘
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX todo_goals_by_group ON todo_goals(group_id, sort_order);

CREATE TABLE todo_tasks (
  id         INTEGER PRIMARY KEY,
  group_id   INTEGER NOT NULL REFERENCES todo_groups(id) ON DELETE CASCADE,
  goal_id    INTEGER REFERENCES todo_goals(id) ON DELETE CASCADE,
  parent_id  INTEGER REFERENCES todo_tasks(id) ON DELETE CASCADE,
  title      TEXT NOT NULL, -- may contain Markdown **bold**
  note       TEXT,
  link       TEXT,
  plan_date  TEXT,          -- 计划哪天做
  start_date TEXT,          -- 时间段开始（需要 due_date 作为结束）
  due_date   TEXT,          -- 截止
  done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
  done_at    TEXT,          -- NULL for done tasks imported without a time
  collapsed  INTEGER NOT NULL DEFAULT 0 CHECK (collapsed IN (0, 1)),
  sort_order INTEGER NOT NULL, -- among siblings
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX todo_tasks_by_place ON todo_tasks(group_id, goal_id, parent_id, sort_order);
CREATE INDEX todo_tasks_by_goal ON todo_tasks(goal_id);
CREATE INDEX todo_tasks_by_parent ON todo_tasks(parent_id);
CREATE INDEX todo_tasks_by_done_at ON todo_tasks(done_at);
