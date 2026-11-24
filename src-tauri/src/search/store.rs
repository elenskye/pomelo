//! search.db (v0.4): everything search computes from the data, next to pomelo.db and youzi.db.
//! Never backed up, exported or undone: a missing, broken or older-version file is deleted and
//! created again, and the indexer refills it in the background (`meta.rebuild` = 1).
//! - docs + docs_fts: one row per searchable record (indexer.rs, sources.rs) and its word index
//!   (contentless FTS5 over text.rs tokens; rowid = docs.id). hash covers everything stored,
//!   vhash what the vectors are made from.
//! - vectors: per model, one vector per passage of a doc (int8, vectors.rs), with the doc's hash
//!   when computed (a different hash = stale).
//! - file_indexes / file_chunks: 柚子 project file passages and vectors (f32), per model and
//!   artifact (sha256 guards against a restored youzi.db reusing the id for other bytes).
//! - memory_vectors: 柚子 memory vectors (f32), with the text hash they were made from.
//! - kb_docs / kb_chunks (v2, 知识库): every readable file a library folder holds (path relative to
//!   the folder; a saved web page is one doc with path ""), with the size and mtime last seen and
//!   what its passages were made from (sha256, model); its passages (label, text, int8 vector).
//!   youzi/knowledge.rs keeps them in line with the folders.
//! - v3: 柚子's docs are module `youzi` (were `picky`); nothing else changed.

use crate::db;
use rusqlite::Connection;
use std::path::Path;

pub const VERSION: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;
CREATE TABLE docs (
  id     INTEGER PRIMARY KEY,
  module TEXT NOT NULL,
  kind   TEXT NOT NULL,
  ref_id INTEGER NOT NULL,
  title  TEXT NOT NULL,
  detail TEXT,
  parent INTEGER,
  day    TEXT,
  at     TEXT NOT NULL,
  body   TEXT NOT NULL,
  hash   INTEGER NOT NULL,
  vhash  INTEGER NOT NULL,
  UNIQUE (module, kind, ref_id)
) STRICT;
CREATE VIRTUAL TABLE docs_fts USING fts5(tokens, content='', contentless_delete=1, tokenize='unicode61 remove_diacritics 2');
CREATE TABLE vectors (
  doc_id  INTEGER NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
  model   TEXT NOT NULL,
  ordinal INTEGER NOT NULL,
  hash    INTEGER NOT NULL,
  vector  BLOB NOT NULL,
  PRIMARY KEY (model, doc_id, ordinal)
) STRICT;
CREATE INDEX vectors_by_doc ON vectors(doc_id);
CREATE TABLE file_indexes (
  artifact_id INTEGER NOT NULL,
  model       TEXT NOT NULL,
  sha256      TEXT NOT NULL,
  chunk_count INTEGER NOT NULL,
  built_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  PRIMARY KEY (artifact_id, model)
) STRICT;
CREATE TABLE file_chunks (
  artifact_id     INTEGER NOT NULL,
  model           TEXT NOT NULL,
  ordinal         INTEGER NOT NULL,
  segment_ordinal INTEGER NOT NULL,
  content         TEXT NOT NULL,
  vector          BLOB NOT NULL,
  PRIMARY KEY (artifact_id, model, ordinal),
  FOREIGN KEY (artifact_id, model) REFERENCES file_indexes(artifact_id, model) ON DELETE CASCADE
) STRICT;
CREATE TABLE memory_vectors (
  memory_id    INTEGER PRIMARY KEY,
  model        TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  vector       BLOB NOT NULL
) STRICT;
CREATE TABLE kb_docs (
  id        INTEGER PRIMARY KEY,
  source_id INTEGER NOT NULL,
  path      TEXT NOT NULL,
  size      INTEGER NOT NULL,
  mtime     INTEGER NOT NULL,
  sha256    TEXT,
  model     TEXT,
  fresh     INTEGER NOT NULL DEFAULT 0,
  error     TEXT,
  UNIQUE (source_id, path)
) STRICT;
CREATE TABLE kb_chunks (
  doc_id  INTEGER NOT NULL REFERENCES kb_docs(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  segment INTEGER NOT NULL,
  label   TEXT NOT NULL,
  content TEXT NOT NULL,
  vector  BLOB NOT NULL,
  PRIMARY KEY (doc_id, ordinal)
) STRICT;
INSERT INTO meta (key, value) VALUES ('rebuild', '1');
";

const DB: db::Derived = db::Derived { what: "搜索索引", not_it: "不是搜索索引", version: VERSION, schema: SCHEMA, probe: "docs" };

pub use crate::db::{meta, remove_files as remove, set_meta};

/// Opens search.db, starting over (delete + create) when it can't be used as is.
pub fn open(path: &Path) -> Result<Connection, String> {
    DB.open(path)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn memory_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::configure(&c).unwrap();
        DB.prepare(&c).unwrap();
        c
    }

    #[test]
    fn a_fresh_index_asks_for_a_rebuild_and_bad_files_start_over() {
        let c = memory_db();
        assert_eq!(meta(&c, "rebuild").unwrap().as_deref(), Some("1"));
        set_meta(&c, "rebuild", "0").unwrap();
        assert_eq!(meta(&c, "rebuild").unwrap().as_deref(), Some("0"));
        DB.prepare(&c).unwrap(); // the current version is kept as is

        let dir = crate::db::tests::temp_dir("search-store");
        let path = dir.join("search.db");
        std::fs::write(&path, b"definitely not a database, just some bytes here").unwrap();
        let c = open(&path).unwrap();
        assert_eq!(meta(&c, "rebuild").unwrap().as_deref(), Some("1"));
        c.pragma_update(None, "user_version", 99).unwrap();
        drop(c);
        let c = open(&path).unwrap(); // a newer / older layout: deleted and made again
        assert_eq!(db::user_version(&c).unwrap(), VERSION);
    }
}
