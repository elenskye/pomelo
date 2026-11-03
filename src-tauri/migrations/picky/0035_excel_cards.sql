-- 小拾 (picky.db) v35 (v0.8): Excel 工具箱. A card's kind can be 'excel' (changes to cells and rows) or 'new-excel'
-- (a new workbook). file_edit_items.label: where a change is, as the card says it (表「预算」B3); payload: the change
-- itself as JSON (excel.rs `Op`; a new workbook's sheets). Both empty for the other kinds.
ALTER TABLE file_edit_items ADD COLUMN label TEXT;
ALTER TABLE file_edit_items ADD COLUMN payload TEXT;
