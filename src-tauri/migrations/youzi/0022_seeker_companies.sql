-- Seeker (v0.7 阶段 ②): which company folder of knowledge/ a 投石问路 job uses when the automatic
-- match by company name is missing or wrong ('' = none). Rules in picky/assistants/seeker/company.rs.
CREATE TABLE seeker_job_companies (
    job_id INTEGER NOT NULL,
    job_created_at TEXT NOT NULL,
    folder TEXT NOT NULL,
    PRIMARY KEY (job_id, job_created_at)
);
