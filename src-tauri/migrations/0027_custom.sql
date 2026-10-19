-- 五花八门 / 循序渐进 (v0.4.5): modules made from a template, each its own sidebar module
-- (key "list/<id>" / "progress/<id>"). Rules in modules/custom.rs, lists.rs, progress.rs.
-- AUTOINCREMENT: a deleted module's id is never reused (its sidebar order, hidden and cloud settings
-- are keyed by it).
CREATE TABLE custom_modules (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  template    TEXT NOT NULL CHECK (template IN ('list', 'progress')),
  name        TEXT NOT NULL UNIQUE,
  icon        TEXT NOT NULL,
  course_word TEXT,                           -- 循序渐进: what a course / lesson is called (课程 / 课节)
  lesson_word TEXT,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- 五花八门: the list's own fields (名称 is built in), then its items
CREATE TABLE list_fields (
  id         INTEGER PRIMARY KEY,
  module_id  INTEGER NOT NULL REFERENCES custom_modules (id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  type       TEXT NOT NULL CHECK (type IN ('text', 'longtext', 'number', 'date', 'select', 'multi', 'check', 'rating', 'link')),
  options    TEXT NOT NULL DEFAULT '[]',     -- select / multi: the option names in order (JSON)
  shown      INTEGER NOT NULL DEFAULT 1,     -- a column in the table
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (module_id, name)
) STRICT;

CREATE TABLE list_items (
  id         INTEGER PRIMARY KEY,
  module_id  INTEGER NOT NULL REFERENCES custom_modules (id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  data       TEXT NOT NULL DEFAULT '{}',     -- {"<field id>": value}; empty values left out
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX list_items_module ON list_items (module_id, sort_order);

-- 循序渐进: courses → lessons (like 随意画画, no 类型)
CREATE TABLE progress_courses (
  id         INTEGER PRIMARY KEY,
  module_id  INTEGER NOT NULL REFERENCES custom_modules (id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (module_id, name)
) STRICT;

CREATE TABLE progress_lessons (
  id             INTEGER PRIMARY KEY,
  course_id      INTEGER NOT NULL REFERENCES progress_courses (id) ON DELETE CASCADE,
  code           TEXT,
  title          TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo', 'doing', 'done')),
  finished_month TEXT,
  note           TEXT,
  sort_order     INTEGER NOT NULL,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX progress_lessons_course ON progress_lessons (course_id, sort_order);
