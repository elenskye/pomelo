-- Seeker (v0.7 阶段 ③): a Chinese and an English resume. The one draft row becomes one per language
-- ('cn' keeps the current draft), the English Word template gets its own column, and the rules
-- that came from SEEKER.md (or are 共用准则) become shared by both languages (records' JSON `lang`:
-- 'cn' when missing, 'en', '' = both). Rules in picky/assistants/seeker.rs.
CREATE TABLE seeker_draft_new (
    lang TEXT PRIMARY KEY NOT NULL CHECK (lang IN ('cn', 'en')),
    body TEXT NOT NULL,
    baseline TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO seeker_draft_new (lang, body, baseline, updated_at) SELECT 'cn', body, baseline, updated_at FROM seeker_draft;
DROP TABLE seeker_draft;
ALTER TABLE seeker_draft_new RENAME TO seeker_draft;
ALTER TABLE seeker_settings ADD COLUMN template_en TEXT;
UPDATE seeker_records SET body = json_set(body, '$.lang', '')
WHERE json_extract(body, '$.kind') IN ('rule', 'decision', 'example')
  AND (source LIKE 'SEEKER.md#%' OR json_extract(body, '$.category') = '共用准则');
