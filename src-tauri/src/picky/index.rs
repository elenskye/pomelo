//! Project file indexes and retrieval (from Picky's ProjectFileIndex):
//! - 建立索引 on a file splits its current version into passages of about 240 estimated tokens
//!   (sentences and lines packed; a short last sentence is repeated at the start of the next),
//!   embeds "资料名 · 第 3 页\n<passage>" in batches of 16 and saves everything at once when done
//!   (a passage the service refuses as too long is halved). At most 5000 passages.
//! - One job at a time, in order; a job can be stopped (nothing saved) and retried. Replacing an
//!   indexed file rebuilds it. Only current versions of indexed files keep an index (`prune`).
//! - A question in a project (with nothing else to wait for) retrieves at most 6 passages scoring
//!   ≥ 0.35 from files whose current version has a complete index for the current embedding model;
//!   files sent in full with the message are skipped. If 8081 can't be used, the page asks: send
//!   without, retry or cancel. Passages are kept with the run (`run_passages`); replies don't cite.

use super::embed::{self, EmbedError, Input};
use super::files::{self, Segment};
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

const TARGET: i64 = 240;
const MAXIMUM: i64 = 320;
pub const MAX_CHUNKS: usize = 5000;
pub const LIMIT: usize = 6;
const MIN_SCORE: f32 = 0.35;

// ---------- chunking ----------

fn est(s: &str) -> i64 {
    super::context::estimate(s)
}

/// Sentences: ends after a line break, 。！？；, or .!?; followed by whitespace.
fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut cur = String::new();
    for (i, c) in chars.iter().enumerate() {
        cur.push(*c);
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        if *c == '\n' || "。！？；".contains(*c) || (".!?;".contains(*c) && next.is_whitespace()) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Windows of about TARGET tokens, ending at a space when one is near.
fn hard_split(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut start = 0;
    while start < chars.len() {
        let (mut end, mut wide, mut other) = (start, 0i64, 0i64);
        while end < chars.len() {
            let w = super::context::is_wide(chars[end]);
            let tokens = wide + i64::from(w) + (other + i64::from(!w) + 2) / 3;
            if tokens > TARGET && end > start {
                break;
            }
            if w {
                wide += 1;
            } else {
                other += 1;
            }
            end += 1;
        }
        if end < chars.len() {
            if let Some(sp) = chars[start..end].iter().rposition(|c| c.is_whitespace()) {
                if sp > (end - start) / 2 {
                    end = start + sp + 1;
                }
            }
        }
        out.push(chars[start..end].iter().collect());
        start = end;
    }
    out
}

pub fn split(text: &str) -> Vec<String> {
    let mut units = vec![];
    for u in sentences(text) {
        if est(&u) <= MAXIMUM {
            units.push(u);
        } else {
            units.extend(hard_split(&u));
        }
    }
    let mut out: Vec<String> = vec![];
    let (mut cur, mut tokens): (Vec<String>, i64) = (vec![], 0);
    for u in units {
        let size = est(&u);
        if !cur.is_empty() && tokens + size > TARGET {
            out.push(cur.concat());
            let last = cur.last().cloned().unwrap_or_default();
            let ls = est(&last);
            let carry = cur.len() > 1 && ls <= TARGET / 4 && ls + size <= MAXIMUM;
            cur = if carry { vec![last] } else { vec![] };
            tokens = if carry { ls } else { 0 };
        }
        tokens += size;
        cur.push(u);
    }
    if !cur.is_empty() {
        out.push(cur.concat());
    }
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Splits near the middle at a line, space or punctuation; under 40 characters stays whole.
fn halve(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 40 {
        return vec![text.to_string()];
    }
    let mid = chars.len() / 2;
    let span = (chars.len() / 4).min(200);
    let cut = chars[mid - span..mid + span].iter().rposition(|c| c.is_whitespace() || "。！？；，,".contains(*c)).map_or(mid, |i| mid - span + i + 1);
    [chars[..cut].iter().collect::<String>(), chars[cut..].iter().collect()].into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub segment: i64,
    pub content: String,
}

pub fn pieces(segments: &[Segment]) -> Vec<Piece> {
    segments.iter().flat_map(|s| split(&s.content).into_iter().map(move |c| Piece { segment: s.ordinal, content: c })).collect()
}

// ---------- storage ----------

#[derive(Debug, Clone)]
pub struct Chunk {
    pub segment: i64,
    pub content: String,
    pub vector: Vec<f32>,
}

/// Keeps only the indexes of current versions of indexed project files.
pub fn prune(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM artifact_indexes WHERE artifact_id NOT IN (
           SELECT v.artifact_id FROM project_files f JOIN project_file_versions v ON v.file_id = f.id AND v.version = f.current_version WHERE f.indexed = 1)",
        [],
    )
    .map(|_| ())
    .map_err(err)
}

pub fn save(conn: &Connection, artifact: i64, model: &str, chunks: &[Chunk]) -> Result<(), String> {
    let tx = Tx::begin(conn)?;
    tx.execute("DELETE FROM artifact_indexes WHERE artifact_id = ?1 AND model = ?2", params![artifact, model]).map_err(err)?;
    tx.execute("INSERT INTO artifact_indexes (artifact_id, model, chunk_count) VALUES (?1, ?2, ?3)", params![artifact, model, chunks.len() as i64]).map_err(err)?;
    let mut st = tx.prepare("INSERT INTO artifact_chunks (artifact_id, model, ordinal, segment_ordinal, content, vector) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").map_err(err)?;
    for (i, c) in chunks.iter().enumerate() {
        let bytes: Vec<u8> = c.vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        st.execute(params![artifact, model, i as i64, c.segment, c.content, bytes]).map_err(err)?;
    }
    drop(st);
    prune(&tx)?; // turned off or replaced meanwhile: keep nothing
    tx.commit()?;
    if let Some(m) = LOADED.lock().ok().as_mut().and_then(|g| g.as_mut()) {
        m.remove(&(artifact, model.to_string()));
    }
    Ok(())
}

pub fn chunks(conn: &Connection, artifact: i64, model: &str) -> Result<Vec<Chunk>, String> {
    let mut st = conn.prepare("SELECT segment_ordinal, content, vector FROM artifact_chunks WHERE artifact_id = ?1 AND model = ?2 ORDER BY ordinal").map_err(err)?;
    let rows = st
        .query_map(params![artifact, model], |r| {
            let b: Vec<u8> = r.get(2)?;
            Ok(Chunk { segment: r.get(0)?, content: r.get(1)?, vector: b.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect() })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

pub fn indexed(conn: &Connection, model: &str) -> Result<Vec<i64>, String> {
    let mut st = conn.prepare("SELECT artifact_id FROM artifact_indexes WHERE model = ?1").map_err(err)?;
    let rows = st.query_map([model], |r| r.get(0)).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

// ---------- jobs ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum IndexState {
    /// 建立索引 not chosen.
    None,
    Queued,
    Building { done: usize, total: usize },
    Ready,
    Failed { message: String },
    /// Chosen, but the current version has no complete index (stopped, or replaced while 8081 was
    /// down): 重新建立索引 finishes it.
    Incomplete,
}

#[derive(Default)]
struct Inner {
    queue: VecDeque<i64>,
    running: Option<(i64, u64, Option<tauri::async_runtime::JoinHandle<()>>)>,
    states: HashMap<i64, IndexState>,
    token: u64,
}

#[derive(Default)]
pub struct Jobs {
    inner: Mutex<Inner>,
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn state(&self, file: i64) -> Option<IndexState> {
        self.lock().states.get(&file).cloned()
    }
    /// Stops (or unqueues) a file's job; nothing is saved.
    pub fn cancel(&self, app: &AppHandle, file: i64) {
        {
            let mut g = self.lock();
            g.queue.retain(|f| *f != file);
            if g.running.as_ref().is_some_and(|r| r.0 == file) {
                if let Some((_, _, Some(task))) = g.running.take() {
                    task.abort();
                }
            }
            if matches!(g.states.get(&file), Some(IndexState::Queued | IndexState::Building { .. })) {
                g.states.remove(&file);
            }
        }
        start_next(app);
    }
}

pub fn enqueue(app: &AppHandle, file: i64) {
    {
        let jobs = &app.state::<AppState>().picky.index_jobs;
        let mut g = jobs.lock();
        if g.running.as_ref().is_some_and(|r| r.0 == file) || g.queue.contains(&file) {
            return;
        }
        g.queue.push_back(file);
        g.states.insert(file, IndexState::Queued);
    }
    start_next(app);
}

fn start_next(app: &AppHandle) {
    let st = app.state::<AppState>();
    let jobs = &st.picky.index_jobs;
    loop {
        let (file, token) = {
            let mut g = jobs.lock();
            if g.running.is_some() {
                return;
            }
            let Some(file) = g.queue.pop_front() else { return };
            g.token += 1;
            let token = g.token;
            g.states.insert(file, IndexState::Building { done: 0, total: 0 });
            g.running = Some((file, token, None));
            (file, token)
        };
        let wanted = st.picky.with_db(|c| files::project_file(c, file)).is_ok_and(|f| f.indexed);
        if !wanted {
            let mut g = jobs.lock();
            g.states.remove(&file);
            g.running = None;
            continue;
        }
        let app2 = app.clone();
        let task = tauri::async_runtime::spawn(async move {
            let outcome = build(&app2, file, token).await;
            let jobs = &app2.state::<AppState>().picky.index_jobs;
            {
                let mut g = jobs.lock();
                if g.running.as_ref().is_none_or(|r| r.1 != token) {
                    return;
                }
                match outcome {
                    Ok(()) => g.states.remove(&file),
                    Err(message) => g.states.insert(file, IndexState::Failed { message }),
                };
                g.running = None;
            }
            start_next(&app2);
        });
        let mut g = jobs.lock();
        match g.running.as_mut() {
            Some(r) if r.1 == token => r.2 = Some(task),
            _ => task.abort(),
        }
        return;
    }
}

fn progress(app: &AppHandle, file: i64, token: u64, done: usize, total: usize) {
    let st = app.state::<AppState>();
    let mut g = st.picky.index_jobs.lock();
    if g.running.as_ref().is_some_and(|r| r.1 == token) {
        g.states.insert(file, IndexState::Building { done, total });
    }
}

async fn build(app: &AppHandle, file: i64, token: u64) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mode = st.picky.mode;
    let model = embed::model(mode);
    let (artifact, name, segs) = st.picky.with_db(|c| {
        let f = files::project_file(c, file)?;
        let cur = f.current().filter(|x| !x.deleted).ok_or("资料已删除或不能读取")?.clone();
        Ok((cur.id, cur.name.clone(), files::segments(c, cur.id)?))
    })?;
    let labels: HashMap<i64, String> = segs.iter().map(|s| (s.ordinal, format!("{name} · {}", s.label()))).collect();
    let mut list = tauri::async_runtime::spawn_blocking(move || pieces(&segs)).await.map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Err("资料没有可以建立索引的文字".into());
    }
    let too_long = || format!("资料太长（超过 {MAX_CHUNKS} 段），暂时没法建立索引");
    if list.len() > MAX_CHUNKS {
        return Err(too_long());
    }
    progress(app, file, token, 0, list.len());
    embed::prepare(app).await?;
    let mut made: Vec<Chunk> = vec![];
    let mut start = 0;
    while start < list.len() {
        let batch: Vec<Piece> = list[start..(start + 16).min(list.len())].to_vec();
        let texts: Vec<String> = batch.iter().map(|p| format!("{}\n{}", labels.get(&p.segment).cloned().unwrap_or_else(|| name.clone()), p.content)).collect();
        match embed::embed(mode, &texts, Input::Document).await {
            Ok(vectors) => {
                made.extend(batch.into_iter().zip(vectors).map(|(p, v)| Chunk { segment: p.segment, content: p.content, vector: v }));
                start += texts.len();
            }
            Err(EmbedError::TooLong) => {
                let halves: Vec<Piece> = batch.iter().flat_map(|p| halve(&p.content).into_iter().map(move |c| Piece { segment: p.segment, content: c })).collect();
                if halves.len() <= batch.len() {
                    return Err("资料中有一段文字没法建立索引。".into());
                }
                let n = batch.len();
                list.splice(start..start + n, halves);
                if list.len() > MAX_CHUNKS {
                    return Err(too_long());
                }
            }
            Err(e) => return Err(e.into()),
        }
        progress(app, file, token, made.len(), list.len());
    }
    st.picky.with_db(|c| save(c, artifact, model, &made))
}

// ---------- retrieval ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Passage {
    pub file_id: i64,
    pub artifact_id: i64,
    pub segment_ordinal: i64,
    /// 资料名 · 第 3 页
    pub label: String,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Indexed files a question searches: (file, current artifact, name).
    #[serde(skip)]
    pub ready: Vec<(i64, i64, String)>,
    pub ready_names: Vec<String>,
    /// Indexed files still without a complete index.
    pub pending: usize,
}

/// `exclude`: project files sent in full with the message.
pub fn plan(conn: &Connection, project: Option<i64>, model: &str, exclude: &[i64]) -> Result<Plan, String> {
    let Some(project) = project else { return Ok(Plan::default()) };
    let done = indexed(conn, model)?;
    let mut p = Plan::default();
    for f in files::project_files(conn, Some(project))? {
        if !f.indexed || !f.usable() || exclude.contains(&f.id) {
            continue;
        }
        let cur = f.current().map(|c| c.id).unwrap_or_default();
        if done.contains(&cur) {
            p.ready_names.push(f.name());
            p.ready.push((f.id, cur, f.name()));
        } else {
            p.pending += 1;
        }
    }
    Ok(p)
}

type Loaded = Arc<(Vec<Chunk>, HashMap<i64, String>)>;
static LOADED: Mutex<Option<HashMap<(i64, String), Loaded>>> = Mutex::new(None);

fn loaded(conn: &Connection, artifact: i64, name: &str, model: &str) -> Result<Loaded, String> {
    let key = (artifact, model.to_string());
    if let Some(hit) = LOADED.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&key).cloned())) {
        return Ok(hit);
    }
    let labels = files::segments(conn, artifact)?.iter().map(|s| (s.ordinal, format!("{name} · {}", s.label()))).collect();
    let l: Loaded = Arc::new((chunks(conn, artifact, model)?, labels));
    if let Ok(mut g) = LOADED.lock() {
        g.get_or_insert_with(HashMap::new).insert(key, l.clone());
    }
    Ok(l)
}

/// The passages most related to `question` among `query`-ranked chunks (pure; for tests).
pub fn rank(query: &[f32], sources: &[(i64, i64, String, Loaded)]) -> Vec<Passage> {
    let mut scored: Vec<(f32, usize, Passage)> = vec![];
    for (file, artifact, name, l) in sources {
        for c in &l.0 {
            let s = embed::cosine(query, &c.vector);
            if s < MIN_SCORE {
                continue;
            }
            let n = scored.len();
            scored.push((s, n, Passage { file_id: *file, artifact_id: *artifact, segment_ordinal: c.segment, label: l.1.get(&c.segment).cloned().unwrap_or_else(|| name.clone()), content: c.content.clone() }));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored.into_iter().take(LIMIT).map(|x| x.2).collect()
}

pub async fn retrieve(app: &AppHandle, question: &str, plan: &Plan) -> Result<Vec<Passage>, String> {
    if plan.ready.is_empty() {
        return Ok(vec![]);
    }
    let st = app.state::<AppState>();
    let mode = st.picky.mode;
    embed::prepare(app).await?;
    let q = embed::embed(mode, &[question.to_string()], Input::PassageQuery).await.map_err(String::from)?.pop().ok_or("向量服务没有返回问题的向量。")?;
    let model = embed::model(mode);
    let sources = st.picky.with_db(|c| plan.ready.iter().map(|(f, a, n)| Ok((*f, *a, n.clone(), loaded(c, *a, n, model)?))).collect::<Result<Vec<_>, String>>())?;
    Ok(rank(&q, &sources))
}

// ---------- commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    #[serde(flatten)]
    pub file: files::ProjectFile,
    pub index: IndexState,
}

fn views(app: &AppHandle, project: i64) -> Result<Vec<FileView>, String> {
    let st = app.state::<AppState>();
    let model = embed::model(st.picky.mode);
    let (list, done) = st.picky.with_db(|c| Ok((files::project_files(c, Some(project))?, indexed(c, model)?)))?;
    Ok(list
        .into_iter()
        .map(|f| {
            let index = if !f.indexed {
                IndexState::None
            } else if let Some(s) = st.picky.index_jobs.state(f.id) {
                s
            } else if f.current().is_some_and(|c| !c.deleted && done.contains(&c.id)) {
                IndexState::Ready
            } else {
                IndexState::Incomplete
            };
            FileView { file: f, index }
        })
        .collect())
}

#[tauri::command]
pub fn picky_project_files(app: AppHandle, project_id: i64) -> Result<Vec<FileView>, String> {
    views(&app, project_id)
}

#[tauri::command]
pub fn picky_project_file_add(state: State<AppState>, project_id: i64, artifact_id: i64) -> Result<i64, String> {
    state.with_picky_undo("添加资料", |c| files::add_project_file(c, project_id, artifact_id))
}

#[tauri::command]
pub fn picky_project_file_replace(app: AppHandle, file_id: i64, artifact_id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    let indexed = st.with_picky_undo("替换资料", |c| files::replace_project_file(c, file_id, artifact_id))?;
    st.picky.index_jobs.cancel(&app, file_id);
    if indexed {
        enqueue(&app, file_id);
    }
    Ok(())
}

#[tauri::command]
pub fn picky_project_file_remove(app: AppHandle, file_id: i64) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.with_picky_undo("移除资料", |c| files::remove_project_file(c, file_id))?;
    st.picky.index_jobs.cancel(&app, file_id);
    Ok(())
}

/// 建立索引 (starts building, which may start 8081) / 取消索引.
#[tauri::command]
pub fn picky_project_file_index(app: AppHandle, file_id: i64, indexed: bool) -> Result<(), String> {
    let st = app.state::<AppState>();
    st.with_picky_undo(if indexed { "建立索引" } else { "取消索引" }, |c| files::set_indexed(c, file_id, indexed))?;
    if indexed {
        enqueue(&app, file_id);
    } else {
        st.picky.index_jobs.cancel(&app, file_id);
    }
    Ok(())
}

#[tauri::command]
pub fn picky_index_rebuild(app: AppHandle, file_id: i64) -> Result<(), String> {
    app.state::<AppState>().picky.index_jobs.cancel(&app, file_id);
    enqueue(&app, file_id);
    Ok(())
}

#[tauri::command]
pub fn picky_index_stop(app: AppHandle, file_id: i64) -> Result<(), String> {
    app.state::<AppState>().picky.index_jobs.cancel(&app, file_id);
    Ok(())
}

fn project_of(conn: &Connection, session: Option<i64>, project: Option<i64>) -> Result<Option<i64>, String> {
    match session {
        Some(s) => conn.query_row("SELECT project_id FROM sessions WHERE id = ?1", [s], |r| r.get(0)).map_err(err),
        None => Ok(project),
    }
}

/// What a question in a conversation (or a draft in `project_id`) would search.
#[tauri::command]
pub fn picky_retrieval_plan(state: State<AppState>, session_id: Option<i64>, project_id: Option<i64>, exclude_files: Vec<i64>) -> Result<Plan, String> {
    let model = embed::model(state.picky.mode);
    state.picky.with_db(|c| plan(c, project_of(c, session_id, project_id)?, model, &exclude_files))
}

/// Passages for a question (empty when no indexed file is ready); an error means 8081 couldn't be
/// used and the page asks what to do.
#[tauri::command]
pub async fn picky_retrieve(app: AppHandle, session_id: Option<i64>, project_id: Option<i64>, question: String, exclude_files: Vec<i64>) -> Result<Vec<Passage>, String> {
    let st = app.state::<AppState>();
    let model = embed::model(st.picky.mode);
    let p = st.picky.with_db(|c| plan(c, project_of(c, session_id, project_id)?, model, &exclude_files))?;
    retrieve(&app, question.trim(), &p).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::files::tests::add;
    use crate::picky::library;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn splitting_packs_sentences_and_carries_a_short_last_one() {
        assert!(split("").is_empty());
        let text = "第一句。".repeat(100);
        let parts = split(&text);
        assert!(parts.len() > 1 && parts.iter().all(|p| est(p) <= MAXIMUM), "{parts:?}");
        // the short last sentence of a passage starts the next one too
        assert!(parts[1].starts_with("第一句。"));
        let long_word = "a".repeat(3000);
        assert!(split(&long_word).iter().all(|p| est(p) <= MAXIMUM));
        assert_eq!(halve("短"), ["短"]);
        let h = halve(&"一二三四五，".repeat(20));
        assert_eq!(h.len(), 2);
        assert_eq!(h.concat(), "一二三四五，".repeat(20));
    }

    #[test]
    fn indexes_follow_current_versions_and_retrieval_ranks() {
        let c = memory_db();
        let dir = crate::db::tests::temp_dir("picky-index");
        let p = library::create_project(&c, "P", "").unwrap();
        let a = add(&c, &dir, "向量.md", "# 选择\n向量数据库的选择标准。\n\n# 其他\n周末去公园散步。".as_bytes());
        let f = files::add_project_file(&c, p, a).unwrap();
        files::set_indexed(&c, f, true).unwrap();
        let segs = files::segments(&c, a).unwrap();
        let made: Vec<Chunk> = pieces(&segs).into_iter().map(|x| Chunk { vector: embed::mock_vector(&x.content), segment: x.segment, content: x.content }).collect();
        save(&c, a, embed::MOCK_MODEL, &made).unwrap();
        let pl = plan(&c, Some(p), embed::MOCK_MODEL, &[]).unwrap();
        assert_eq!((pl.ready.len(), pl.pending), (1, 0));
        assert!(plan(&c, Some(p), embed::MOCK_MODEL, &[f]).unwrap().ready.is_empty()); // sent in full
        assert!(plan(&c, Some(p), embed::REAL_MODEL, &[]).unwrap().ready.is_empty()); // another model's index doesn't count

        let l: Loaded = Arc::new((chunks(&c, a, embed::MOCK_MODEL).unwrap(), files::segments(&c, a).unwrap().iter().map(|s| (s.ordinal, format!("向量.md · {}", s.label()))).collect()));
        let hits = rank(&embed::mock_vector("向量数据库怎么选择"), &[(f, a, "向量.md".into(), l)]);
        assert_eq!(hits[0].label, "向量.md · 选择");
        assert!(hits[0].content.contains("向量数据库"));

        // a new version leaves the old index out; turning it off removes it
        let b = add(&c, &dir, "向量.md", b"new text");
        files::replace_project_file(&c, f, b).unwrap();
        assert!(indexed(&c, embed::MOCK_MODEL).unwrap().is_empty());
        let pl = plan(&c, Some(p), embed::MOCK_MODEL, &[]).unwrap();
        assert_eq!((pl.ready.len(), pl.pending), (0, 1));
    }
}
