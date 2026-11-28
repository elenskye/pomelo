-- 1.2.0 (requirements「柚子 › 搜索和出处」): a citation of a PDF page (柚子's pdf_* tools) keeps where it is: the physical
-- page (from 1), the page label printed on it, and the boxes pdf_find matched (JSON [[x, y, w, h], …] in PDF points,
-- bottom-left origin, CropBox offset kept). NULL for every other citation.
ALTER TABLE run_sources ADD COLUMN pdf_page INTEGER;
ALTER TABLE run_sources ADD COLUMN pdf_label TEXT;
ALTER TABLE run_sources ADD COLUMN pdf_rects TEXT;
