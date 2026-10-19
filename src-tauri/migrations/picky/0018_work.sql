-- 小拾 (picky.db) v18 (v0.4.5 阶段 9): Work inside conversations.
-- message_edit_files: the file a question asked to change (改文件): its path (read and written in place,
-- never copied) and name; a new version of the question keeps it.
-- file_edits: a reply's proposed changes to that file (one per run): path, name, kind (markdown | text |
-- word), the file's sha256 when it was read (writing refuses a file changed since), how many paragraphs
-- it had, state pending | written | cancelled, error (why writing failed), decided_at.
-- file_edit_items: one changed paragraph each, in file order: its number (1-based, as the model saw it),
-- the text before and after, checked when written.
CREATE TABLE message_edit_files (
  message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  name       TEXT NOT NULL
) STRICT;

CREATE TABLE file_edits (
  run_id     INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  path       TEXT NOT NULL,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL,
  sha256     TEXT NOT NULL,
  total      INTEGER NOT NULL,
  state      TEXT NOT NULL DEFAULT 'pending',
  error      TEXT,
  decided_at TEXT
) STRICT;

CREATE TABLE file_edit_items (
  run_id    INTEGER NOT NULL REFERENCES file_edits(run_id) ON DELETE CASCADE,
  position  INTEGER NOT NULL,
  paragraph INTEGER NOT NULL,
  before    TEXT NOT NULL,
  after     TEXT NOT NULL,
  checked   INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (run_id, position)
) STRICT;
