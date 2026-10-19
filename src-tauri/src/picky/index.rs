//! Project file indexes and retrieval (from Picky's ProjectFileIndex):
//! - 建立索引 on a file splits its current version into passages of about 240 estimated tokens
//!   (sentences and lines packed; a short last sentence is repeated at the start of the next),
//!   embeds "资料名 · 第 3 页\n<passage>" in batches of 16 and saves everything at once when done
//!   (a passage the service refuses as too long is halved). At most 5000 passages.
//! - One job at a time, in order; a job can be stopped (nothing saved) and retried. Replacing an
//!   indexed file rebuilds it. Only current versions of indexed files keep an index (`prune`).
//! - The passages and vectors live in search.db (v0.4; derived, keyed by artifact + sha256 +
//!   model); picky.db only says which files are indexed. Files whose index is missing (search.db
//!   deleted, another embedding model) are queued again by the search indexer (`requeue_missing`).
//! - A question in a project (with nothing else to wait for) retrieves at most 6 passages scoring
//!   ≥ 0.35 from files whose current version has a complete index for the current embedding model;
//!   files sent in full with the message are skipped (v0.4: a pool of 20 ≥ 0.30 reordered by the
//!   rerank model, rerank.rs). If 8081 can't be used, the page asks: send
//!   without, retry or cancel. Passages are kept with the run (`run_passages`) and numbered for
//!   the reply's citations (cite.rs).
//! - v0.4.5 阶段 7: the same two commands also cover the conversation's 知识库 (knowledge.rs): its
//!   plan, and passages from its libraries (at most 6 more, each with its source, `Passage::kb`).

use super::embed::{self, EmbedError, Input};
use super::files::{self, Segment};
use crate::modules::common::{err, Tx};
use crate::AppState;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

const TARGET: i64 = 240;
const MAXIMUM: i64 = 320;
pub const MAX_CHUNKS: usize = 5000;
pub const LIMIT: usize = 6;
pub const MIN_SCORE: f32 = 0.35;
/// v0.4: the pool the rerank model reorders (wider than what is sent).
pub const POOL: usize = 20;
pub const POOL_MIN: f32 = 0.30;

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
pub fn halve(text: &str) -> Vec<String> {
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

/// picky.db: the current versions of indexed, readable project files: file → (artifact, sha256).
pub fn wanted(picky: &Connection, project: Option<i64>) -> Result<HashMap<i64, (i64, String)>, String> {
    let mut st = picky
        .prepare(
            "SELECT f.id, a.id, a.sha256 FROM project_files f
             JOIN project_file_versions v ON v.file_id = f.id AND v.version = f.current_version JOIN artifacts a ON a.id = v.artifact_id
             WHERE f.indexed = 1 AND a.deleted_at IS NULL AND (?1 IS NULL OR f.project_id = ?1)",
        )
        .map_err(err)?;
    let rows = st.query_map([project], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?)))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// search.db: keeps only the indexes of `keep` (artifact, sha256).
pub fn prune(search: &Connection, keep: &HashSet<(i64, String)>) -> Result<(), String> {
    let have: Vec<(i64, String)> = {
        let mut st = search.prepare("SELECT DISTINCT artifact_id, sha256 FROM file_indexes").map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)?
    };
    for (a, sha) in have.into_iter().filter(|x| !keep.contains(x)) {
        search.execute("DELETE FROM file_indexes WHERE artifact_id = ?1 AND sha256 = ?2", params![a, sha]).map_err(err)?;
        if let Some(m) = LOADED.lock().ok().as_mut().and_then(|g| g.as_mut()) {
            m.retain(|k, _| k.0 != a);
        }
    }
    Ok(())
}

/// search.db: the index of one artifact for one model, replacing any earlier one.
pub fn save(search: &Connection, artifact: i64, sha256: &str, model: &str, chunks: &[Chunk]) -> Result<(), String> {
    let tx = Tx::begin(search)?;
    tx.execute("DELETE FROM file_indexes WHERE artifact_id = ?1 AND model = ?2", params![artifact, model]).map_err(err)?;
    tx.execute("INSERT INTO file_indexes (artifact_id, model, sha256, chunk_count) VALUES (?1, ?2, ?3, ?4)", params![artifact, model, sha256, chunks.len() as i64]).map_err(err)?;
    let mut st = tx.prepare("INSERT INTO file_chunks (artifact_id, model, ordinal, segment_ordinal, content, vector) VALUES (?1, ?2, ?3, ?4, ?5, ?6)").map_err(err)?;
    for (i, c) in chunks.iter().enumerate() {
        let bytes: Vec<u8> = c.vector.iter().flat_map(|x| x.to_le_bytes()).collect();
        st.execute(params![artifact, model, i as i64, c.segment, c.content, bytes]).map_err(err)?;
    }
    drop(st);
    tx.commit()?;
    if let Some(m) = LOADED.lock().ok().as_mut().and_then(|g| g.as_mut()) {
        m.remove(&(artifact, model.to_string()));
    }
    Ok(())
}

pub fn chunks(search: &Connection, artifact: i64, model: &str) -> Result<Vec<Chunk>, String> {
    let mut st = search.prepare("SELECT segment_ordinal, content, vector FROM file_chunks WHERE artifact_id = ?1 AND model = ?2 ORDER BY ordinal").map_err(err)?;
    let rows = st
        .query_map(params![artifact, model], |r| {
            let b: Vec<u8> = r.get(2)?;
            Ok(Chunk { segment: r.get(0)?, content: r.get(1)?, vector: b.chunks_exact(4).map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]])).collect() })
        })
        .map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// search.db: (artifact, sha256) with a complete index for `model`.
pub fn indexed(search: &Connection, model: &str) -> Result<HashSet<(i64, String)>, String> {
    let mut st = search.prepare("SELECT artifact_id, sha256 FROM file_indexes WHERE model = ?1").map_err(err)?;
    let rows = st.query_map([model], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
    rows.collect::<Result<_, _>>().map_err(err)
}

/// Drops indexes of files no longer indexed (or replaced, removed, deleted).
pub fn prune_with(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    let keep: HashSet<(i64, String)> = st.picky.with_db(|c| wanted(c, None))?.into_values().collect();
    st.search.with_db(|c| prune(c, &keep))
}

/// Queues indexed files whose current version has no index for the current model.
pub fn requeue_missing(app: &AppHandle) {
    let st = app.state::<AppState>();
    let model = embed::model(&st.picky);
    let (Ok(done), Ok(cur)) = (st.search.with_db(|c| indexed(c, &model)), st.picky.with_db(|c| wanted(c, None))) else { return };
    for (file, key) in cur {
        if !done.contains(&key) && st.picky.index_jobs.state(file).is_none() {
            enqueue(app, file);
        }
    }
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
    let (artifact, sha, name, segs) = st.picky.with_db(|c| {
        let f = files::project_file(c, file)?;
        let cur = f.current().filter(|x| !x.deleted).ok_or("资料已删除或不能读取")?.clone();
        let sha: String = c.query_row("SELECT sha256 FROM artifacts WHERE id = ?1", [cur.id], |r| r.get(0)).map_err(err)?;
        Ok((cur.id, sha, cur.name.clone(), files::segments(c, cur.id)?))
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
    let model = embed::prepare(app).await?;
    let mut made: Vec<Chunk> = vec![];
    let mut start = 0;
    while start < list.len() {
        let batch: Vec<Piece> = list[start..(start + 16).min(list.len())].to_vec();
        let texts: Vec<String> = batch.iter().map(|p| format!("{}\n{}", labels.get(&p.segment).cloned().unwrap_or_else(|| name.clone()), p.content)).collect();
        match embed::embed(&st.picky.runtime, &model, &texts, Input::Document).await {
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
    st.search.with_db(|c| save(c, artifact, &sha, &model.key, &made))?;
    prune_with(app) // turned off or replaced meanwhile: keep nothing
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
    /// From a 知识库 (file_id 0; artifact_id: a web page's copy, else 0).
    #[serde(default)]
    pub kb: Option<super::knowledge::KbRef>,
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
    /// The conversation's 知识库 when its switch is on.
    pub knowledge: Option<super::knowledge::KbPlan>,
}

/// `done`: `indexed` for the current model; `exclude`: project files sent in full with the message.
pub fn plan(picky: &Connection, project: Option<i64>, done: &HashSet<(i64, String)>, exclude: &[i64]) -> Result<Plan, String> {
    let Some(project) = project else { return Ok(Plan::default()) };
    let cur = wanted(picky, Some(project))?;
    let mut p = Plan::default();
    for f in files::project_files(picky, Some(project))? {
        if !f.indexed || !f.usable() || exclude.contains(&f.id) {
            continue;
        }
        if let Some(key) = cur.get(&f.id).filter(|k| done.contains(*k)) {
            p.ready_names.push(f.name());
            p.ready.push((f.id, key.0, f.name()));
        } else {
            p.pending += 1;
        }
    }
    Ok(p)
}

type Loaded = Arc<(Vec<Chunk>, HashMap<i64, String>)>;
static LOADED: Mutex<Option<HashMap<(i64, String), Loaded>>> = Mutex::new(None);

fn loaded(st: &AppState, artifact: i64, name: &str, model: &str) -> Result<Loaded, String> {
    let key = (artifact, model.to_string());
    if let Some(hit) = LOADED.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&key).cloned())) {
        return Ok(hit);
    }
    let chunks = st.search.with_db(|c| chunks(c, artifact, model))?;
    let labels = st.picky.with_db(|c| Ok(files::segments(c, artifact)?.iter().map(|s| (s.ordinal, format!("{name} · {}", s.label()))).collect()))?;
    let l: Loaded = Arc::new((chunks, labels));
    if let Ok(mut g) = LOADED.lock() {
        g.get_or_insert_with(HashMap::new).insert(key, l.clone());
    }
    Ok(l)
}

/// The passages most related to `question` among `query`-ranked chunks (pure; for tests).
pub fn rank(query: &[f32], sources: &[(i64, i64, String, Loaded)]) -> Vec<Passage> {
    similar(query, sources, LIMIT, MIN_SCORE)
}

fn similar(query: &[f32], sources: &[(i64, i64, String, Loaded)], keep: usize, min: f32) -> Vec<Passage> {
    let mut scored: Vec<(f32, usize, Passage)> = vec![];
    for (file, artifact, name, l) in sources {
        for c in &l.0 {
            let s = embed::cosine(query, &c.vector);
            if s < min {
                continue;
            }
            let n = scored.len();
            scored.push((s, n, Passage { file_id: *file, artifact_id: *artifact, segment_ordinal: c.segment, label: l.1.get(&c.segment).cloned().unwrap_or_else(|| name.clone()), content: c.content.clone(), kb: None }));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored.into_iter().take(keep).map(|x| x.2).collect()
}

pub async fn retrieve(app: &AppHandle, question: &str, plan: &Plan) -> Result<Vec<Passage>, String> {
    if plan.ready.is_empty() {
        return Ok(vec![]);
    }
    let st = app.state::<AppState>();
    let model = embed::prepare(app).await?;
    let q = embed::embed(&st.picky.runtime, &model, &[question.to_string()], Input::PassageQuery).await.map_err(String::from)?.pop().ok_or("向量服务没有返回问题的向量。")?;
    let sources = plan.ready.iter().map(|(f, a, n)| Ok((*f, *a, n.clone(), loaded(&st, *a, n, &model.key)?))).collect::<Result<Vec<_>, String>>()?;
    // v0.4: the rerank model picks from a wider pool; without it, similarity alone
    let pool = similar(&q, &sources, POOL, POOL_MIN);
    Ok(super::rerank::pick(app, question, &pool, |p| format!("{}\n{}", p.label, p.content), LIMIT).await.unwrap_or_else(|| rank(&q, &sources)))
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
    let model = embed::model(&st.picky);
    let done = st.search.with_db(|c| indexed(c, &model)).unwrap_or_default();
    let (list, cur) = st.picky.with_db(|c| Ok((files::project_files(c, Some(project))?, wanted(c, Some(project))?)))?;
    Ok(list
        .into_iter()
        .map(|f| {
            let index = if !f.indexed {
                IndexState::None
            } else if let Some(s) = st.picky.index_jobs.state(f.id) {
                s
            } else if cur.get(&f.id).is_some_and(|k| done.contains(k)) {
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

/// The 知识库 a question searches (a draft: `knowledge`), under the selected model's rules.
fn kb_scope(st: &AppState, session: Option<i64>, project: Option<i64>, knowledge: Option<&super::knowledge::Draft>) -> Result<Option<super::knowledge::Scope>, String> {
    let cloud = matches!(st.picky.selection(), super::Selection::Cloud(_));
    st.picky.with_db(|c| super::knowledge::scope(c, session, project, knowledge, cloud))
}

/// What a question in a conversation (or a draft in `project_id`, with its `knowledge`) would search.
#[tauri::command]
pub fn picky_retrieval_plan(state: State<AppState>, session_id: Option<i64>, project_id: Option<i64>, exclude_files: Vec<i64>, knowledge: Option<super::knowledge::Draft>) -> Result<Plan, String> {
    let model = embed::model(&state.picky);
    let done = state.search.with_db(|c| indexed(c, &model)).unwrap_or_default();
    let mut p = state.picky.with_db(|c| plan(c, project_of(c, session_id, project_id)?, &done, &exclude_files))?;
    if let Some(scope) = kb_scope(&state, session_id, project_id, knowledge.as_ref())? {
        p.knowledge = Some(state.search.with_db(|c| super::knowledge::plan(c, &scope, &model))?);
    }
    Ok(p)
}

/// Passages for a question: indexed project files, then its 知识库 (empty when there is nothing to
/// search); an error means 8081 couldn't be used and the page asks what to do.
#[tauri::command]
pub async fn picky_retrieve(app: AppHandle, session_id: Option<i64>, project_id: Option<i64>, question: String, exclude_files: Vec<i64>, knowledge: Option<super::knowledge::Draft>) -> Result<Vec<Passage>, String> {
    let st = app.state::<AppState>();
    let done = st.search.with_db(|c| indexed(c, &embed::model(&st.picky))).unwrap_or_default();
    let p = st.picky.with_db(|c| plan(c, project_of(c, session_id, project_id)?, &done, &exclude_files))?;
    let mut out = retrieve(&app, question.trim(), &p).await?;
    if let Some(scope) = kb_scope(&st, session_id, project_id, knowledge.as_ref())? {
        out.extend(super::knowledge::retrieve(&app, question.trim(), &scope).await?);
    }
    Ok(out)
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
        const MOCK: &str = "mock-embedding";
        let c = memory_db();
        let s = crate::search::store::tests::memory_db();
        let sha = |a: i64| c.query_row("SELECT sha256 FROM artifacts WHERE id = ?1", [a], |r| r.get::<_, String>(0)).unwrap();
        let dir = crate::db::tests::temp_dir("picky-index");
        let p = library::create_project(&c, "P", "").unwrap();
        let a = add(&c, &dir, "向量.md", "# 选择\n向量数据库的选择标准。\n\n# 其他\n周末去公园散步。".as_bytes());
        let f = files::add_project_file(&c, p, a).unwrap();
        files::set_indexed(&c, f, true).unwrap();
        let segs = files::segments(&c, a).unwrap();
        let made: Vec<Chunk> = pieces(&segs).into_iter().map(|x| Chunk { vector: embed::mock_vector(&x.content), segment: x.segment, content: x.content }).collect();
        save(&s, a, &sha(a), MOCK, &made).unwrap();
        let pl = plan(&c, Some(p), &indexed(&s, MOCK).unwrap(), &[]).unwrap();
        assert_eq!((pl.ready.len(), pl.pending), (1, 0));
        assert!(plan(&c, Some(p), &indexed(&s, MOCK).unwrap(), &[f]).unwrap().ready.is_empty()); // sent in full
        assert!(plan(&c, Some(p), &indexed(&s, "another-model").unwrap(), &[]).unwrap().ready.is_empty()); // another model's index doesn't count

        let l: Loaded = Arc::new((chunks(&s, a, MOCK).unwrap(), files::segments(&c, a).unwrap().iter().map(|s| (s.ordinal, format!("向量.md · {}", s.label()))).collect()));
        let hits = rank(&embed::mock_vector("向量数据库怎么选择"), &[(f, a, "向量.md".into(), l)]);
        assert_eq!(hits[0].label, "向量.md · 选择");
        assert!(hits[0].content.contains("向量数据库"));

        // a new version leaves the old index out; turning it off removes it
        let b = add(&c, &dir, "向量.md", b"new text");
        files::replace_project_file(&c, f, b).unwrap();
        let pl = plan(&c, Some(p), &indexed(&s, MOCK).unwrap(), &[]).unwrap();
        assert_eq!((pl.ready.len(), pl.pending), (0, 1));
        prune(&s, &wanted(&c, None).unwrap().into_values().collect()).unwrap();
        assert!(indexed(&s, MOCK).unwrap().is_empty());
        // the same artifact id with other bytes (a restored picky.db) is not the same index
        save(&s, b, "other-sha", MOCK, &made).unwrap();
        assert_eq!(plan(&c, Some(p), &indexed(&s, MOCK).unwrap(), &[]).unwrap().pending, 1);
    }
}
