-- 投石问路 (job applications). Statuses are user-editable; each belongs to a fixed stage that
-- drives the rules: wish 准备 · applied 等回音 (follow-up reminders) · active 进行中 (step dates)
-- · got 拿到 · closed 结束. No status history: only the furthest status reached (for the funnel).
-- 渠道 = kinds (module 'job_channel').
CREATE TABLE job_statuses (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  stage      TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE jobs (
  id           INTEGER PRIMARY KEY,
  company      TEXT NOT NULL,
  position     TEXT,
  location     TEXT,
  link         TEXT,                                               -- JD
  kind_id      INTEGER REFERENCES kinds(id) ON DELETE SET NULL,    -- 渠道
  resume       TEXT,                                               -- file path (Seeker)
  cover_letter TEXT,
  deadline     TEXT,                                               -- YYYY-MM-DD
  salary       TEXT,
  note         TEXT,
  status_id    INTEGER NOT NULL REFERENCES job_statuses(id),
  applied_on   TEXT,                                               -- set when first marked applied
  step_on      TEXT,                                               -- date of the current step (笔试 / 面试)
  followed_on  TEXT,                                               -- last follow-up
  reached_id   INTEGER REFERENCES job_statuses(id) ON DELETE SET NULL, -- furthest status reached
  task_id      INTEGER REFERENCES todo_tasks(id) ON DELETE SET NULL,   -- linked 一步一步 task
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX jobs_by_status ON jobs(status_id);
CREATE INDEX jobs_by_kind ON jobs(kind_id);
CREATE INDEX jobs_by_task ON jobs(task_id);

INSERT INTO job_statuses (name, stage, sort_order) VALUES
  ('想投', 'wish', 1), ('已投递', 'applied', 2), ('网申测评 / 笔试', 'active', 3), ('面试', 'active', 4),
  ('Offer', 'got', 5), ('已接受', 'got', 6), ('被拒', 'closed', 7);

INSERT INTO kinds (module, name, sort_order) VALUES
  ('job_channel', '官网', 1), ('job_channel', '内推', 2), ('job_channel', 'Boss 直聘', 3), ('job_channel', '牛客', 4),
  ('job_channel', 'LinkedIn', 5), ('job_channel', '学校', 6), ('job_channel', '其他', 7);
