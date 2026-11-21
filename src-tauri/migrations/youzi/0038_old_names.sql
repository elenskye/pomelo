-- 柚子's code name was picky: the module key it left in sources and lookups becomes youzi, the 来源
-- label 「小拾对话」 says 柚子对话, and the old Picky app's import note (unused) goes. Messages, the
-- model's calls and what they returned stay as they were.
UPDATE run_sources SET module = 'youzi' WHERE module = 'picky';
UPDATE run_sources SET detail = '柚子对话' || substr(detail, 5) WHERE module = 'youzi' AND substr(detail, 1, 4) = '小拾对话';
UPDATE run_queries SET module = 'youzi' WHERE module = 'picky';
UPDATE run_queries SET records = replace(records, '"module":"picky"', '"module":"youzi"') WHERE instr(records, '"module":"picky"') > 0;
UPDATE briefs SET left_out = replace(left_out, '"picky"', '"youzi"') WHERE instr(left_out, '"picky"') > 0;
DELETE FROM preferences WHERE key = 'import.picky';
