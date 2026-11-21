-- Rater 面试 (v0.7 阶段 ⑦): 准备 (prep: the resume and JD it was made from, the questions picked, likely
-- follow-ups, the one-page 速查卡) and 复盘 (review: what was asked and answered, the feedback, the
-- comparison with the prep). One row each, a JSON body, under a 投石问路 job. Rules in
-- picky/assistants/rater/interview.rs.
CREATE TABLE rater_interviews (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    job_id INTEGER NOT NULL,
    job_created_at TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX rater_interviews_job ON rater_interviews (job_id, job_created_at);
