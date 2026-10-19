//! Keeps search.db in line with pomelo.db and picky.db, in the background (`spawn`, every 2 s):
//! - Changes: its own read-only connections watch `PRAGMA data_version` (it moves whenever the
//!   app's connection commits: edits, undo, import, restore, 小拾 messages). Then every source is
//!   read again and compared by hash: new / changed docs are written with their word tokens;
//!   docs whose vector text changed lose their vectors; deleted records are removed.
//! - Vectors (the user's choice): computed only while the embedding service runs anyway, or when
//!   a meaning search / 小拾 needs them (`fill` before searching), except a rebuild — fresh
//!   search.db, other embedding model, other text rules — which starts the service and fills
//!   everything (`meta.rebuild`), newest records first. Once complete, other models' vectors,
//!   file indexes and memory vectors are dropped.
//! - Project files whose index is missing for the current model are queued again (picky::index)
//!   on launch and when a rebuild starts; memory vectors refill when memories are next picked.
//! - search.db deleted while running: opened again empty (= rebuild).

use super::sources::{self, Db, Row, SOURCES};
use super::store;
use super::text::{self, RULES};
use super::vectors::{self, Entry, Flat, VectorIndex};
use crate::modules::common::{err, Tx};
use crate::paths::DataPaths;
use crate::picky::catalog::ServiceModel;
use crate::picky::embed::{self, EmbedError, Input};
use crate::picky::runtime::{ServiceState, Svc};
use crate::AppState;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const TICK: Duration = Duration::from_secs(2);
/// Passages embedded per round (a rebuild loops quickly until done).
const FILL_TEXTS: usize = 96;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    /// Searchable records (docs).
    pub records: i64,
    /// Records whose vectors are current for the embedding model.
    pub done: i64,
    /// A rebuild is filling every record's vectors.
    pub rebuilding: bool,
    /// Vectors are being computed right now.
    pub filling: bool,
    pub model: Option<String>,
    pub bytes: u64,
    pub error: Option<String>,
}

/// The indexer's own read-only connections and what they saw last.
#[derive(Default)]
struct Readers {
    pomelo: Option<Connection>,
    picky: Option<Connection>,
    versions: [Option<i64>; 2],
}

pub struct Search {
    path: PathBuf,
    pomelo_path: PathBuf,
    picky_path: PathBuf,
    conn: Mutex<Option<Connection>>,
    readers: Mutex<Readers>,
    flat: RwLock<Option<Box<dyn VectorIndex>>>,
    status: Mutex<IndexStatus>,
    fill_lock: tokio::sync::Mutex<()>,
    /// After a failed start or fill, background filling waits until then.
    retry_at: Mutex<Option<std::time::Instant>>,
}

/// A doc that needs vectors.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: i64,
    pub source: usize,
    pub title: String,
    pub body: String,
    pub vhash: i64,
}

fn source_index(module: &str, kind: &str) -> Option<usize> {
    sources::source(module, kind).map(|(i, _)| i)
}

impl Search {
    pub fn open(paths: &DataPaths) -> Self {
        let (conn, error) = match store::open(&paths.search_db) {
            Ok(c) => (Some(c), None),
            Err(e) => (None, Some(e)),
        };
        Search {
            path: paths.search_db.clone(),
            pomelo_path: paths.db.clone(),
            picky_path: paths.picky_db.clone(),
            conn: Mutex::new(conn),
            readers: Mutex::new(Readers::default()),
            flat: RwLock::new(None),
            status: Mutex::new(IndexStatus { error, ..IndexStatus::default() }),
            fill_lock: tokio::sync::Mutex::new(()),
            retry_at: Mutex::new(None),
        }
    }

    pub fn with_db<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let guard = self.conn.lock().map_err(|_| "搜索索引的连接异常，请重启 App".to_string())?;
        match guard.as_ref() {
            Some(c) => f(c),
            None => Err(self.status().error.unwrap_or_else(|| "搜索索引没有打开".into())),
        }
    }

    pub fn status(&self) -> IndexStatus {
        let mut s = self.status.lock().unwrap_or_else(|e| e.into_inner()).clone();
        s.bytes = [self.path.clone(), self.path.with_extension("db-wal")].iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        s
    }

    fn set_status(&self, f: impl FnOnce(&mut IndexStatus)) {
        f(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// search.db was deleted (or never opened): start again with an empty one.
    fn reopen_if_missing(&self) {
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() && self.path.exists() {
            return;
        }
        *guard = None;
        store::remove(&self.path);
        let opened = store::open(&self.path);
        let error = opened.as_ref().err().cloned();
        *guard = opened.ok();
        drop(guard);
        *self.flat.write().unwrap_or_else(|e| e.into_inner()) = None;
        self.readers.lock().unwrap_or_else(|e| e.into_inner()).versions = [None, None];
        self.set_status(|s| s.error = error);
    }

    // ----- docs -----

    /// Reads the changed databases through the indexer's own connections and syncs their sources.
    /// Returns whether anything was compared.
    fn sync_changed(&self) -> Result<bool, String> {
        let mut r = self.readers.lock().unwrap_or_else(|e| e.into_inner());
        let open = |p: &Path| -> Option<Connection> {
            if !p.exists() {
                return None;
            }
            let c = Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
            c.busy_timeout(Duration::from_secs(5)).ok()?;
            Some(c)
        };
        if r.pomelo.is_none() {
            r.pomelo = open(&self.pomelo_path);
        }
        if r.picky.is_none() {
            r.picky = open(&self.picky_path);
        }
        let mut any = false;
        for (i, db) in [Db::Pomelo, Db::Picky].into_iter().enumerate() {
            let conn = if db == Db::Pomelo { r.pomelo.as_ref() } else { r.picky.as_ref() };
            let Some(conn) = conn else { continue };
            let Ok(v) = conn.query_row("PRAGMA data_version", [], |x| x.get::<_, i64>(0)) else { continue };
            if r.versions[i] == Some(v) {
                continue;
            }
            self.sync_db(db, conn)?;
            r.versions[i] = Some(v);
            any = true;
        }
        Ok(any)
    }

    /// Brings every source of `db` up to date (read first, then written under the search.db lock).
    pub fn sync_db(&self, db: Db, src: &Connection) -> Result<(), String> {
        let mut stale: HashSet<i64> = HashSet::new();
        for (i, s) in SOURCES.iter().enumerate().filter(|(_, s)| s.db == db) {
            let rows = match sources::read(src, s) {
                Ok(rows) => rows,
                Err(_) if db == Db::Picky => continue, // a picky.db that failed to open/migrate
                Err(e) => return Err(e),
            };
            stale.extend(self.with_db(|c| apply(c, i, rows))?);
        }
        if !stale.is_empty() {
            if let Some(f) = self.flat.write().unwrap_or_else(|e| e.into_inner()).as_mut() {
                f.remove(&stale);
            }
        }
        Ok(())
    }

    // ----- vectors -----

    /// Embedding model change or new text rules: start a rebuild. Returns whether one started.
    fn check_model(&self, model: &str) -> Result<bool, String> {
        let started = self.with_db(|c| {
            let mut started = false;
            if store::meta(c, "rules")?.as_deref() != Some(RULES.to_string().as_str()) {
                let tx = Tx::begin(c)?;
                tx.execute_batch("DELETE FROM vectors; DELETE FROM docs; DELETE FROM docs_fts;").map_err(err)?;
                store::set_meta(&tx, "rules", &RULES.to_string())?;
                store::set_meta(&tx, "rebuild", "1")?;
                tx.commit()?;
                started = true;
            }
            if store::meta(c, "model")?.as_deref() != Some(model) {
                store::set_meta(c, "model", model)?;
                store::set_meta(c, "rebuild", "1")?;
                started = true;
            }
            Ok(started)
        })?;
        if started {
            *self.flat.write().unwrap_or_else(|e| e.into_inner()) = None;
            self.readers.lock().unwrap_or_else(|e| e.into_inner()).versions = [None, None];
        }
        Ok(started)
    }

    fn rebuilding(&self) -> bool {
        self.with_db(|c| store::meta(c, "rebuild")).ok().flatten().as_deref() == Some("1")
    }

    /// A rebuild is complete: keep only this model's vectors.
    fn finish_rebuild(&self, model: &str) -> Result<(), String> {
        self.with_db(|c| {
            let tx = Tx::begin(c)?;
            for table in ["vectors", "file_indexes", "memory_vectors"] {
                tx.execute(&format!("DELETE FROM {table} WHERE model <> ?1"), [model]).map_err(err)?;
            }
            store::set_meta(&tx, "rebuild", "0")?;
            tx.commit()
        })
    }

    /// Embeds docs that need vectors (newest first, about FILL_TEXTS passages). Returns how
    /// many docs got them.
    pub async fn fill(&self, app: &AppHandle, model: &ServiceModel) -> Result<usize, String> {
        let _one = self.fill_lock.lock().await;
        let docs = self.with_db(|c| pending(c, &model.key, FILL_TEXTS))?;
        if docs.is_empty() {
            return Ok(0);
        }
        self.set_status(|s| s.filling = true);
        let out = self.embed_and_save(app, model, &docs).await;
        self.set_status(|s| s.filling = false);
        out
    }

    async fn embed_and_save(&self, app: &AppHandle, model: &ServiceModel, docs: &[Pending]) -> Result<usize, String> {
        let rt = &app.state::<AppState>().picky.runtime;
        let mut texts = vec![];
        let mut owners = vec![];
        for (i, d) in docs.iter().enumerate() {
            for (o, p) in text::passages(&d.title, &d.body).into_iter().enumerate() {
                texts.push(format!("{}：{p}", SOURCES[d.source].label));
                owners.push((i, o));
            }
            if texts.len() >= FILL_TEXTS {
                break;
            }
        }
        let n = owners.last().map_or(0, |o| o.0 + 1);
        let vecs = match embed::embed(rt, model, &texts, Input::Document).await {
            Ok(v) => v,
            // one passage the service refuses as too long: embed one by one, shortening those
            Err(EmbedError::TooLong) => {
                let mut v = vec![];
                for t in &texts {
                    let one = match embed::embed(rt, model, std::slice::from_ref(t), Input::Document).await {
                        Err(EmbedError::TooLong) => embed::embed(rt, model, &[t.chars().take(400).collect()], Input::Document).await,
                        r => r,
                    };
                    v.push(one.map_err(String::from)?.remove(0));
                }
                v
            }
            Err(e) => return Err(e.into()),
        };
        let mut per_doc: Vec<Vec<Vec<f32>>> = vec![vec![]; n];
        for ((i, _), v) in owners.into_iter().zip(vecs) {
            per_doc[i].push(v);
        }
        let saved: Vec<(usize, Vec<vectors::Quantized>)> = self.with_db(|c| {
            let tx = Tx::begin(c)?;
            let mut saved = vec![];
            for (i, vs) in per_doc.into_iter().enumerate() {
                let qs: Vec<_> = vs.iter().map(|v| vectors::quantize(v)).collect();
                if save_vectors(&tx, &model.key, &docs[i], &qs)? {
                    saved.push((i, qs));
                }
            }
            tx.commit()?;
            Ok(saved)
        })?;
        if let Some(f) = self.flat.write().unwrap_or_else(|e| e.into_inner()).as_mut().filter(|f| f.model() == model.key) {
            f.remove(&saved.iter().map(|(i, _)| docs[*i].id).collect());
            for (i, qs) in &saved {
                for (o, q) in qs.iter().enumerate() {
                    f.add(Entry { doc: docs[*i].id, ordinal: o as u16, source: docs[*i].source as u8 }, q);
                }
            }
        }
        Ok(saved.len())
    }

    /// Loads `model`'s vectors into memory once (later fills and syncs keep it current).
    pub async fn ensure_loaded(app: &AppHandle, model: &str) -> Result<(), String> {
        let st = app.state::<AppState>();
        if st.search.flat.read().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|f| f.model() == model) {
            return Ok(());
        }
        let (app2, model) = (app.clone(), model.to_string());
        tauri::async_runtime::spawn_blocking(move || {
            let s = &app2.state::<AppState>().search;
            let flat = s.with_db(|c| load(c, &model))?;
            *s.flat.write().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(flat));
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub fn with_vectors<T>(&self, f: impl FnOnce(Option<&dyn VectorIndex>) -> T) -> T {
        let guard = self.flat.read().unwrap_or_else(|e| e.into_inner());
        f(guard.as_deref())
    }

    fn count(&self, model: &str) -> Result<(i64, i64), String> {
        self.with_db(|c| {
            let records = c.query_row("SELECT count(*) FROM docs", [], |r| r.get(0)).map_err(err)?;
            let done = c
                .query_row("SELECT count(*) FROM vectors v JOIN docs d ON d.id = v.doc_id WHERE v.model = ?1 AND v.ordinal = 0 AND v.hash = d.vhash", [model], |r| r.get(0))
                .map_err(err)?;
            Ok((records, done))
        })
    }
}

/// Writes one source's rows: returns the docs whose vectors went stale or were removed.
pub fn apply(c: &Connection, source: usize, rows: Vec<Row>) -> Result<Vec<i64>, String> {
    let s = &SOURCES[source];
    let mut existing: HashMap<i64, (i64, i64, i64)> = HashMap::new();
    {
        // a dynamic source holds every made module's docs of its kind
        let sql = if s.dynamic() { "SELECT ref_id, id, hash, vhash FROM docs WHERE substr(module, 1, length(?1)) = ?1 AND kind = ?2" } else { "SELECT ref_id, id, hash, vhash FROM docs WHERE module = ?1 AND kind = ?2" };
        let mut st = c.prepare(sql).map_err(err)?;
        let found = st.query_map([s.module, s.kind], |r| Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))).map_err(err)?;
        for f in found {
            let (k, v) = f.map_err(err)?;
            existing.insert(k, v);
        }
    }
    let mut stale = vec![];
    let tx = Tx::begin(c)?;
    let mut seen = HashSet::new();
    for row in rows {
        seen.insert(row.ref_id);
        let (hash, vhash) = (row.hash(), sources::vector_hash(s, &row.body));
        match existing.get(&row.ref_id) {
            Some((_, h, _)) if *h == hash => {}
            Some((id, _, vh)) => {
                tx.execute(
                    "UPDATE docs SET title = ?2, detail = ?3, parent = ?4, day = ?5, at = ?6, body = ?7, hash = ?8, vhash = ?9 WHERE id = ?1",
                    params![id, row.title, row.detail, row.parent, row.day, row.at, row.body, hash, vhash],
                )
                .map_err(err)?;
                if *vh != vhash {
                    tx.execute("DELETE FROM docs_fts WHERE rowid = ?1", [id]).map_err(err)?;
                    tx.execute("INSERT INTO docs_fts (rowid, tokens) VALUES (?1, ?2)", params![id, text::tokens(&row.body)]).map_err(err)?;
                    tx.execute("DELETE FROM vectors WHERE doc_id = ?1", [id]).map_err(err)?;
                    stale.push(*id);
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO docs (module, kind, ref_id, title, detail, parent, day, at, body, hash, vhash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![row.module.as_deref().unwrap_or(s.module), s.kind, row.ref_id, row.title, row.detail, row.parent, row.day, row.at, row.body, hash, vhash],
                )
                .map_err(err)?;
                let id = tx.last_insert_rowid();
                tx.execute("INSERT INTO docs_fts (rowid, tokens) VALUES (?1, ?2)", params![id, text::tokens(&row.body)]).map_err(err)?;
            }
        }
    }
    for (ref_id, (id, _, _)) in &existing {
        if !seen.contains(ref_id) {
            tx.execute("DELETE FROM docs WHERE id = ?1", [id]).map_err(err)?;
            tx.execute("DELETE FROM docs_fts WHERE rowid = ?1", [id]).map_err(err)?;
            stale.push(*id);
        }
    }
    tx.commit()?;
    Ok(stale)
}

/// Docs without current vectors for `model`, newest first.
pub fn pending(c: &Connection, model: &str, limit: usize) -> Result<Vec<Pending>, String> {
    let mut st = c
        .prepare(
            "SELECT d.id, d.module, d.kind, d.title, d.body, d.vhash FROM docs d
             WHERE NOT EXISTS (SELECT 1 FROM vectors v WHERE v.model = ?1 AND v.doc_id = d.id AND v.ordinal = 0 AND v.hash = d.vhash)
             ORDER BY d.at DESC, d.id DESC LIMIT ?2",
        )
        .map_err(err)?;
    let rows = st
        .query_map(params![model, limit as i64], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .map_err(err)?;
    let mut out = vec![];
    for row in rows {
        let (id, module, kind, title, body, vhash) = row.map_err(err)?;
        if let Some(source) = source_index(&module, &kind) {
            out.push(Pending { id, source, title, body, vhash });
        }
    }
    Ok(out)
}

/// Replaces a doc's vectors unless it changed meanwhile (then it stays pending). Returns whether saved.
pub fn save_vectors(c: &Connection, model: &str, d: &Pending, qs: &[vectors::Quantized]) -> Result<bool, String> {
    let current: Option<i64> = c.query_row("SELECT vhash FROM docs WHERE id = ?1", [d.id], |r| r.get(0)).optional().map_err(err)?;
    if current != Some(d.vhash) {
        return Ok(false);
    }
    c.execute("DELETE FROM vectors WHERE model = ?1 AND doc_id = ?2", params![model, d.id]).map_err(err)?;
    for (o, q) in qs.iter().enumerate() {
        c.execute("INSERT INTO vectors (doc_id, model, ordinal, hash, vector) VALUES (?1, ?2, ?3, ?4, ?5)", params![d.id, model, o as i64, d.vhash, vectors::to_blob(q)]).map_err(err)?;
    }
    Ok(true)
}

/// All current vectors of `model`.
pub fn load(c: &Connection, model: &str) -> Result<Flat, String> {
    let mut flat = Flat::new(model);
    let mut st = c
        .prepare("SELECT v.doc_id, v.ordinal, d.module, d.kind, v.vector FROM vectors v JOIN docs d ON d.id = v.doc_id WHERE v.model = ?1 AND v.hash = d.vhash")
        .map_err(err)?;
    let mut rows = st.query([model]).map_err(err)?;
    while let Some(r) = rows.next().map_err(err)? {
        let (module, kind): (String, String) = (r.get(2).map_err(err)?, r.get(3).map_err(err)?);
        let Some(source) = source_index(&module, &kind) else { continue };
        let blob: Vec<u8> = r.get(4).map_err(err)?;
        if let Some(q) = vectors::from_blob(&blob) {
            flat.add(Entry { doc: r.get(0).map_err(err)?, ordinal: r.get::<_, i64>(1).map_err(err)? as u16, source: source as u8 }, &q);
        }
    }
    Ok(flat)
}

/// One round of the background work.
async fn tick(app: &AppHandle, first: bool) -> bool {
    let st = app.state::<AppState>();
    let s = &st.search;
    s.reopen_if_missing();
    let Some(model) = st.picky.runtime.service_model(Svc::Embedding) else {
        s.set_status(|x| x.model = None);
        return false;
    };
    let started = match s.check_model(&model.key) {
        Ok(started) => started,
        Err(e) => {
            s.set_status(|x| x.error = Some(e));
            return false;
        }
    };
    let app2 = app.clone();
    let synced = tauri::async_runtime::spawn_blocking(move || app2.state::<AppState>().search.sync_changed()).await.map_err(|e| e.to_string()).and_then(|r| r);
    match &synced {
        Err(e) => s.set_status(|x| x.error = Some(format!("搜索索引没能更新：{e}"))),
        Ok(_) if s.status().error.as_deref().is_some_and(|e| e.starts_with("搜索索引没能更新")) => s.set_status(|x| x.error = None),
        Ok(_) => {}
    }
    if first || started {
        crate::picky::index::requeue_missing(app);
    }
    if first || started || synced == Ok(true) {
        let _ = crate::picky::index::prune_with(app);
        let (records, done) = s.count(&model.key).unwrap_or_default();
        s.set_status(|x| (x.records, x.done) = (records, done));
    }
    let rebuilding = s.rebuilding();
    s.set_status(|x| (x.rebuilding, x.model) = (rebuilding, Some(model.name.clone())));
    let status = s.status();
    let waiting = s.retry_at.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| std::time::Instant::now() < t);
    let mut more = false;
    if status.done < status.records && !waiting {
        let running = st.picky.runtime.service_status(Svc::Embedding).state == ServiceState::Running;
        // without a rebuild, vectors wait until a search needs them or the service runs anyway
        let ready = if running || !rebuilding { Ok(running) } else { st.picky.runtime.ensure(Svc::Embedding).await.map(|_| true) };
        let outcome = match ready {
            Ok(true) => s.fill(app, &model).await,
            Ok(false) => Ok(0),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(n) => {
                more = n > 0;
                if more {
                    s.set_status(|x| (x.done, x.error) = ((x.done + n as i64).min(x.records), None));
                }
            }
            Err(e) => {
                *s.retry_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now() + Duration::from_secs(60));
                s.set_status(|x| x.error = Some(e));
            }
        }
    } else if rebuilding && status.done >= status.records && synced.is_ok() {
        let _ = s.finish_rebuild(&model.key);
        s.set_status(|x| x.rebuilding = false);
    }
    more
}

/// The indexer: a tick every 2 s, back to back while a fill has more to do.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut first = true;
        loop {
            let more = tick(&app, first).await;
            first = false;
            tokio::time::sleep(if more { Duration::from_millis(100) } else { TICK }).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::store::tests::memory_db;

    fn rows(c: &Connection, kind: &str) -> Vec<(i64, String)> {
        let mut st = c.prepare("SELECT ref_id, title FROM docs WHERE kind = ?1 ORDER BY ref_id").unwrap();
        st.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    }

    fn fts(c: &Connection, q: &str) -> Vec<i64> {
        let mut st = c.prepare("SELECT d.ref_id FROM docs_fts JOIN docs d ON d.id = docs_fts.rowid WHERE docs_fts MATCH ?1 ORDER BY d.ref_id").unwrap();
        st.query_map([text::fts_query(q).unwrap()], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn sync_adds_changes_and_removes_and_only_stale_vectors_go() {
        let s = memory_db();
        let p = crate::db::tests::memory_db();
        p.execute_batch("INSERT INTO books (title, author) VALUES ('小王子', '圣埃克苏佩里'), ('百年孤独', '马尔克斯');").unwrap();
        let (bi, book) = sources::source("books", "book").unwrap();
        assert!(apply(&s, bi, sources::read(&p, book).unwrap()).unwrap().is_empty());
        assert_eq!(rows(&s, "book").len(), 2);
        assert_eq!(fts(&s, "孤独"), [2]);

        // vectors for both, then one book changes its author (vector text) and the other is gone
        for d in pending(&s, "m", 10).unwrap() {
            assert!(save_vectors(&s, "m", &d, &[vectors::quantize(&[1.0, 0.0])]).unwrap());
        }
        assert!(pending(&s, "m", 10).unwrap().is_empty());
        assert_eq!(pending(&s, "other", 10).unwrap().len(), 2); // per model
        p.execute_batch("UPDATE books SET author = '加西亚·马尔克斯' WHERE id = 2; DELETE FROM books WHERE id = 1;").unwrap();
        let stale = apply(&s, bi, sources::read(&p, book).unwrap()).unwrap();
        assert_eq!(stale.len(), 2);
        assert_eq!(rows(&s, "book"), [(2, "百年孤独".into())]);
        assert_eq!(fts(&s, "加西亚"), [2]);
        assert_eq!(pending(&s, "m", 10).unwrap().len(), 1);
        assert_eq!(s.query_row("SELECT count(*) FROM vectors", [], |r| r.get::<_, i64>(0)).unwrap(), 0);

        // a doc that changed while its vectors were made stays pending
        let d = pending(&s, "m", 10).unwrap().remove(0);
        p.execute("UPDATE books SET author = '某人' WHERE id = 2", []).unwrap();
        apply(&s, bi, sources::read(&p, book).unwrap()).unwrap();
        assert!(!save_vectors(&s, "m", &d, &[vectors::quantize(&[1.0, 0.0])]).unwrap());

        // nothing changed: nothing written
        assert!(apply(&s, bi, sources::read(&p, book).unwrap()).unwrap().is_empty());
        let d = pending(&s, "m", 10).unwrap().remove(0);
        save_vectors(&s, "m", &d, &[vectors::quantize(&[1.0, 0.0]), vectors::quantize(&[0.0, 1.0])]).unwrap();
        let flat = load(&s, "m").unwrap();
        assert_eq!(flat.len(), 2);
    }
}
