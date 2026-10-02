-- 一步一步: simpler dates (validated in Rust).
--   normal / progress  no plan date: a task has nothing, or a [start, due] range (start optional)
--   repeat             due_date = the current occurrence (derived from the rule); plan_date = series
--                      start (monthly / after rules); done_before = occurrences done before logging,
--                      so occurrences done = done_before + logs
--   progress           period is always all
--   checkin            unchanged (plan_date = start)
ALTER TABLE todo_tasks ADD COLUMN done_before INTEGER NOT NULL DEFAULT 0;

-- A plan date becomes the range start (plan only: a one-day range).
UPDATE todo_tasks SET
  start_date = coalesce(start_date, CASE WHEN due_date IS NULL OR plan_date <= due_date THEN plan_date END),
  due_date = coalesce(due_date, plan_date),
  plan_date = NULL
WHERE plan_date IS NOT NULL AND task_type IN ('normal', 'progress');

UPDATE todo_tasks SET due_date = coalesce(due_date, plan_date), plan_date = NULL, start_date = NULL
WHERE task_type = 'repeat';

-- Progress tasks always count in total (per-day / week / month counting was dropped).
UPDATE todo_tasks SET period = 'all' WHERE task_type = 'progress';
