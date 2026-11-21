-- 摸鱼读读: add 类型. kind: fiction|nonfiction|comic|other (validated in Rust). Existing rows → fiction.
ALTER TABLE books ADD COLUMN kind TEXT NOT NULL DEFAULT 'fiction';
