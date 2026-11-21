-- 柚子's module key was picky: the 模块坞, 页签, hidden modules, the old sidebar order and the
-- last page follow it to youzi; its setting picky.cloudGroups becomes youzi.cloudGroups.
UPDATE settings SET value = replace(value, '"picky"', '"youzi"')
WHERE key IN ('ui.dock', 'ui.module_hidden', 'ui.module_order') AND instr(value, '"picky"') > 0;
UPDATE settings SET value = replace(value, '"/picky', '"/youzi')
WHERE key IN ('ui.tabs', 'ui.last_route') AND instr(value, '"/picky') > 0;
UPDATE settings SET key = 'youzi.cloudGroups'
WHERE key = 'picky.cloudGroups' AND NOT EXISTS (SELECT 1 FROM settings WHERE key = 'youzi.cloudGroups');
DELETE FROM settings WHERE key = 'picky.cloudGroups';
