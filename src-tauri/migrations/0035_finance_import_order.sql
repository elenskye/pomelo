-- 导入账单 list: own order (newest first as before) and a name of its own (null = the file name, kept for 选原图 / AI 复核).
ALTER TABLE fin_imports ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
ALTER TABLE fin_imports ADD COLUMN title TEXT;
UPDATE fin_imports SET sort_order = (SELECT count(*) FROM fin_imports b WHERE b.id >= fin_imports.id);
