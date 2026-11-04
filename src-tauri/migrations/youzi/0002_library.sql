-- 小拾 v2: projects, pin / archive, hand-arranged list order (from Picky's v3 sessions / projects).
-- instructions: the project's background instructions, sent with its conversations.
-- sort_order: projects are always in hand order (a new one goes first).
CREATE TABLE projects (
  id           INTEGER PRIMARY KEY,
  name         TEXT NOT NULL,
  instructions TEXT NOT NULL DEFAULT '',
  sort_order   INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- A pinned conversation shows only under 置顶 (it keeps its project); an archived one only under 归档.
-- sort_order: its place once its list was arranged by hand (manual_lists); NULL = arrived since,
-- shown first by last activity.
ALTER TABLE sessions ADD COLUMN project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL;
ALTER TABLE sessions ADD COLUMN pinned_at TEXT;
ALTER TABLE sessions ADD COLUMN archived_at TEXT;
ALTER TABLE sessions ADD COLUMN sort_order INTEGER;
CREATE INDEX sessions_by_project ON sessions(project_id);

-- Lists the user arranged by hand: 'pinned' | 'recent' | 'archived' | 'project:<id>'. Others follow
-- last activity.
CREATE TABLE manual_lists (
  list TEXT PRIMARY KEY
) STRICT;
