-- 一步一步: a group and a goal can wear an icon from the icon library (`i:<key>`, like a 念念不忘 tag's). Names are
-- left as they are (an emoji typed at the start of a name stays in the name).
ALTER TABLE todo_groups ADD COLUMN icon TEXT;
ALTER TABLE todo_goals ADD COLUMN icon TEXT;
