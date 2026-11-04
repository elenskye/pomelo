-- v0.7 refine: the user's own order (drag) for Seeker's records and Rater's questions. `sort` keeps
-- the order they were added in until moved; a new row goes last (max + 1). Rules in
-- picky/assistants/seeker/catalog.rs `reorder` and rater.rs `reorder`.
ALTER TABLE seeker_records ADD COLUMN sort INTEGER NOT NULL DEFAULT 0;
UPDATE seeker_records SET sort = rowid;
ALTER TABLE rater_questions ADD COLUMN sort INTEGER NOT NULL DEFAULT 0;
UPDATE rater_questions SET sort = rowid;
