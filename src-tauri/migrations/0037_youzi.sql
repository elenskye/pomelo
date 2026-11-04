-- 小拾 is now 柚子: the 模块坞 category still carrying the old default name follows.
UPDATE settings
SET value = (
  SELECT json_set(settings.value, '$.cats[' || c.key || '].name', '柚子')
  FROM json_each(settings.value, '$.cats') c
  WHERE json_extract(c.value, '$.name') = '小拾'
  LIMIT 1
)
WHERE key = 'ui.dock' AND json_valid(value)
  AND EXISTS (SELECT 1 FROM json_each(settings.value, '$.cats') c WHERE json_extract(c.value, '$.name') = '小拾');
