-- Atlas's groundwork (the journey itself comes in v0.9): each 年度总结 keeps its 「呈现结果」, written once
-- right after the letter. run_id: the run that wrote it (NULL once its conversation is deleted: the row
-- stays, Atlas lists and deletes them). until: written up to that day (the year wasn't over); NULL =
-- the whole year. letter: a copy of the letter. snapshot: what Rust counted and dated (JSON, atlas.rs
-- `Snapshot`). scenes: each scene's words and the records it quotes (JSON); NULL when that step
-- failed, with `error`.
CREATE TABLE year_atlas (
  id         INTEGER PRIMARY KEY,
  run_id     INTEGER UNIQUE REFERENCES runs(id) ON DELETE SET NULL,
  year       TEXT NOT NULL,
  until      TEXT,
  letter     TEXT NOT NULL,
  snapshot   TEXT NOT NULL,
  scenes     TEXT,
  error      TEXT,
  model      TEXT NOT NULL,
  model_name TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX year_atlas_by_year ON year_atlas(year, id);
