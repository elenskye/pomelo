-- 小拾 (picky.db) v10 (v0.4 phase 5): edits on cards.
-- options: when the words fit several records, the candidates (JSON [{title, fields, input}]) and
-- chosen = the one confirmed; record_before: an edited record as it was before (to tell 已撤销).
ALTER TABLE card_items ADD COLUMN options TEXT;
ALTER TABLE card_items ADD COLUMN chosen INTEGER;
ALTER TABLE card_items ADD COLUMN record_before TEXT;
