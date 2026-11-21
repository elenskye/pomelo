-- v0.7 refine R8: 复盘 · 记录. A review being recorded in a conversation: the 小拾 session it is on (kept
-- out of 小拾's list and searches; private) and the table so far (JSON `Draft`). It goes with its session;
-- 确认 makes it a review in rater_interviews and deletes the session. Rules in
-- picky/assistants/rater/record.rs.
CREATE TABLE rater_records (
    session_id INTEGER PRIMARY KEY NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    body TEXT NOT NULL
);
