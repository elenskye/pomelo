-- User-editable 类型 lists per module ('books' | 'films' | 'drawing').
-- Items reference kinds.id; deleting a kind leaves its items 未分类 (kind_id NULL).
CREATE TABLE kinds (
  id         INTEGER PRIMARY KEY,
  module     TEXT NOT NULL,
  name       TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (module, name)
) STRICT;

INSERT INTO kinds (module, name, sort_order) VALUES
  ('books', '虚构', 1), ('books', '非虚构', 2), ('books', '漫画', 3), ('books', '其他', 4),
  ('films', '电影', 1), ('films', '剧集', 2), ('films', '动漫', 3);

-- books.kind (fixed codes, v3) → books.kind_id
ALTER TABLE books ADD COLUMN kind_id INTEGER REFERENCES kinds(id) ON DELETE SET NULL;
UPDATE books SET kind_id = (
  SELECT id FROM kinds WHERE module = 'books' AND name = CASE books.kind
    WHEN 'fiction' THEN '虚构' WHEN 'nonfiction' THEN '非虚构' WHEN 'comic' THEN '漫画' ELSE '其他' END);
ALTER TABLE books DROP COLUMN kind;
CREATE INDEX books_by_kind ON books(kind_id);

-- films.kind (fixed codes, v4) → films.kind_id
ALTER TABLE films ADD COLUMN kind_id INTEGER REFERENCES kinds(id) ON DELETE SET NULL;
UPDATE films SET kind_id = (
  SELECT id FROM kinds WHERE module = 'films' AND name = CASE films.kind
    WHEN 'series' THEN '剧集' WHEN 'anime' THEN '动漫' ELSE '电影' END);
ALTER TABLE films DROP COLUMN kind;
CREATE INDEX films_by_kind ON films(kind_id);
