-- 随意画画: courses → lessons. Lesson status: unmarked|todo|doing|done (validated in Rust).
CREATE TABLE drawing_courses (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE drawing_lessons (
  id             INTEGER PRIMARY KEY,
  course_id      INTEGER NOT NULL REFERENCES drawing_courses(id) ON DELETE CASCADE,
  code           TEXT,
  kind_id        INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  title          TEXT NOT NULL,
  status         TEXT NOT NULL DEFAULT 'unmarked',
  finished_month TEXT,
  note           TEXT,
  sort_order     INTEGER NOT NULL,
  created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX drawing_lessons_by_course ON drawing_lessons(course_id, sort_order);
CREATE INDEX drawing_lessons_by_kind ON drawing_lessons(kind_id);

INSERT INTO kinds (module, name, sort_order) VALUES
  ('drawing', '正课', 1), ('drawing', 'DLC 跟画', 2), ('drawing', 'LIVE 辅导', 3);
