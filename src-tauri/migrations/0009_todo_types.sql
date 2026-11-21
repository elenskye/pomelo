-- 一步一步: task types (validated in Rust).
--   normal   plain task
--   repeat   dated task that recurs by repeat_rule; completing an occurrence logs it and moves the
--            dates to the next one; ends after repeat_total occurrences or past repeat_until
--   checkin  habit on scheduled days (repeat_rule daily / weekly); one log per checked day, back-fill allowed;
--            plan_date = start, repeat_until = end, repeat_total = target days
--   progress count toward target (unit, e.g. 本); period all|day|week|month = when the count restarts
-- repeat_rule: daily | weekly:<1-7,…> (Mon=1) | monthly:<1-31> | every:<n>d | every:<n>w | after:<n>d
ALTER TABLE todo_tasks ADD COLUMN task_type TEXT NOT NULL DEFAULT 'normal';
ALTER TABLE todo_tasks ADD COLUMN repeat_rule TEXT;
ALTER TABLE todo_tasks ADD COLUMN repeat_total INTEGER;
ALTER TABLE todo_tasks ADD COLUMN repeat_until TEXT;
ALTER TABLE todo_tasks ADD COLUMN target INTEGER;
ALTER TABLE todo_tasks ADD COLUMN unit TEXT;
ALTER TABLE todo_tasks ADD COLUMN period TEXT;

-- Occurrences done (repeat), days checked (checkin), amounts counted (progress).
CREATE TABLE todo_logs (
  id         INTEGER PRIMARY KEY,
  task_id    INTEGER NOT NULL REFERENCES todo_tasks(id) ON DELETE CASCADE,
  day        TEXT NOT NULL, -- YYYY-MM-DD (local)
  amount     INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX todo_logs_by_task ON todo_logs(task_id, day);
CREATE INDEX todo_logs_by_day ON todo_logs(day);
