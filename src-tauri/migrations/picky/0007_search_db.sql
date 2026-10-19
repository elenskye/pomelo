-- 小拾 (picky.db) v7 (v0.4): project file indexes and memory vectors move to search.db (derived
-- data, rebuilt there in the background). Which files to index stays in project_files.indexed.
DROP TABLE artifact_chunks;
DROP TABLE artifact_indexes;
DROP TABLE memory_vectors;
