-- Seeker (assistant): the remembered folder and workflow options, the one current draft, the user's
-- materials with their prior versions, confirmed resumes, the Word template, exports and job links.
-- Bodies are JSON; rules in picky/assistants/seeker.rs.
CREATE TABLE seeker_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    folder TEXT NOT NULL DEFAULT '',
    options TEXT NOT NULL DEFAULT '{}',
    template TEXT
);
CREATE TABLE seeker_draft (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    body TEXT NOT NULL,
    baseline TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE seeker_records (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT UNIQUE,
    body TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE seeker_record_history (
    id INTEGER PRIMARY KEY,
    record_id TEXT NOT NULL,
    body TEXT NOT NULL,
    saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE seeker_versions (
    id INTEGER PRIMARY KEY,
    label TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE seeker_exports (id TEXT PRIMARY KEY NOT NULL, body TEXT NOT NULL);
CREATE TABLE seeker_job_exports (
    id TEXT PRIMARY KEY NOT NULL,
    export_id TEXT NOT NULL REFERENCES seeker_exports(id) ON DELETE CASCADE,
    job_id INTEGER NOT NULL,
    job_created_at TEXT NOT NULL,
    body TEXT NOT NULL,
    current INTEGER NOT NULL DEFAULT 0 CHECK (current IN (0, 1)),
    UNIQUE (job_id, job_created_at, export_id)
);
CREATE UNIQUE INDEX seeker_job_current ON seeker_job_exports(job_id, job_created_at) WHERE current = 1;
