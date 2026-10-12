-- 小拾 (picky.db) v3: templates, purpose presets, the global / session instruction layers and
-- context summaries (from Picky's v3 schema).
-- templates: body may hold {{名称}} places to fill. Listed pinned first (by pinned_at), then by
-- category and title; last_used_at orders the "/" menu (not undone).
CREATE TABLE templates (
  id           INTEGER PRIMARY KEY,
  title        TEXT NOT NULL,
  category     TEXT NOT NULL DEFAULT '',
  body         TEXT NOT NULL DEFAULT '',
  pinned_at    TEXT,
  last_used_at TEXT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE presets (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  content    TEXT NOT NULL DEFAULT '',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- The global layer: one row; empty = none.
CREATE TABLE global_instructions (
  id         INTEGER PRIMARY KEY CHECK (id = 1),
  content    TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO global_instructions (id) VALUES (1);

-- The session layer and the chosen purpose preset (deleting a preset unsets it).
ALTER TABLE sessions ADD COLUMN instructions TEXT NOT NULL DEFAULT '';
ALTER TABLE sessions ADD COLUMN preset_id INTEGER REFERENCES presets(id) ON DELETE SET NULL;

-- A summary of the path up to and including through_message_id; it applies only while that message
-- is on the shown path (the farthest one wins). Only finished summaries are saved.
CREATE TABLE context_summaries (
  id                 INTEGER PRIMARY KEY,
  session_id         INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  through_message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  run_id             INTEGER REFERENCES runs(id) ON DELETE SET NULL,
  content            TEXT NOT NULL,
  created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX context_summaries_by_session ON context_summaries(session_id);
