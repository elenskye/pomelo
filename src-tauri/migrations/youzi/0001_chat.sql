-- 小拾 (picky.db) v1: conversation core, adapted from Picky's v3–v5 schema (sessions, message
-- version tree, runs). Later phases add projects, templates, attachments, memories in new files.
-- preferences: 小拾 settings (JSON values); never undone.
CREATE TABLE preferences (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- updated_at = last activity (a sent message); renaming / pinning never touch it.
-- title_source: auto (from the first question) | user (renamed).
-- active_root_id: first message of the displayed path (app-validated, no FK).
CREATE TABLE sessions (
  id             INTEGER PRIMARY KEY,
  title          TEXT NOT NULL,
  title_source   TEXT NOT NULL DEFAULT 'auto',
  active_root_id INTEGER,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX sessions_recent ON sessions(updated_at DESC);

-- One model call. kind: reply | summary | reading. model: local:<key> | cloud:<id>;
-- model_name: what it was called then (shown in history). state: running | completed |
-- cancelled | failed | interrupted. error: user-facing Chinese text.
CREATE TABLE runs (
  id                INTEGER PRIMARY KEY,
  session_id        INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  kind              TEXT NOT NULL DEFAULT 'reply',
  model             TEXT NOT NULL,
  model_name        TEXT NOT NULL,
  state             TEXT NOT NULL,
  error             TEXT,
  prompt_tokens     INTEGER,
  completion_tokens INTEGER,
  context_window    INTEGER,
  started_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  ended_at          TEXT
) STRICT;
CREATE INDEX runs_by_session ON runs(session_id, started_at);

-- Versions of a message are its siblings (same parent, same role); each remembers its own
-- continuation in active_child_id. reasoning is never sent back to the model.
-- state: complete | streaming | cancelled | failed | interrupted.
CREATE TABLE messages (
  id              INTEGER PRIMARY KEY,
  session_id      INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  run_id          INTEGER REFERENCES runs(id) ON DELETE SET NULL,
  parent_id       INTEGER REFERENCES messages(id) ON DELETE CASCADE,
  active_child_id INTEGER,
  role            TEXT NOT NULL,
  content         TEXT NOT NULL DEFAULT '',
  reasoning       TEXT NOT NULL DEFAULT '',
  reasoning_ms    INTEGER,
  state           TEXT NOT NULL,
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX messages_by_session ON messages(session_id, created_at);
CREATE INDEX messages_children ON messages(parent_id, created_at);
