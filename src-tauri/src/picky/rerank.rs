//! Reranking (v0.4): scores how well each passage answers a question, to reorder search results.
//! - Real: the manifest's rerank model behind llmctl.sh on 127.0.0.1:8082 (`/v1/rerank`, never
//!   through a proxy). The model file carries its own judging prompt (Qwen3-Reranker: the score is
//!   the chance of "yes"), so only the question and the passages are sent. `prepare` starts the
//!   service when needed (runtime::ensure); it stops again after 30 idle minutes.
//! - Speed: about 3600 tokens a second on this Mac (30 passages of ~200 tokens ≈ 1.7 s), so
//!   callers send a few dozen short snippets, not whole records.
//! - `pick` (project file passages, memories): a wider pool by similarity, reordered here, the
//!   clearly unrelated (score < FLOOR) dropped; None when the service can't be used, and the caller
//!   keeps its similarity order (a question is never held up by this).
//! - Mock (dev / tests): cosine of the mock vectors; 「重排失败测试」 fails like an unavailable
//!   service.

use super::catalog::ServiceModel;
use super::embed::{cosine, mock_vector};
use super::runtime::{Runtime, Svc};
use crate::AppState;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const MOCK_FAILURE: &str = "重排失败测试";
/// Below this the passage has nothing to do with the question (unrelated ones score ~0.0001).
pub const FLOOR: f32 = 0.01;

/// The `keep` best of `items` for `question` (each read as `text`), best first; None = the rerank
/// service couldn't be used.
pub async fn pick<T: Clone>(app: &AppHandle, question: &str, items: &[T], text: impl Fn(&T) -> String, keep: usize) -> Option<Vec<T>> {
    if items.is_empty() {
        return Some(vec![]);
    }
    let model = prepare(app).await.ok()?;
    let rt = &app.state::<AppState>().picky.runtime;
    let s = scores(rt, &model, question, &items.iter().map(text).collect::<Vec<_>>()).await.ok()?;
    Some(order(items, &s, keep))
}

/// Pure part of `pick`: by score (ties keep their order), at most `keep`, none under FLOOR.
pub fn order<T: Clone>(items: &[T], scores: &[f32], keep: usize) -> Vec<T> {
    let mut idx: Vec<usize> = (0..items.len().min(scores.len())).filter(|i| scores[*i] >= FLOOR).collect();
    idx.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]).then(a.cmp(b)));
    idx.into_iter().take(keep).map(|i| items[i].clone()).collect()
}

pub async fn prepare(app: &AppHandle) -> Result<ServiceModel, String> {
    app.state::<AppState>().picky.runtime.ensure(Svc::Rerank).await
}

/// One score per passage (0…1, higher is better), in the passages' order.
pub async fn scores(rt: &Runtime, model: &ServiceModel, question: &str, passages: &[String]) -> Result<Vec<f32>, String> {
    rt.touch(Svc::Rerank);
    if passages.is_empty() {
        return Ok(vec![]);
    }
    if rt.is_mock() {
        if question.contains(MOCK_FAILURE) {
            return Err("模拟：8082 重排服务没有就绪。".into());
        }
        let q = mock_vector(question);
        return Ok(passages.iter().map(|p| cosine(&q, &mock_vector(p)).max(0.0)).collect());
    }
    let c = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(60)).build().map_err(|e| e.to_string())?;
    let resp = c
        .post(format!("{}/v1/rerank", Svc::Rerank.root()))
        .json(&json!({ "model": model.key, "query": question, "documents": passages, "top_n": passages.len() }))
        .send()
        .await
        .map_err(|_| "连不上 8082 重排服务。".to_string())?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    rt.touch(Svc::Rerank);
    if status != 200 {
        return Err("8082 重排服务返回了错误。".into());
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "8082 重排服务返回了无法读取的结果。".to_string())?;
    let mut out = vec![f32::NAN; passages.len()];
    for r in v["results"].as_array().into_iter().flatten() {
        let (Some(i), Some(s)) = (r["index"].as_u64(), r["relevance_score"].as_f64()) else { continue };
        if let Some(slot) = out.get_mut(i as usize) {
            *slot = s as f32;
        }
    }
    if out.iter().any(|s| s.is_nan()) {
        return Err("8082 重排服务返回的数量不对。".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::runtime::LlmPaths;
    use crate::picky::Mode;
    use std::path::Path;

    #[test]
    fn order_keeps_the_best_above_the_floor() {
        assert_eq!(order(&["a", "b", "c", "d"], &[0.2, 0.9, 0.001, 0.2], 2), ["b", "a"]);
        assert_eq!(order(&["a", "b"], &[0.005, 0.0], 6), Vec::<&str>::new());
    }

    #[test]
    fn mock_scores_follow_shared_words() {
        let rt = Runtime::new(Mode::Mock, LlmPaths::new(Path::new("/nonexistent-home")));
        let m = rt.service_model(Svc::Rerank).unwrap();
        let docs = ["周末去公园散步。", "整理了租房要注意的事：合同、押金。"].map(String::from);
        let s = tauri::async_runtime::block_on(scores(&rt, &m, "租房合同", &docs)).unwrap();
        assert!(s[1] > s[0]);
        assert!(tauri::async_runtime::block_on(scores(&rt, &m, "x", &[])).unwrap().is_empty());
        assert!(tauri::async_runtime::block_on(scores(&rt, &m, MOCK_FAILURE, &docs)).is_err());
    }

    /// Against the real 8082 service (start it first: `llmctl.sh start rerank`).
    /// POMELO_REAL_LLM=1 cargo test real_rerank -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_rerank() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        let text = std::fs::read_to_string(Path::new(&home).join("Documents/LLM/config/models.json")).unwrap();
        let model = super::super::catalog::parse(&text).unwrap().rerank.remove(0);
        let rt = Runtime::new(Mode::Real, LlmPaths::new(Path::new(&home)));
        let docs = ["周末去看了两套出租的公寓，中介说押一付三，租金每月 4500。", "今天读完了《百年孤独》，结尾很震撼。", "午饭 35 元，和同事吃了牛肉面。", "整理了一下租房要注意的事：合同、押金、水电燃气的交接。"].map(String::from);
        let s = tauri::async_runtime::block_on(scores(&rt, &model, "我关于租房记过什么", &docs)).unwrap();
        println!("{s:?}");
        assert!(s[3] > 0.5 && s[3] > s[1] && s[3] > s[2] && s[0] > s[1]);
    }
}
