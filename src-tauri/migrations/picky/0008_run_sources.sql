-- 小拾 (picky.db) v8 (v0.4): what a reply was given, numbered for citations ([n] in its text):
-- project file passages first, then records 小拾's search found. kind: record | passage.
-- Records open like a 全局搜索 hit (module, record_kind, ref_id, parent, day); passages show their
-- text (artifact_id, segment_ordinal). content = what the model got.
CREATE TABLE run_sources (
  run_id          INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  n               INTEGER NOT NULL,
  kind            TEXT NOT NULL,
  module          TEXT,
  record_kind     TEXT,
  ref_id          INTEGER,
  parent          INTEGER,
  day             TEXT,
  artifact_id     INTEGER,
  segment_ordinal INTEGER,
  title           TEXT NOT NULL,
  detail          TEXT,
  content         TEXT NOT NULL,
  PRIMARY KEY (run_id, n)
) STRICT;
