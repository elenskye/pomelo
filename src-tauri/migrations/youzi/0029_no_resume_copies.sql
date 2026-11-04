-- v0.7 refine R3: Seeker keeps no copies of résumés. 确认版本 goes (with the drafts kept there); an export
-- keeps its name, path, language, time and the materials it was made from (an interview reads their
-- 简历题 and 经历档案), not the draft or the template. Groups of 素材 / 规则 can show another name
-- (`seeker_groups`; a built-in group keeps what the program does with it). Rules in
-- picky/assistants/seeker/output.rs (`Export`) and catalog.rs (`rename_group`).
DROP TABLE seeker_versions;
UPDATE seeker_exports SET body = json_remove(json_set(body,
    '$.lang', coalesce(json_extract(body, '$.draft.lang'), 'cn'),
    '$.materials', json(coalesce((SELECT json_group_array(DISTINCT json_extract(e.value, '$.source.recordId'))
        FROM json_each(json_extract(seeker_exports.body, '$.draft.sections')) s, json_each(json_extract(s.value, '$.entries')) e
        WHERE json_extract(e.value, '$.source.recordId') IS NOT NULL), '[]'))),
  '$.draft', '$.template');
CREATE TABLE seeker_groups (
    kind TEXT NOT NULL CHECK (kind IN ('material', 'rule')),
    lang TEXT NOT NULL,
    category TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY (kind, lang, category)
);
