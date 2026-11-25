-- DevJanitor 备份 (1.1.0; requirements「DevJanitor › 备份」): one row a backup run. The tasks and the drive
-- are a preference (`assistant.janitor.backup`). Written by the run itself: never undone.
-- tasks: [{id, source, target, files, bytes, updated, done}] as they were run; errors: [[path, why]] (at most 200).
CREATE TABLE backup_runs (
  id          INTEGER PRIMARY KEY,
  dest        TEXT NOT NULL DEFAULT 'drive',
  drive       TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'stopped', 'failed')),
  tasks       TEXT NOT NULL DEFAULT '[]',
  files       INTEGER NOT NULL DEFAULT 0,
  bytes       INTEGER NOT NULL DEFAULT 0,
  errors      TEXT NOT NULL DEFAULT '[]',
  note        TEXT,
  started_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  finished_at TEXT
) STRICT;
