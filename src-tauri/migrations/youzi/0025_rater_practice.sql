-- Rater 卡片练习 (v0.7 阶段 ⑥): one row per round, a JSON body (how the cards were picked, each card's
-- mark and typed text, follow-up cards, feedback). A question's practice record is worked out from
-- the rounds, never stored on the question. Rules in picky/assistants/rater/practice.rs.
CREATE TABLE rater_rounds (
    id TEXT PRIMARY KEY NOT NULL,
    body TEXT NOT NULL,
    started_at TEXT NOT NULL
);
