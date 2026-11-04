-- 小拾 is now 柚子: what the user wrote for the model follows (memories, instructions, templates, presets,
-- schedule prompts, the brief / Translator / Importer prompts, Seeker rules). Messages and file paths stay.
UPDATE memories SET content = replace(content, '小拾', '柚子') WHERE instr(content, '小拾') > 0;
UPDATE global_instructions SET content = replace(content, '小拾', '柚子') WHERE instr(content, '小拾') > 0;
UPDATE projects SET instructions = replace(instructions, '小拾', '柚子') WHERE instr(instructions, '小拾') > 0;
UPDATE sessions SET instructions = replace(instructions, '小拾', '柚子') WHERE instr(instructions, '小拾') > 0;
UPDATE templates SET title = replace(title, '小拾', '柚子'), body = replace(body, '小拾', '柚子') WHERE instr(title || body, '小拾') > 0;
UPDATE presets SET name = replace(name, '小拾', '柚子'), content = replace(content, '小拾', '柚子') WHERE instr(name || content, '小拾') > 0;
UPDATE schedules SET prompt = replace(prompt, '小拾', '柚子') WHERE instr(prompt, '小拾') > 0;
UPDATE preferences SET value = replace(value, '小拾', '柚子')
WHERE key IN ('brief.prompt', 'assistant.translate', 'assistant.importer') AND instr(value, '小拾') > 0;
UPDATE seeker_records SET body = replace(body, '小拾', '柚子')
WHERE instr(body, '小拾') > 0 AND (CASE WHEN json_valid(body) THEN json_extract(body, '$.kind') END) = 'rule';
