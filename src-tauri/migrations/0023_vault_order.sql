-- 芝麻开门: hand-made order (drag to reorder). Names are encrypted, so 0 everywhere = never
-- arranged: the page keeps sorting by name until the first drag numbers every entry.
ALTER TABLE vault_items ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
