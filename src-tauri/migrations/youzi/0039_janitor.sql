-- DevJanitor's Agent (v0.8 阶段 8). janitor_rules: what the Agent proposed and I added to 一键清理 (a
-- virtual path, `*` matching within one folder name; undone with ⌘Z). janitor_cards: its proposals in a
-- reply — kind trash (paths to move to the Trash) | command (one shell command) | rule (a rule to add);
-- body: the card (JSON, janitor/agent.rs `Card`); state pending | running | done | failed | stopped |
-- cancelled; result: what happened (JSON). They go with their run.
CREATE TABLE janitor_rules (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  pattern    TEXT NOT NULL,
  recognize  TEXT NOT NULL DEFAULT '',
  rebuild    TEXT NOT NULL DEFAULT '',
  checked    INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE TABLE janitor_cards (
  id         INTEGER PRIMARY KEY,
  run_id     INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  body       TEXT NOT NULL,
  state      TEXT NOT NULL DEFAULT 'pending',
  result     TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX janitor_cards_by_run ON janitor_cards(run_id, id);
