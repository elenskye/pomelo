-- 年度总结 (v1.0): whether a run keeps its conversation under 定时 (off: once the letter is written the conversation
-- goes; the tabloid and the letter stay in Atlas; a run that failed keeps its conversation). Only the yearly one uses it.
ALTER TABLE schedules ADD COLUMN keep_chat INTEGER NOT NULL DEFAULT 0;
