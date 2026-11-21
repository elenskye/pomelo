-- 见字如面 (articles, after Bear). `content` is Markdown (CommonMark + Bear's own), the only source of truth;
-- `title` and article_tags are derived from it by articles.rs on every write (a locked one keeps the ones it
-- had when it was written). Locked: `sealed` = the content encrypted with 芝麻开门's key, content = ''.
-- archived: kept but out of the way. trashed_at: in 废纸篓 since (NULL = not). created_at is an article's
-- identity in data files (unique).
CREATE TABLE articles (
  id         INTEGER PRIMARY KEY,
  title      TEXT NOT NULL DEFAULT '',
  content    TEXT NOT NULL DEFAULT '',
  sealed     BLOB,
  pinned     INTEGER NOT NULL DEFAULT 0,
  archived   INTEGER NOT NULL DEFAULT 0,
  trashed_at TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX articles_by_created ON articles(created_at);

CREATE TABLE article_tags (
  article_id INTEGER NOT NULL REFERENCES articles(id) ON DELETE CASCADE,
  tag        TEXT NOT NULL,
  PRIMARY KEY (article_id, tag)
) STRICT;
CREATE INDEX article_tags_by_tag ON article_tags(tag);

-- Same as note_tag_meta (tags.rs), 见字如面's own: the two modules' tags don't mix.
CREATE TABLE article_tag_meta (
  tag        TEXT PRIMARY KEY,
  pin_order  INTEGER,
  icon       TEXT,
  sort_order INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- 附件 may hang on articles too (pictures and files written into the text by name): the owner check is
-- rebuilt with 'article' (no other table points at attachments).
CREATE TABLE attachments_new (
  id         INTEGER PRIMARY KEY,
  owner      TEXT NOT NULL CHECK (owner IN ('note', 'task', 'goal', 'article')),
  record_id  INTEGER NOT NULL,
  file_id    INTEGER NOT NULL,
  sort_order INTEGER NOT NULL,
  UNIQUE (owner, record_id, file_id)
) STRICT;
INSERT INTO attachments_new (id, owner, record_id, file_id, sort_order) SELECT id, owner, record_id, file_id, sort_order FROM attachments;
DROP TABLE attachments;
ALTER TABLE attachments_new RENAME TO attachments;
CREATE INDEX attachments_record ON attachments (owner, record_id, sort_order);
