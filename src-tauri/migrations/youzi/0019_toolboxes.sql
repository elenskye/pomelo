-- 小拾 (picky.db) v19: 改文件 becomes the plugin library's Word 工具箱 / 文本工具箱, used when 小拾
-- decides. artifacts.source_path: the file an attachment was added from (dialog or drop), the one a
-- 工具箱 changes; pasted images and saved web pages have none. A reply's card (file_edits) can also be a
-- new Word document: kind 'new-word', name, path = the folder it goes to (then the file written), its
-- Markdown in its one item's `after`.
ALTER TABLE artifacts ADD COLUMN source_path TEXT;
