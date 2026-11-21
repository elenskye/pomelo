-- 好奇看看. kind: movie|series|anime; status: unmarked|todo|doing|done (validated in Rust).
CREATE TABLE films (
  id             INTEGER PRIMARY KEY,
  title          TEXT NOT NULL,
  director       TEXT,
  country        TEXT,
  kind           TEXT NOT NULL DEFAULT 'movie',
  status         TEXT NOT NULL DEFAULT 'unmarked',
  rating         INTEGER CHECK (rating BETWEEN 1 AND 5),
  finished_month TEXT,
  note           TEXT,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
