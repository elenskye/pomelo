-- 芝麻开门: categories (one per entry; names encrypted like the entries) and archive.
-- category_id / archived are plain numbers, like sort_order; deleting a category → 未分类.
CREATE TABLE vault_categories (
  id         INTEGER PRIMARY KEY,
  data       BLOB NOT NULL,                   -- nonce ‖ AEAD(vault key, name)
  sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;

ALTER TABLE vault_items ADD COLUMN category_id INTEGER REFERENCES vault_categories(id) ON DELETE SET NULL;
ALTER TABLE vault_items ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
