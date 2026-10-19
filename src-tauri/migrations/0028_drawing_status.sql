-- 随意画画 (v0.4.5): no 待标记 any more; such lessons become 未开始.
UPDATE drawing_lessons SET status = 'todo' WHERE status = 'unmarked';
