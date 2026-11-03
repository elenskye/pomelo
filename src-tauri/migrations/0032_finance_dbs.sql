-- Extend provenance sources without firing the existing parent's ON DELETE CASCADE.
CREATE TABLE fin_imports_new (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('alipay', 'wechat', 'dbs')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO fin_imports_new SELECT * FROM fin_imports;
CREATE TABLE fin_import_rows_new (
  id INTEGER PRIMARY KEY,
  batch_id INTEGER NOT NULL REFERENCES fin_imports_new(id) ON DELETE CASCADE,
  source_json TEXT NOT NULL,
  draft_json TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'ready', 'posted', 'neutral', 'duplicate')),
  entry_id INTEGER REFERENCES fin_entries(id) ON DELETE SET NULL,
  resolution TEXT CHECK (resolution IN ('new', 'linked'))
) STRICT;
INSERT INTO fin_import_rows_new (id, batch_id, source_json, draft_json, state, entry_id)
  SELECT id, batch_id, source_json, draft_json, state, entry_id FROM fin_import_rows;
DROP TABLE fin_import_rows;
DROP TABLE fin_imports;
ALTER TABLE fin_imports_new RENAME TO fin_imports;
ALTER TABLE fin_import_rows_new RENAME TO fin_import_rows;
CREATE INDEX fin_import_rows_batch ON fin_import_rows(batch_id);
CREATE INDEX fin_import_rows_entry ON fin_import_rows(entry_id);
