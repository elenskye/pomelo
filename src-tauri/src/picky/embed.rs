//! Turning text into vectors (from Picky's MemoryEmbedding), for project file indexes and memories:
//! - Real: Qwen3-Embedding-0.6B behind `embeddingctl.sh` on 127.0.0.1:8081 (OpenAI-compatible,
//!   never through a proxy). `prepare` starts the service when needed and waits up to 45 s.
//!   Questions get the retrieval instruction; stored text is embedded as is.
//! - Mock (dev / tests): hashed characters and character pairs (256 dimensions), so texts sharing
//!   words score higher; never starts a process or opens a connection. A text containing
//!   「向量失败测试」 fails like an unavailable service.
//! Vectors made by another model are never compared: indexes are stored per model.

use super::{runtime::EmbState, Mode};
use crate::AppState;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

pub const REAL_MODEL: &str = "qwen3-embedding-0.6b";
pub const MOCK_MODEL: &str = "mock-embedding";
const ROOT: &str = "http://127.0.0.1:8081";
const READY_TIMEOUT: Duration = Duration::from_secs(45);
const MEMORY_QUERY: &str = "Instruct: Given a user's message, retrieve the personal notes that are relevant to it\nQuery:";
const PASSAGE_QUERY: &str = "Instruct: Given a user's question, retrieve passages from their project documents that help answer it\nQuery:";
pub const MOCK_FAILURE: &str = "向量失败测试";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Document,
    MemoryQuery,
    PassageQuery,
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

pub fn model(mode: Mode) -> &'static str {
    if mode == Mode::Mock {
        MOCK_MODEL
    } else {
        REAL_MODEL
    }
}

fn client(timeout: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(timeout)).build().map_err(|e| e.to_string())
}

async fn healthy() -> bool {
    let Ok(c) = client(2) else { return false };
    c.get(format!("{ROOT}/health")).send().await.is_ok_and(|r| r.status() == 200)
}

/// Makes sure the service can answer, starting it if needed.
pub async fn prepare(app: &AppHandle) -> Result<(), String> {
    let p = &app.state::<AppState>().picky;
    if p.mode == Mode::Mock {
        if p.runtime.emb_status().state != EmbState::Running {
            p.runtime.emb_control("start").await;
        }
        return Ok(());
    }
    if healthy().await {
        return Ok(());
    }
    p.runtime.emb_control("start").await;
    if let Some(issue) = p.runtime.emb_status().issue {
        return Err(format!("{}：{}", issue.title, issue.message));
    }
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if healthy().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("8081 向量服务没有在 45 秒内就绪。".into())
}

/// One vector per text, in order (batches of 16).
pub async fn embed(mode: Mode, texts: &[String], input: Input) -> Result<Vec<Vec<f32>>, EmbedError> {
    if mode == Mode::Mock {
        if texts.iter().any(|t| t.contains(MOCK_FAILURE)) {
            return Err(EmbedError::Failed("模拟：8081 向量服务没有就绪。".into()));
        }
        return Ok(texts.iter().map(|t| mock_vector(t)).collect());
    }
    let c = client(30).map_err(EmbedError::Failed)?;
    let mut out = vec![];
    for batch in texts.chunks(16) {
        let inputs: Vec<String> = batch
            .iter()
            .map(|t| match input {
                Input::PassageQuery => format!("{PASSAGE_QUERY}{t}"),
                Input::MemoryQuery => format!("{MEMORY_QUERY}{t}"),
                Input::Document => t.clone(),
            })
            .collect();
        let resp = c
            .post(format!("{ROOT}/v1/embeddings"))
            .json(&json!({ "model": REAL_MODEL, "input": inputs }))
            .send()
            .await
            .map_err(|_| EmbedError::Failed("连不上 8081 向量服务。".into()))?;
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        if status != 200 {
            // llama-server: "input (N tokens) is too large to process. increase the physical batch size"
            return Err(if body.chars().take(2000).collect::<String>().contains("too large") { EmbedError::TooLong } else { EmbedError::Failed("8081 向量服务返回了错误。".into()) });
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

    #[test]
    fn mock_vectors_score_shared_words_higher() {
        let q = mock_vector("向量数据库怎么选");
        assert!(cosine(&q, &mock_vector("选择向量数据库的几个标准")) > cosine(&q, &mock_vector("周末去公园散步")));
        assert!((cosine(&q, &q) - 1.0).abs() < 1e-5);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        let r = tauri::async_runtime::block_on(embed(Mode::Mock, &[format!("x{MOCK_FAILURE}")], Input::PassageQuery));
        assert!(matches!(r, Err(EmbedError::Failed(_))));
    }
}
