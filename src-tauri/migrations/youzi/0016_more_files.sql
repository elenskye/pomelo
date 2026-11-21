-- 小拾 (picky.db) v16 (v0.4.5 阶段 6): more kinds of files (Word, PowerPoint, Excel / CSV, web pages, images).
-- artifacts.source_url: a saved web page's address (its copy is the article's text).
-- artifacts.row_count: a table's data rows (all sheets); a big one keeps only a preview as text.
ALTER TABLE artifacts ADD COLUMN source_url TEXT;
ALTER TABLE artifacts ADD COLUMN row_count INTEGER;
