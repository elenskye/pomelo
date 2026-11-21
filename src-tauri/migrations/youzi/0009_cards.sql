-- 小拾 (picky.db) v9 (v0.4): a reply's confirmation card. 小拾 only proposes; confirming writes the
-- ticked items in one undoable step. state: pending | confirmed | cancelled.
CREATE TABLE cards (
  run_id     INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  state      TEXT NOT NULL DEFAULT 'pending',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  decided_at TEXT
) STRICT;
-- One proposal: tool (pomelo_add_entry…), what the card shows (title, fields JSON [{label, before?,
-- after}]), input = exactly what the module's create gets. checked: confirmed (ticked); record_*: what
-- got written (opened like a 全局搜索 hit).
CREATE TABLE card_items (
  run_id        INTEGER NOT NULL REFERENCES cards(run_id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  tool          TEXT NOT NULL,
  module        TEXT NOT NULL,
  title         TEXT NOT NULL,
  fields        TEXT NOT NULL,
  input         TEXT NOT NULL,
  checked       INTEGER NOT NULL DEFAULT 1,
  record_kind   TEXT,
  record_id     INTEGER,
  record_parent INTEGER,
  record_day    TEXT,
  PRIMARY KEY (run_id, position)
) STRICT;
