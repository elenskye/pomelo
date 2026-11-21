-- 念念不忘: only first-level tags can be pinned (a pinned one shows with its sub-tags). Sub-tags that
-- were pinned are unpinned; their icons stay.
UPDATE note_tag_meta SET pin_order = NULL WHERE instr(tag, '/') > 0;
DELETE FROM note_tag_meta WHERE pin_order IS NULL AND icon IS NULL;
