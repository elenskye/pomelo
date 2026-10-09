-- 念念不忘: per-tag settings, keyed by the tag's full path (tags themselves still live in the notes'
-- text). pin_order: pinned tags are listed above the tag tree in this order (NULL = not pinned).
-- icon: an emoji shown instead of 「#」. Renaming a tag moves its settings (and its sub-tags').
CREATE TABLE note_tag_meta (
  tag        TEXT PRIMARY KEY,
  pin_order  INTEGER,
  icon       TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
