-- 出门走走: templates (scene lists) → trips (snapshot copies, independent after creation).
-- 类别 = kinds (module 'packing'), shared by template items and trip items.
CREATE TABLE packing_templates (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE packing_template_items (
  id          INTEGER PRIMARY KEY,
  template_id INTEGER NOT NULL REFERENCES packing_templates(id) ON DELETE CASCADE,
  kind_id     INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  name        TEXT NOT NULL,
  note        TEXT,
  sort_order  INTEGER NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX packing_template_items_by_template ON packing_template_items(template_id, sort_order);
CREATE INDEX packing_template_items_by_kind ON packing_template_items(kind_id);

CREATE TABLE trips (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  template_id INTEGER REFERENCES packing_templates(id) ON DELETE SET NULL, -- provenance only
  depart_on   TEXT, -- YYYY-MM-DD
  note        TEXT,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE trip_items (
  id         INTEGER PRIMARY KEY,
  trip_id    INTEGER NOT NULL REFERENCES trips(id) ON DELETE CASCADE,
  kind_id    INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  name       TEXT NOT NULL,
  note       TEXT,
  packed     INTEGER NOT NULL DEFAULT 0 CHECK (packed IN (0, 1)),
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX trip_items_by_trip ON trip_items(trip_id, sort_order);
CREATE INDEX trip_items_by_kind ON trip_items(kind_id);

INSERT INTO kinds (module, name, sort_order) VALUES
  ('packing', '重要物品', 1), ('packing', '随身', 2), ('packing', '护理', 3), ('packing', '兴趣', 4);
