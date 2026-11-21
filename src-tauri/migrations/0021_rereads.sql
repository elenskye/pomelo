-- 摸鱼读读 / 好奇看看: 重读 / 重看. Each later time: the year (YYYY) and its rating. The first time stays
-- the book's / film's own 完成时间 and 尝尝咸淡.
CREATE TABLE book_rereads (
  id         INTEGER PRIMARY KEY,
  book_id    INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  year       TEXT,
  rating     INTEGER CHECK (rating BETWEEN 1 AND 5),
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX book_rereads_by_book ON book_rereads(book_id, sort_order);

CREATE TABLE film_rereads (
  id         INTEGER PRIMARY KEY,
  film_id    INTEGER NOT NULL REFERENCES films(id) ON DELETE CASCADE,
  year       TEXT,
  rating     INTEGER CHECK (rating BETWEEN 1 AND 5),
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX film_rereads_by_film ON film_rereads(film_id, sort_order);
