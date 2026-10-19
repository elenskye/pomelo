//! 联网搜索 (插件库): Tavily, called by Rust (the model only writes the search words). The key lives
//! only in the keychain (cloud.rs KeyStore, account `tavily`); requests go only to api.tavily.com.
//! The model gets each page's title, site and Tavily's extract; the pages become numbered sources.
//! Mock (dev / tests): three made-up pages about the query; 「联网失败测试」 fails like Tavily down.

use serde_json::{json, Value};
use std::time::Duration;

pub const ACCOUNT: &str = "tavily";
const URL: &str = "https://api.tavily.com/search";
pub const MOCK_FAILURE: &str = "联网失败测试";
const RESULTS: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub title: String,
    pub url: String,
    pub content: String,
}

/// The site a url is on (「example.com」).
pub fn site(url: &str) -> String {
    let host = url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or("");
    host.trim_start_matches("www.").to_string()
}

pub async fn search(key: Option<String>, query: &str, mock: bool) -> Result<Vec<Page>, String> {
    if mock {
        if query.contains(MOCK_FAILURE) {
            return Err("模拟：连不上 Tavily。".into());
        }
        return Ok((1..=3)
            .map(|i| Page {
                title: format!("模拟网页 {i}：{query}"),
                url: format!("https://example.com/mock/{i}?q={}", query.chars().filter(|c| c.is_alphanumeric()).collect::<String>()),
                content: format!("这是开发版的模拟搜索结果，讲的是「{query}」的第 {i} 件事。"),
            })
            .collect());
    }
    let key = key.ok_or("还没填 Tavily API Key（小拾 → 插件库 → 联网搜索）")?;
    let c = reqwest::Client::builder().timeout(Duration::from_secs(25)).build().map_err(|e| e.to_string())?;
    let resp = c
        .post(URL)
        .bearer_auth(key)
        .json(&json!({ "query": query, "max_results": RESULTS, "search_depth": "basic", "include_answer": false }))
        .send()
        .await
        .map_err(|_| "连不上 Tavily。".to_string())?;
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    match status {
        200 => {}
        401 | 403 => return Err("Tavily API Key 不对（小拾 → 插件库 → 联网搜索）。".into()),
        429 | 432 | 433 => return Err("Tavily 这个月的额度用完了，或者搜得太频繁。".into()),
        _ => return Err(format!("Tavily 返回了错误（{status}）。")),
    }
    let v: Value = serde_json::from_str(&body).map_err(|_| "Tavily 返回了看不懂的结果。".to_string())?;
    Ok(v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let url = r["url"].as_str()?.to_string();
            Some(Page { title: r["title"].as_str().unwrap_or(&url).trim().to_string(), content: r["content"].as_str().unwrap_or("").trim().to_string(), url })
        })
        .take(RESULTS)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_and_mock_pages() {
        assert_eq!(site("https://www.example.com/a/b?c=1"), "example.com");
        assert_eq!(site("http://news.ycombinator.com"), "news.ycombinator.com");
        let pages = tauri::async_runtime::block_on(search(None, "台风", true)).unwrap();
        assert_eq!(pages.len(), 3);
        assert!(pages[0].title.contains("台风"));
        assert!(tauri::async_runtime::block_on(search(None, MOCK_FAILURE, true)).is_err());
        assert!(tauri::async_runtime::block_on(search(None, "x", false)).unwrap_err().contains("Tavily API Key"));
    }

    /// Against the real Tavily (ask first): POMELO_TAVILY_KEY=… cargo test real_web -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_web() {
        let Ok(key) = std::env::var("POMELO_TAVILY_KEY") else { return };
        let q = std::env::var("POMELO_ASK").unwrap_or_else(|_| "今天的科技新闻".into());
        for p in tauri::async_runtime::block_on(search(Some(key), &q, false)).unwrap() {
            println!("{} · {}\n  {}", p.title, site(&p.url), p.content.chars().take(120).collect::<String>());
        }
    }
}
