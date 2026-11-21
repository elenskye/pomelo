-- 念念不忘: tags dragged into an order of their own. sort_order = place among its siblings (same parent);
-- NULL = never placed (listed after the placed ones, in pinyin order). Moves with a rename like pin / icon.
ALTER TABLE note_tag_meta ADD COLUMN sort_order INTEGER;
