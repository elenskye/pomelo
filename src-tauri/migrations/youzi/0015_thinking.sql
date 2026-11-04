-- 小拾 (picky.db) v15 (v0.4 阶段 9): 思考.
-- sessions.thinking: the conversation's 思考 switch (on for new ones; replies ask the model to think
-- when it can). schedules.thinking: the 定时任务 wants thinking (its model must be able to: Rust checks).
ALTER TABLE sessions ADD COLUMN thinking INTEGER NOT NULL DEFAULT 1;
ALTER TABLE schedules ADD COLUMN thinking INTEGER NOT NULL DEFAULT 1;
