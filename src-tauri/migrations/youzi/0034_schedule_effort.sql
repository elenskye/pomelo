-- schedules.effort: a 定时任务's 思考强度 (NULL: the model's default; a word the model doesn't have counts as that too).
ALTER TABLE schedules ADD COLUMN effort TEXT;
