-- 小拾 (picky.db) v5: memories (from Picky's v4 schema).
-- A memory: one or two sentences (1–300 characters), global (project_id NULL) or one project's,
-- which may also apply to other projects (memory_projects). noted_on (YYYY-MM-DD) is sent as
-- 「记于」. source: manual (管理) | command (/记住 in a conversation) | import (from Picky).
-- source_* have no foreign key: a memory outlives the conversation it came from. Deleting a project
-- makes its memories global and disabled (library.rs).
CREATE TABLE memories (
  id                INTEGER PRIMARY KEY,
  content           TEXT NOT NULL,
  project_id        INTEGER REFERENCES projects(id) ON DELETE SET NULL,
  enabled           INTEGER NOT NULL DEFAULT 1,
  local_only        INTEGER NOT NULL DEFAULT 0,
  source            TEXT NOT NULL DEFAULT 'manual',
  source_session_id INTEGER,
  source_message_id INTEGER,
  noted_on          TEXT NOT NULL,
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX memories_by_project ON memories(project_id, created_at);

CREATE TABLE memory_projects (
  memory_id  INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  PRIMARY KEY (memory_id, project_id)
) STRICT;
CREATE INDEX memory_projects_by_project ON memory_projects(project_id);

-- Cached vectors (per embedding model and content hash): derived, never undone.
CREATE TABLE memory_vectors (
  memory_id    INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
  model        TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  vector       BLOB NOT NULL
) STRICT;

-- The memories a reply's run was given, as they read then (kept after edits or deletion).
CREATE TABLE run_memories (
  run_id    INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  position  INTEGER NOT NULL,
  memory_id INTEGER,
  content   TEXT NOT NULL,
  noted_on  TEXT NOT NULL,
  PRIMARY KEY (run_id, position)
) STRICT;

-- A project's conversations may leave out global memories; a conversation may turn memories off.
ALTER TABLE projects ADD COLUMN include_global_memories INTEGER NOT NULL DEFAULT 1;
ALTER TABLE sessions ADD COLUMN memory_disabled INTEGER NOT NULL DEFAULT 0;
