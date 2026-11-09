-- Dawn Plan · Phase 6: the conversation list has no 定时 list any more; what a 定时任务 writes lies in 最近 with
-- the rest (library.rs). The ones that were in 定时 give up the place they had there (a hand-arranged 最近 puts
-- them on top, like any newcomer), and 定时 is no longer a hand-arranged list.
UPDATE sessions SET sort_order = NULL
 WHERE scheduled = 1 AND pinned_at IS NULL AND archived_at IS NULL
   AND (project_id IS NULL OR project_id NOT IN (SELECT id FROM projects));
DELETE FROM manual_lists WHERE list = 'scheduled';
