-- 小拾 (picky.db) v6: what a reply looked up in Pomelo (v0.3.5 「问我的数据」, picky/data.rs).
-- One row per tool call of a reply's run, in call order (round = which model request asked).
-- module: the Pomelo module asked (NULL when the call named no known tool).
-- state: ok | blocked (hidden module, or not for this cloud model) | failed | unsupported (the
--   model service refused tools; the reply went on without them).
-- label: the 「查了」 line, e.g. 钱来钱往 · 2026 年 9 月支出（42 笔）.
-- result: exactly what the model got (follow-ups, old conversations and exports use it).
-- records: JSON list of the records in it ({module, kind, id, title, parent, day}) so the answer
--   can link them.
-- call_id / tool / arguments rebuild the exchange for follow-up questions.
CREATE TABLE run_queries (
  run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  position   INTEGER NOT NULL,
  round      INTEGER NOT NULL,
  call_id    TEXT NOT NULL,
  tool       TEXT NOT NULL,
  arguments  TEXT NOT NULL,
  module     TEXT,
  label      TEXT NOT NULL,
  state      TEXT NOT NULL,
  result     TEXT NOT NULL,
  records    TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (run_id, position)
) STRICT;
