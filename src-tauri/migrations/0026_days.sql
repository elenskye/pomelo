-- 来日方长 (v0.4.5, after Days Matter): days counted toward or since. Rules in modules/days.rs.
CREATE TABLE days (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('day', 'birthday', 'document')),
  name       TEXT NOT NULL,
  date       TEXT NOT NULL,                   -- YYYY-MM-DD; a birthday without the year: --MM-DD
  repeat     TEXT NOT NULL DEFAULT 'none' CHECK (repeat IN ('none', 'year', 'month', 'week')),
  remind     TEXT NOT NULL DEFAULT '',        -- lead days, largest first: "30,7,1"
  pinned     INTEGER NOT NULL DEFAULT 0,
  note       TEXT,
  ideas      TEXT,                            -- birthday: 礼物点子
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- birthday: 送过的, one line per gift
CREATE TABLE day_gifts (
  id         INTEGER PRIMARY KEY,
  day_id     INTEGER NOT NULL REFERENCES days (id) ON DELETE CASCADE,
  year       TEXT NOT NULL,
  gift       TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX day_gifts_day ON day_gifts (day_id);
