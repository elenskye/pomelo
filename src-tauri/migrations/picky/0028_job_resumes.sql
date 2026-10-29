-- v0.7 refine R2: a job's résumés are files (a Pomelo export, or any résumé under cv/), by path; one is
-- 「本次使用」 per job. The export links move over with their export's path. Rules in
-- picky/assistants/seeker/output.rs (`associate`, `adopt_job_resumes`).
CREATE TABLE seeker_job_resumes (
    id TEXT PRIMARY KEY NOT NULL,
    job_id INTEGER NOT NULL,
    job_created_at TEXT NOT NULL,
    path TEXT NOT NULL,
    body TEXT NOT NULL,
    current INTEGER NOT NULL DEFAULT 0 CHECK (current IN (0, 1)),
    UNIQUE (job_id, job_created_at, path)
);
CREATE UNIQUE INDEX seeker_job_resume_current ON seeker_job_resumes(job_id, job_created_at) WHERE current = 1;
INSERT OR IGNORE INTO seeker_job_resumes (id, job_id, job_created_at, path, body, current)
SELECT l.id, l.job_id, l.job_created_at, json_extract(e.body, '$.path'), json_set(json_remove(l.body, '$.exportId'), '$.path', json_extract(e.body, '$.path')), l.current
FROM seeker_job_exports l JOIN seeker_exports e ON e.id = l.export_id
ORDER BY l.rowid;
DROP TABLE seeker_job_exports;
