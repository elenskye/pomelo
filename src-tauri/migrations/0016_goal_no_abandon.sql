-- 一步一步: goals no longer have 「已放弃」 (an unwanted goal is deleted instead). Any left over
-- become 暂停 so they stay visible and can be deleted by hand.
UPDATE todo_goals SET status = 'paused' WHERE status = 'abandoned';
