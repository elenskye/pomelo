-- 小拾 (picky.db) v11 (v0.4 phase 6): skills (插件库).
-- runs.skills: the skills a question asked for (「/书单检索」, the 联网 button), JSON array, NULL =
-- none; a new version of the question keeps them. run_sources.url: a web page source (kind web:
-- 联网搜索, 书单检索, 影单检索), opened in the browser.
ALTER TABLE runs ADD COLUMN skills TEXT;
ALTER TABLE run_sources ADD COLUMN url TEXT;
