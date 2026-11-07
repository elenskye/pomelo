-- 一网打尽: the top level holds folders only, a link is always in a folder. Links left at the top level go
-- into a new folder 「未分类」 (last at the top level), in their order.
INSERT INTO bookmarks (parent_id, kind, title, url, on_home, sort_order)
SELECT NULL, 'folder', '未分类', '', 0, coalesce((SELECT max(sort_order) FROM bookmarks WHERE parent_id IS NULL), 0) + 1
WHERE EXISTS (SELECT 1 FROM bookmarks WHERE parent_id IS NULL AND kind = 'link');
UPDATE bookmarks SET parent_id = (SELECT max(id) FROM bookmarks WHERE parent_id IS NULL AND kind = 'folder')
WHERE parent_id IS NULL AND kind = 'link';
