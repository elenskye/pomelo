-- 附件 (v0.4.5): files hung on 念念不忘 notes and 一步一步 tasks / goals. The bytes are copies in
-- <data>/attachments/ (not backed up, like picky-files/); the same bytes are kept once (sha256).
CREATE TABLE attachment_files (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,                   -- the file's own name, as shown
  stored     TEXT UNIQUE,                     -- its copy in attachments/; NULL = not on this Mac
  kind       TEXT NOT NULL CHECK (kind IN ('image', 'file')),
  bytes      INTEGER NOT NULL,
  sha256     TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX attachment_files_sha ON attachment_files (sha256);

-- Which record shows which file, in order. No foreign keys: a record's delete removes its rows in
-- the same step (attachments.rs `prune`), and an undone step may bring back a file since cleaned up.
CREATE TABLE attachments (
  id         INTEGER PRIMARY KEY,
  owner      TEXT NOT NULL CHECK (owner IN ('note', 'task', 'goal')),
  record_id  INTEGER NOT NULL,
  file_id    INTEGER NOT NULL,
  sort_order INTEGER NOT NULL,
  UNIQUE (owner, record_id, file_id)
) STRICT;
CREATE INDEX attachments_record ON attachments (owner, record_id, sort_order);
