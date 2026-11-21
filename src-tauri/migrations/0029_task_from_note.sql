-- 笔记转待办 (v0.4.5): the note a task was made from (deleting the note leaves the task, unlinked).
ALTER TABLE todo_tasks ADD COLUMN from_note INTEGER REFERENCES notes (id) ON DELETE SET NULL;
CREATE INDEX todo_tasks_from_note ON todo_tasks (from_note);
