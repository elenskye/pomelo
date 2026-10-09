-- 摸鱼读读 / 好奇看看: 完成时间 is a year now ('YYYY', was 'YYYY-MM'). 随意画画 keeps its month.
ALTER TABLE books RENAME COLUMN finished_month TO finished_year;
UPDATE books SET finished_year = substr(finished_year, 1, 4) WHERE finished_year IS NOT NULL;
ALTER TABLE films RENAME COLUMN finished_month TO finished_year;
UPDATE films SET finished_year = substr(finished_year, 1, 4) WHERE finished_year IS NOT NULL;
