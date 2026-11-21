-- Statement provenance is independent of asset snapshots. Original files are not retained.
ALTER TABLE fin_entries ADD COLUMN is_refund INTEGER NOT NULL DEFAULT 0 CHECK (is_refund IN (0, 1));
ALTER TABLE fin_entries ADD COLUMN refund_of INTEGER REFERENCES fin_entries(id) ON DELETE SET NULL;
CREATE INDEX fin_entries_refund_of ON fin_entries(refund_of);
CREATE TABLE fin_imports (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('alipay', 'wechat')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE TABLE fin_import_rows (
  id INTEGER PRIMARY KEY,
  batch_id INTEGER NOT NULL REFERENCES fin_imports(id) ON DELETE CASCADE,
  source_json TEXT NOT NULL,
  draft_json TEXT NOT NULL,
  state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'ready', 'posted', 'neutral', 'duplicate')),
  entry_id INTEGER REFERENCES fin_entries(id) ON DELETE SET NULL
) STRICT;
CREATE INDEX fin_import_rows_batch ON fin_import_rows(batch_id);
CREATE INDEX fin_import_rows_entry ON fin_import_rows(entry_id);
INSERT INTO kinds (module, name, sort_order)
  SELECT 'fin_in', '工资', coalesce(max(sort_order), 0) + 1 FROM kinds WHERE module = 'fin_in'
  HAVING NOT EXISTS (SELECT 1 FROM kinds WHERE module = 'fin_in' AND name = '工资');
