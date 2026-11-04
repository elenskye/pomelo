-- v0.7 refine R6: the rule group 共用准则 is gone (its points are the program's own now: seeker/model.rs
-- `PRINCIPLES`); its rules go, their last version to the history (like a delete), and so does the name
-- the user gave it. `seeker_groups` keeps every page's group order too (上移 / 下移, `sort`; none = after
-- the ordered ones, in the page's own order): 素材 (`material`, per language), 规则 (`rule`; the group
-- 豁免情况 is the category 「豁免情况」), 档案 (`fact`), 网申 (`application`, both kinds). `label` '' =
-- the group's own name. Rules in picky/assistants/seeker/catalog.rs (`order_groups`, `rename_group`).
INSERT INTO seeker_record_history (record_id, body)
    SELECT id, body FROM seeker_records WHERE json_extract(body, '$.kind') IN ('rule', 'decision') AND json_extract(body, '$.category') = '共用准则' ORDER BY sort;
DELETE FROM seeker_records WHERE json_extract(body, '$.kind') IN ('rule', 'decision') AND json_extract(body, '$.category') = '共用准则';
CREATE TABLE seeker_groups_new (
    kind TEXT NOT NULL CHECK (kind IN ('material', 'rule', 'fact', 'application')),
    lang TEXT NOT NULL,
    category TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '',
    sort INTEGER,
    PRIMARY KEY (kind, lang, category)
);
INSERT INTO seeker_groups_new (kind, lang, category, label) SELECT kind, lang, category, label FROM seeker_groups WHERE NOT (kind = 'rule' AND category = '共用准则');
DROP TABLE seeker_groups;
ALTER TABLE seeker_groups_new RENAME TO seeker_groups;
