-- 念念不忘 (notes, Flomo-style). `content` is the only source of truth: Markdown-lite text with
-- inline #tags (#a/b = nested). note_tags is derived from content by notes.rs on every write.
-- note_refs: a note quoting others (Flomo 批注). created_at is a note's identity in data files
-- (refs are exported as the target's created_at), hence unique.
CREATE TABLE notes (
  id         INTEGER PRIMARY KEY,
  content    TEXT NOT NULL,
  pinned     INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX notes_by_created ON notes(created_at);

CREATE TABLE note_tags (
  note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  tag     TEXT NOT NULL,
  PRIMARY KEY (note_id, tag)
) STRICT;
CREATE INDEX note_tags_by_tag ON note_tags(tag);

CREATE TABLE note_refs (
  note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  ref_id     INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
  sort_order INTEGER NOT NULL,
  PRIMARY KEY (note_id, ref_id)
) STRICT;
CREATE INDEX note_refs_by_ref ON note_refs(ref_id);
