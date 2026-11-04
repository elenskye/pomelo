-- 小拾 (picky.db) v32: 思考强度.
-- sessions.effort: the level a conversation chose (NULL = its model's default).
-- cloud.models: each model now lists the levels it takes; models saved before get what their service documents (the
-- user can change them), so a level chosen in Pub2MD keeps working. A model that can't think, or Aliyun's (a switch only), none.
ALTER TABLE sessions ADD COLUMN effort TEXT;
UPDATE preferences SET value = (
  SELECT json_group_array(json_set(m.value, '$.efforts', json(CASE
    WHEN json_extract(m.value, '$.thinking') = 'never' THEN '[]'
    WHEN json_extract(m.value, '$.service') = 'deepSeek' THEN '["low","high","max"]'
    WHEN json_extract(m.value, '$.service') = 'anthropic' THEN '["low","medium","high","xhigh","max"]'
    WHEN json_extract(m.value, '$.service') = 'aliyun' THEN '[]'
    ELSE '["low","medium","high"]'
  END)))
  FROM json_each(preferences.value) AS m
) WHERE key = 'cloud.models' AND json_valid(value);
