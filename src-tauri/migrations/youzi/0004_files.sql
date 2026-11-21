-- 小拾 (picky.db) v4: attachments and project files (from Picky's v3 / v5 schema).
-- artifacts: one stored copy of a file the user added (text, Markdown, PDF), in picky-files/
-- under file_name. Shared by attachments and project file versions; the same bytes (sha256) are
-- stored once. deleted_at: the copy was removed by storage management (the row stays so old
-- messages can say so).
CREATE TABLE artifacts (
  id           INTEGER PRIMARY KEY,
  display_name TEXT NOT NULL,
  file_name    TEXT NOT NULL UNIQUE,
  media_type   TEXT NOT NULL,
  byte_count   INTEGER NOT NULL,
  sha256       TEXT NOT NULL,
  page_count   INTEGER,
  text_chars   INTEGER NOT NULL,
  deleted_at   TEXT,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX artifacts_by_hash ON artifacts(sha256);

-- Extracted text: one row per PDF page with text, or per Markdown section / ~6000 characters.
CREATE TABLE artifact_segments (
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
  ordinal     INTEGER NOT NULL,
  page_start  INTEGER,
  page_end    INTEGER,
  heading     TEXT,
  content     TEXT NOT NULL,
  PRIMARY KEY (artifact_id, ordinal)
) STRICT;

CREATE TABLE message_attachments (
  message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id),
  position    INTEGER NOT NULL,
  PRIMARY KEY (message_id, artifact_id)
) STRICT;
CREATE INDEX message_attachments_by_artifact ON message_attachments(artifact_id);

-- Notes from reading long material segment by segment (a reply's run, kind = reading).
CREATE TABLE reading_notes (
  run_id      INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
  ordinal     INTEGER NOT NULL,
  content     TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (run_id, artifact_id, ordinal)
) STRICT;

-- A project's files. Each version is an artifact; current_version is what new messages and the
-- index use. indexed = 建立索引 chosen.
CREATE TABLE project_files (
  id              INTEGER PRIMARY KEY,
  project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  current_version INTEGER NOT NULL DEFAULT 1,
  indexed         INTEGER NOT NULL DEFAULT 0,
  sort_order      INTEGER NOT NULL DEFAULT 0,
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX project_files_by_project ON project_files(project_id, sort_order);

CREATE TABLE project_file_versions (
  file_id     INTEGER NOT NULL REFERENCES project_files(id) ON DELETE CASCADE,
  version     INTEGER NOT NULL,
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id),
  added_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (file_id, version)
) STRICT;
CREATE INDEX project_file_versions_by_artifact ON project_file_versions(artifact_id);

-- The project files a question used, with the version it used (kept after replacing / removing).
CREATE TABLE message_project_files (
  message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  file_id     INTEGER NOT NULL,
  version     INTEGER NOT NULL,
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id),
  PRIMARY KEY (message_id, position)
) STRICT;
CREATE INDEX message_project_files_by_artifact ON message_project_files(artifact_id);

-- The index of one file version for one embedding model: passages and their vectors (f32, little
-- endian). Derived data: never undone, rebuilt when needed.
CREATE TABLE artifact_indexes (
  artifact_id INTEGER NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
  model       TEXT NOT NULL,
  chunk_count INTEGER NOT NULL,
  built_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (artifact_id, model)
) STRICT;

CREATE TABLE artifact_chunks (
  artifact_id     INTEGER NOT NULL,
  model           TEXT NOT NULL,
  ordinal         INTEGER NOT NULL,
  segment_ordinal INTEGER NOT NULL,
  content         TEXT NOT NULL,
  vector          BLOB NOT NULL,
  PRIMARY KEY (artifact_id, model, ordinal),
  FOREIGN KEY (artifact_id, model) REFERENCES artifact_indexes(artifact_id, model) ON DELETE CASCADE
) STRICT;

-- Passages retrieved for a reply, as they read then.
CREATE TABLE run_passages (
  run_id          INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  position        INTEGER NOT NULL,
  file_id         INTEGER NOT NULL,
  artifact_id     INTEGER NOT NULL,
  segment_ordinal INTEGER NOT NULL,
  label           TEXT NOT NULL,
  content         TEXT NOT NULL,
  PRIMARY KEY (run_id, position)
) STRICT;
