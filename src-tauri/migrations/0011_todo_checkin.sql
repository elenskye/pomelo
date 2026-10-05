-- 一步一步: check-ins use the optional [start, due] range like normal / progress tasks (no target
-- days; the check-in completes once its range is over). Monthly repeats end by count like daily /
-- weekly ones. repeat_until is no longer used.
UPDATE todo_tasks SET
  start_date = CASE WHEN repeat_until IS NOT NULL THEN plan_date END,
  due_date = repeat_until,
  plan_date = NULL, repeat_until = NULL, repeat_total = NULL
WHERE task_type = 'checkin';

UPDATE todo_tasks SET plan_date = NULL, repeat_until = NULL
WHERE task_type = 'repeat' AND repeat_rule LIKE 'monthly:%';
