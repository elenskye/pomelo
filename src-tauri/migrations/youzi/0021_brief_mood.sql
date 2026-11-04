-- 小拾 (picky.db) v21 (v0.7 phase 1): the 今日简报's mood for the pomelo's face.
-- calm | busy | good | worry; NULL = none given (older briefs, or the model left it out).
ALTER TABLE briefs ADD COLUMN mood TEXT;
