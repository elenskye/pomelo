-- 五花八门: 日期 fields become 年份 (values keep their year: '2026-10-05' → '2026').
-- 循序渐进: lessons get a 类型 from the module's own kinds list (kinds.module = 'progress/<id>').
-- list_fields is rebuilt for its CHECK; nothing references it (item values are keyed by field id).
CREATE TABLE list_fields_new (
  id         INTEGER PRIMARY KEY,
  module_id  INTEGER NOT NULL REFERENCES custom_modules (id) ON DELETE CASCADE,
  name       TEXT NOT NULL,
  type       TEXT NOT NULL CHECK (type IN ('text', 'longtext', 'number', 'year', 'select', 'multi', 'check', 'rating', 'link')),
  options    TEXT NOT NULL DEFAULT '[]',
  shown      INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (module_id, name)
) STRICT;
INSERT INTO list_fields_new (id, module_id, name, type, options, shown, sort_order, created_at, updated_at)
  SELECT id, module_id, name, CASE type WHEN 'date' THEN 'year' ELSE type END, options, shown, sort_order, created_at, updated_at FROM list_fields;
DROP TABLE list_fields;
ALTER TABLE list_fields_new RENAME TO list_fields;

-- every value of a 年份 field cut to its year; other values rebuilt as they were (JSON types kept)
UPDATE list_items SET data = (
  SELECT json_group_object(e.key,
    CASE
      WHEN f.type = 'year' AND e.type = 'text' THEN substr(e.value, 1, 4)
      WHEN e.type = 'true' THEN json('true')
      WHEN e.type = 'false' THEN json('false')
      WHEN e.type = 'null' THEN json('null')
      WHEN e.type IN ('array', 'object') THEN json(e.value)
      ELSE e.value
    END)
  FROM json_each(list_items.data) e LEFT JOIN list_fields f ON f.id = CAST(e.key AS INTEGER) AND f.module_id = list_items.module_id
)
WHERE EXISTS (SELECT 1 FROM list_fields f WHERE f.module_id = list_items.module_id AND f.type = 'year');

ALTER TABLE progress_lessons ADD COLUMN kind_id INTEGER REFERENCES kinds (id) ON DELETE SET NULL;
