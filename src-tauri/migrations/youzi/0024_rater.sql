-- Rater (v0.7 阶段 ④): the interview question bank. A question is a JSON body (category, question,
-- answer, tags, what it is about); every replaced or deleted version goes to the history. Its rules
-- are Seeker records (seeker_records, categories 题库维护 / 面试). Rules in picky/assistants/rater.rs.
CREATE TABLE rater_questions (
    id TEXT PRIMARY KEY NOT NULL,
    body TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE rater_question_history (
    id INTEGER PRIMARY KEY,
    question_id TEXT NOT NULL,
    body TEXT NOT NULL,
    saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
