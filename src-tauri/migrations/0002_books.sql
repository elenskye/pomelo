-- 摸鱼读读. status: unmarked|todo|doing|done (validated in Rust). finished_month: 'YYYY-MM'.
CREATE TABLE books (
  id             INTEGER PRIMARY KEY,
  title          TEXT NOT NULL,
  author         TEXT,
  country        TEXT,
  status         TEXT NOT NULL DEFAULT 'unmarked',
  rating         INTEGER CHECK (rating BETWEEN 1 AND 5),
  finished_month TEXT,
  note           TEXT,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
