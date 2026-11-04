-- 小拾 (picky.db) v33: ASK. A reply's questions to me, as cards. state: waiting | answered | cancelled.
-- questions: JSON [{ question, kind: single | multi | judge, options }]; answers: JSON [{ picked, other }].
CREATE TABLE run_asks (
  id          INTEGER PRIMARY KEY,
  run_id      INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  questions   TEXT NOT NULL,
  answers     TEXT,
  state       TEXT NOT NULL DEFAULT 'waiting',
  created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  answered_at TEXT
) STRICT;
CREATE INDEX run_asks_run ON run_asks(run_id);
