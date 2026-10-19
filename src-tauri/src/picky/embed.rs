//! Turning text into vectors (from Picky's MemoryEmbedding), for project file indexes and memories:
//! - Real: the manifest's embedding model behind llmctl.sh on 127.0.0.1:8081 (OpenAI-compatible,
//!   never through a proxy). `prepare` starts it when needed (runtime::ensure). Questions are
//!   phrased with the model's `query` template; stored text is embedded as is.
//! - Mock (dev / tests): hashed characters and character pairs (256 dimensions), so texts sharing
//!   words score higher; never starts a process or opens a connection. A text containing
//!   「向量失败测试」 fails like an unavailable service.
//! Vectors made by another model are never compared: indexes and memory vectors are stored under
//! the model's key.

use super::catalog::ServiceModel;
use super::runtime::{Runtime, Svc};
use super::Picky;
use crate::AppState;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const MEMORY_TASK: &str = "Given a user's message, retrieve the personal notes that are relevant to it";
const PASSAGE_TASK: &str = "Given a user's question, retrieve passages from their project documents that help answer it";
const RECORD_TASK: &str = "Given a search query, retrieve the user's notes, records and conversations that match its meaning";
pub const MOCK_FAILURE: &str = "向量失败测试";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Document,
    MemoryQuery,
    PassageQuery,
    /// A search over Pomelo's records (search/query.rs).
    RecordQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EmbedError {
    /// The service refused an input as longer than it can embed at once.
    TooLong,
    Failed(String),
}

impl From<EmbedError> for String {
    fn from(e: EmbedError) -> String {
        match e {
            EmbedError::TooLong => "资料中有一段文字太长，没法建立索引。".into(),
            EmbedError::Failed(m) => m,
        }
    }
}

/// The key vectors are stored under: the manifest's embedding model ("" when it lists none,
/// which matches nothing stored).
pub fn model(p: &Picky) -> String {
    p.runtime.service_model(Svc::Embedding).map_or_else(String::new, |m| m.key)
}

/// Makes sure the service can answer, starting it if needed; the model to embed with.
pub async fn prepare(app: &AppHandle) -> Result<ServiceModel, String> {
    app.state::<AppState>().picky.runtime.ensure(Svc::Embedding).await
}

/// One vector per text, in order (batches of 16).
pub async fn embed(rt: &Runtime, model: &ServiceModel, texts: &[String], input: Input) -> Result<Vec<Vec<f32>>, EmbedError> {
    rt.touch(Svc::Embedding);
    if rt.is_mock() {
        if texts.iter().any(|t| t.contains(MOCK_FAILURE)) {
            return Err(EmbedError::Failed("模拟：8081 向量服务没有就绪。".into()));
        }
        return Ok(texts.iter().map(|t| mock_vector(t)).collect());
    }
    let c = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(30)).build().map_err(|e| EmbedError::Failed(e.to_string()))?;
    let mut out = vec![];
    for batch in texts.chunks(16) {
        let inputs: Vec<String> = batch
            .iter()
            .map(|t| match input {
                Input::PassageQuery => model.question(PASSAGE_TASK, t),
                Input::RecordQuery => model.question(RECORD_TASK, t),
                Input::MemoryQuery => model.question(MEMORY_TASK, t),
                Input::Document => t.clone(),
            })
            .collect();
        let resp = c
            .post(format!("{}/v1/embeddings", Svc::Embedding.root()))
            .json(&json!({ "model": model.key, "input": inputs }))
            .send()
            .await
            .map_err(|_| EmbedError::Failed("连不上 8081 向量服务。".into()))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if status != 200 {
            // llama-server: "input (N tokens) is too large to process…" / "…larger than the context size"
            let head: String = body.chars().take(2000).collect();
            return Err(if head.contains("too large") || head.contains("context size") { EmbedError::TooLong } else { EmbedError::Failed("8081 向量服务返回了错误。".into()) });
        }
        let v: Value = serde_json::from_str(&body).map_err(|_| EmbedError::Failed("8081 向量服务返回了无法读取的结果。".into()))?;
        let mut items: Vec<&Value> = v["data"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        if items.len() != batch.len() {
            return Err(EmbedError::Failed("8081 向量服务返回的数量不对。".into()));
        }
        items.sort_by_key(|i| i["index"].as_i64().unwrap_or(0));
        for i in items {
            let vec: Vec<f32> = i["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
            if vec.is_empty() {
                return Err(EmbedError::Failed("8081 向量服务返回了空向量。".into()));
            }
            out.push(vec);
        }
        rt.touch(Svc::Embedding);
    }
    Ok(out)
}

pub fn mock_vector(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 256];
    let chars: Vec<char> = text.to_lowercase().chars().filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation() && !"，。！？；：、（）《》「」“”".contains(*c)).collect();
    let mut add = |piece: &[char]| {
        let mut h: u32 = 2_166_136_261;
        for c in piece {
            h = (h ^ *c as u32).wrapping_mul(16_777_619);
        }
        v[(h % 256) as usize] += 1.0;
    };
    for i in 0..chars.len() {
        add(&chars[i..=i]);
        if i + 1 < chars.len() {
            add(&chars[i..=i + 1]);
        }
    }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::runtime::LlmPaths;
    use crate::picky::Mode;
    use std::path::Path;

    #[test]
    fn mock_vectors_score_shared_words_higher() {
        let q = mock_vector("向量数据库怎么选");
        assert!(cosine(&q, &mock_vector("选择向量数据库的几个标准")) > cosine(&q, &mock_vector("周末去公园散步")));
        assert!((cosine(&q, &q) - 1.0).abs() < 1e-5);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        let rt = Runtime::new(Mode::Mock, LlmPaths::new(Path::new("/nonexistent-home")));
        let m = rt.service_model(Svc::Embedding).unwrap();
        let r = tauri::async_runtime::block_on(embed(&rt, &m, &[format!("x{MOCK_FAILURE}")], Input::PassageQuery));
        assert!(matches!(r, Err(EmbedError::Failed(_))));
    }

    /// Against the real 8081 service (start it first: `llmctl.sh start embedding`).
    /// POMELO_REAL_LLM=1 cargo test real_embedding -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_embedding() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        let text = std::fs::read_to_string(Path::new(&home).join("Documents/LLM/config/models.json")).unwrap();
        let model = super::super::catalog::parse(&text).unwrap().embedding.remove(0);
        let rt = Runtime::new(Mode::Real, LlmPaths::new(Path::new(&home)));
        let texts = ["找房子", "整理了一下租房要注意的事：合同、押金、水电燃气的交接。", "午饭 35 元，和同事吃了牛肉面。"].map(String::from);
        let q = tauri::async_runtime::block_on(embed(&rt, &model, &texts[..1], Input::MemoryQuery)).unwrap().remove(0);
        let docs = tauri::async_runtime::block_on(embed(&rt, &model, &texts[1..], Input::Document)).unwrap();
        let (rent, lunch) = (cosine(&q, &docs[0]), cosine(&q, &docs[1]));
        println!("{} dims; 租房 {rent:.3}, 午饭 {lunch:.3}", q.len());
        assert!(rent > lunch);
    }
}
