-- 小拾 (picky.db) v14 (v0.4, 第六轮问卷): 私密对话, 年度总结写的是哪一年.
-- private: 年度总结 doesn't read it and 小拾's pomelo_search / pomelo_read don't find it (⌘K and
-- 对话搜索 still do). Undoable like pin / archive.
ALTER TABLE sessions ADD COLUMN private INTEGER NOT NULL DEFAULT 0;

-- A 年度总结 reply and the year it wrote about (首页's new-year card; next year's comparison).
CREATE TABLE year_runs (
  run_id INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  year   TEXT NOT NULL
) STRICT;
CREATE INDEX year_runs_by_year ON year_runs(year);

-- The ready-made 年度总结's 要做什么, unless it was changed: how it's written now lives in yearly.rs.
UPDATE schedules SET prompt = '写我这一年的年度总结：一封写给我的信，陪我回看这一年。'
 WHERE builtin = 'yearly' AND prompt = '写一篇这一年的年度总结：读了什么、看了什么、做成了什么、画了多少，常在想的事和这一年的变化；按时间或主题分几块，最后写几句对明年的期待。';

-- 每月小结 are written differently now (keeping quotes, names, dates): the old ones go.
DELETE FROM month_summaries;
