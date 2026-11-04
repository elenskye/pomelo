-- 小拾 (picky.db) v13 (v0.4 phase 8): 今日简报, 年度总结.
-- One 今日简报 per local day. state: writing | done | failed (content keeps the last good text while
-- rewriting or after a failure). left_out: JSON array of module names not read (hidden / not sent to
-- cloud). Not undoable, not exported.
CREATE TABLE briefs (
  day        TEXT PRIMARY KEY,
  content    TEXT NOT NULL DEFAULT '',
  state      TEXT NOT NULL,
  error      TEXT,
  model      TEXT,
  model_name TEXT,
  left_out   TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- 每月小结, only for 年度总结: month YYYY-MM; fingerprint of what it read (a month whose records
-- changed is written again, otherwise reused).
CREATE TABLE month_summaries (
  month       TEXT PRIMARY KEY,
  content     TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  model       TEXT NOT NULL,
  model_name  TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- The monthly summaries a 年度总结 reply was written from (copies; shown folded under it).
CREATE TABLE run_month_notes (
  run_id  INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  month   TEXT NOT NULL,
  content TEXT NOT NULL,
  PRIMARY KEY (run_id, month)
) STRICT;

-- every = year: month 1–12 (day 1–31, past the month's end = its last day).
ALTER TABLE schedules ADD COLUMN month INTEGER;

INSERT INTO schedules (name, prompt, every, month, day, at, builtin, sort_order) VALUES
  ('年度总结', '写一篇这一年的年度总结：读了什么、看了什么、做成了什么、画了多少，常在想的事和这一年的变化；按时间或主题分几块，最后写几句对明年的期待。', 'year', 12, 31, '21:00', 'yearly', 2);
