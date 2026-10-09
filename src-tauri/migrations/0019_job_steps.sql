-- 投石问路, round 6: each application has its own steps (the statuses it really goes through, in the
-- status list's order), each checked with the day it was done. Nothing checked = 想投 (准备 stage
-- statuses are never steps). Current status = the last checked step; 投递日期 = the first checked day.
-- Gone: 截止日期 / 薪资 / 求职信 (old values move to the end of 备注), 投递日期, the step date,
-- 最远到过, and the link to 一步一步 (tasks it made stay as ordinary tasks).
-- Old rows → steps: every status that isn't 准备 / 结束, plus the current status if it is 结束.
-- Checked: those up to 最远到过 (or the current status), and the 结束 one. Days: 等回音 steps get
-- 投递日期, the current 进行中 step its step date; others are checked without a day.

CREATE TABLE jobs_new (
  id          INTEGER PRIMARY KEY,
  company     TEXT NOT NULL,
  position    TEXT,
  location    TEXT,
  link        TEXT,                                            -- JD
  kind_id     INTEGER REFERENCES kinds(id) ON DELETE SET NULL, -- 渠道
  resume      TEXT,                                            -- file path (Seeker)
  note        TEXT,
  followed_on TEXT,                                            -- last follow-up
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO jobs_new (id, company, position, location, link, kind_id, resume, note, followed_on, created_at, updated_at)
  SELECT id, company, position, location, link, kind_id, resume,
    nullif(trim(coalesce(note, '')
      || CASE WHEN deadline IS NULL THEN '' ELSE char(10) || '截止日期：' || deadline END
      || CASE WHEN salary IS NULL THEN '' ELSE char(10) || '薪资：' || salary END
      || CASE WHEN cover_letter IS NULL THEN '' ELSE char(10) || '求职信：' || cover_letter END, char(10)), ''),
    followed_on, created_at, updated_at
  FROM jobs;

CREATE TABLE job_steps (
  id         INTEGER PRIMARY KEY,
  job_id     INTEGER NOT NULL REFERENCES jobs_new(id) ON DELETE CASCADE,
  status_id  INTEGER NOT NULL REFERENCES job_statuses(id), -- a status in use can't be deleted (checked in Rust)
  done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
  done_on    TEXT,                                         -- YYYY-MM-DD, only when done (may be unknown)
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (job_id, status_id)
) STRICT;
INSERT INTO job_steps (job_id, status_id, done, done_on)
  SELECT j.id, s.id,
    s.stage = 'closed' OR s.sort_order <= coalesce(r.sort_order, -1),
    CASE
      WHEN NOT (s.stage = 'closed' OR s.sort_order <= coalesce(r.sort_order, -1)) THEN NULL
      WHEN s.id = j.status_id AND s.stage = 'active' THEN j.step_on
      WHEN s.stage = 'applied' THEN j.applied_on
    END
  FROM jobs j
  JOIN job_statuses s ON s.stage NOT IN ('wish', 'closed') OR (s.stage = 'closed' AND s.id = j.status_id)
  LEFT JOIN job_statuses r ON r.id = coalesce(j.reached_id, j.status_id) AND r.stage <> 'closed';

-- the old table is nobody's parent; job_steps' FK follows the rename
DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;
CREATE INDEX jobs_by_kind ON jobs(kind_id);
CREATE INDEX job_steps_by_status ON job_steps(status_id);
