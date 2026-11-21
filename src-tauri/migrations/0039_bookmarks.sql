-- 一网打尽 (网址收藏夹): folders and links in one tree. parent_id NULL = the top level; folders nest to any
-- depth (deleting one deletes what's in it). sort_order = place among its siblings, folders and links mixed
-- (as dragged). A folder's url is ''. on_home: the link shows on 首页.
CREATE TABLE bookmarks (
  id         INTEGER PRIMARY KEY,
  parent_id  INTEGER REFERENCES bookmarks(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL CHECK (kind IN ('folder', 'link')),
  title      TEXT NOT NULL,
  url        TEXT NOT NULL DEFAULT '',
  on_home    INTEGER NOT NULL DEFAULT 0 CHECK (on_home IN (0, 1)),
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX bookmarks_by_parent ON bookmarks(parent_id, sort_order);
