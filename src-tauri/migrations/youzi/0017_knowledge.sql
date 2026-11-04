-- 小拾 (picky.db) v17 (v0.4.5 阶段 7): 知识库.
-- kb_libraries: 库 (name ≤ 30 chars), in the user's order. kb_sources: what a library holds — a folder
-- (`path`, absolute; read in place, never copied) or a saved web page (`artifact_id`: its article, a copy
-- like 添加网页). `cloud`: may go to a cloud model (default yes). What a folder holds, the passages and
-- their vectors are derived from the folder (search.db `kb_docs` / `kb_chunks`), never stored here.
-- session_libraries / project_libraries: the libraries a conversation / project chose (none = its
-- project's, then all). sessions.knowledge: the composer's 知识库 switch (its questions search them).
-- run_passages / run_sources: a passage from a library (kb_source + kb_path, '' for a web page; the
-- library's name as it was), to open its file.
CREATE TABLE kb_libraries (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE kb_sources (
  id          INTEGER PRIMARY KEY,
  library_id  INTEGER NOT NULL REFERENCES kb_libraries(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL CHECK (kind IN ('folder', 'web')),
  path        TEXT,
  artifact_id INTEGER REFERENCES artifacts(id),
  cloud       INTEGER NOT NULL DEFAULT 1,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX kb_sources_by_library ON kb_sources(library_id, sort_order);
CREATE INDEX kb_sources_by_artifact ON kb_sources(artifact_id);

CREATE TABLE session_libraries (
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  library_id INTEGER NOT NULL REFERENCES kb_libraries(id) ON DELETE CASCADE,
  PRIMARY KEY (session_id, library_id)
) STRICT;
CREATE INDEX session_libraries_by_library ON session_libraries(library_id);

CREATE TABLE project_libraries (
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  library_id INTEGER NOT NULL REFERENCES kb_libraries(id) ON DELETE CASCADE,
  PRIMARY KEY (project_id, library_id)
) STRICT;
CREATE INDEX project_libraries_by_library ON project_libraries(library_id);

ALTER TABLE sessions ADD COLUMN knowledge INTEGER NOT NULL DEFAULT 0;

ALTER TABLE run_passages ADD COLUMN kb_source INTEGER;
ALTER TABLE run_passages ADD COLUMN kb_path TEXT;
ALTER TABLE run_passages ADD COLUMN kb_library TEXT;
ALTER TABLE run_sources ADD COLUMN kb_source INTEGER;
ALTER TABLE run_sources ADD COLUMN kb_path TEXT;
