-- 小拾 (picky.db) v12 (v0.4 phase 7): 定时任务.
-- every: day | week | month; day: week 1 (周一) … 7, month 1 … 31 (past the month's end = its last
-- day), NULL for day; at: HH:MM local time. model: local:<key> | cloud:<id>, NULL = what 小拾 has
-- selected. builtin: weekly | monthly for the ready-made ones (deleting one doesn't bring it back).
-- since: only times after it count (set when made, when its time changes and when switched on), so a
-- new or changed one never runs for the past.
CREATE TABLE schedules (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  prompt     TEXT NOT NULL,
  every      TEXT NOT NULL,
  day        INTEGER,
  at         TEXT NOT NULL,
  model      TEXT,
  enabled    INTEGER NOT NULL DEFAULT 1,
  builtin    TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  since      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- The scheduler's own bookkeeping (never in undo steps, so it can't block one): the latest time
-- ("YYYY-MM-DD HH:MM", local) each schedule ran for.
CREATE TABLE schedule_state (
  schedule_id INTEGER PRIMARY KEY REFERENCES schedules(id) ON DELETE CASCADE,
  last_slot   TEXT NOT NULL
) STRICT;

-- A conversation a schedule wrote: scheduled (stays when the schedule is deleted; listed under 定时),
-- schedule_id (which one), unseen (a dot until opened).
ALTER TABLE sessions ADD COLUMN scheduled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN schedule_id INTEGER REFERENCES schedules(id) ON DELETE SET NULL;
ALTER TABLE sessions ADD COLUMN unseen INTEGER NOT NULL DEFAULT 0;
CREATE INDEX sessions_schedule ON sessions(schedule_id);

INSERT INTO schedules (name, prompt, every, day, at, builtin, sort_order) VALUES
  ('每周回顾', '回顾我上周（周一到周日）：做完了哪些事、还拖着没做完的、花了多少钱（主要花在哪）、找工作的投递有什么进展。分几块写，简洁；最后给这周一两条建议。', 'week', 1, '09:00', 'weekly', 0),
  ('每月回顾', '回顾我上个月：收支和总资产的变化、读完看完的书和影视、画了多少。分几块写，简洁；和再上一个月比一比。', 'month', 1, '09:00', 'monthly', 1);
