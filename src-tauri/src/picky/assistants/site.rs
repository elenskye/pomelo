//! 呦呦漫游 · 按中文起草英文版 (v0.5 阶段 3): a work's Chinese title, summary and body translated into
//! English by the 翻译 assistant's pipeline (translate.rs `translate_with`) with its model, 思考 and prompt
//! (own paragraph + keywords), Chinese → English, plus a note on the site's Markdown (image / link
//! addresses and Markdoc tags kept). The cloud rule is 呦呦漫游's (设置 → 小拾 → 发给云端).
//! - In memory only, for the page to take into the English version (the user edits and saves): never
//!   in picky.db, the translate page's last result untouched. One at a time; 停止 keeps what was written.
//! - The page polls `picky_site_draft` while it runs and clears it once taken (`picky_site_draft_clear`).

use super::super::assist;
use super::super::schedule;
use super::translate::{self, translate_with, Update};
use crate::AppState;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Added to the translation prompt: the body is the site's Markdown.
pub const NOTE: &str = "原文是个人网站上一篇作品的 Markdown：标题行、列表、引用、表格和 {% … %} 这类标签保持原样；图片 ![描述](地址) 只翻译方括号里的描述，链接 [文字](地址) 只翻译文字，地址都不改。";

#[derive(Default)]
struct Inner {
    running: bool,
    token: u64,
    slug: Option<String>,
    title: String,
    summary: String,
    body: String,
    status: Option<String>,
    error: Option<String>,
    model_name: Option<String>,
    note: Option<String>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

#[derive(Default)]
pub struct Live {
    inner: Mutex<Inner>,
}

impl Live {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The work it is for (None: nothing to take).
    pub slug: Option<String>,
    pub running: bool,
    pub title: String,
    pub summary: String,
    pub body: String,
    /// 正在启动本地模型… / 正文 · 正在翻译 2/5
    pub status: Option<String>,
    pub error: Option<String>,
    pub model_name: Option<String>,
    pub note: Option<String>,
}

fn view(app: &AppHandle) -> View {
    let st = app.state::<AppState>();
    let g = st.picky.assistants.site.lock();
    View { slug: g.slug.clone(), running: g.running, title: g.title.clone(), summary: g.summary.clone(), body: g.body.clone(), status: g.status.clone(), error: g.error.clone(), model_name: g.model_name.clone(), note: g.note.clone() }
}

/// A translated title: one line, no heading marks or quotes.
pub fn clean_title(s: &str) -> String {
    let line = s.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().trim_start_matches('#').trim();
    assist::clean_text(line)
}

fn set(app: &AppHandle, token: u64, f: impl FnOnce(&mut Inner)) {
    let st = app.state::<AppState>();
    let mut g = st.picky.assistants.site.lock();
    if g.token == token {
        f(&mut g);
    }
}

async fn run(app: AppHandle, token: u64, title: String, summary: String, body: String) {
    let st = app.state::<AppState>();
    let p = &st.picky;
    let saved = p.with_db(|c| Ok(translate::load(c).settings)).unwrap_or_default();
    let mut settings = saved.clone();
    (settings.source, settings.target) = ("zh".into(), "en".into());
    settings.prompt = [saved.prompt.trim(), NOTE].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("\n");
    let local = settings.model.as_deref().map_or(p.selection() == super::super::Selection::Local, |m| m.starts_with("local:"));
    if local && p.runtime.ready_model().is_none() {
        set(&app, token, |g| g.status = Some("正在启动本地模型…".into()));
    }
    let route = match schedule::route(p, settings.model.as_deref()).await {
        Ok((route, notice, _)) => {
            let note = (!notice.is_empty()).then(|| notice.replace("这个任务选的是", "翻译选的是").replace("到点时正在运行的是", "正在运行的是"));
            set(&app, token, |g| (g.note, g.model_name) = (note, Some(route.name.clone())));
            route
        }
        Err(e) => return finish(&app, token, Some(e)),
    };
    if let Err(e) = assist::check(p, &route, "site") {
        return finish(&app, token, Some(e));
    }
    for (part, text) in [("标题", &title), ("摘要", &summary), ("正文", &body)] {
        if text.trim().is_empty() {
            continue;
        }
        let app2 = app.clone();
        let result = translate_with(&route, &settings, text, move |u| {
            set(&app2, token, |g| match u {
                Update::Status(s) => g.status = Some(format!("{part} · {s}")),
                Update::Content(t) => match part {
                    "标题" => g.title.push_str(&t),
                    "摘要" => g.summary.push_str(&t),
                    _ => g.body.push_str(&t),
                },
            });
        })
        .await;
        if let Err(e) = result {
            return finish(&app, token, Some(e));
        }
        if part == "标题" {
            set(&app, token, |g| g.title = clean_title(&g.title));
        }
    }
    finish(&app, token, None);
}

fn finish(app: &AppHandle, token: u64, error: Option<String>) {
    set(app, token, |g| {
        (g.running, g.status, g.task, g.error) = (false, None, None, error);
        g.summary = g.summary.trim().to_string();
    });
}

#[tauri::command]
pub fn picky_site_draft(app: AppHandle) -> View {
    view(&app)
}

/// Starts drafting (returns at once).
#[tauri::command]
pub fn picky_site_draft_run(app: AppHandle, slug: String, title: String, summary: String, body: String) -> Result<View, String> {
    if title.trim().is_empty() && body.trim().is_empty() {
        return Err("中文版还没有标题和正文".into());
    }
    if body.chars().count() > translate::MAX_INPUT {
        return Err(format!("正文太长了（最多 {} 字）", translate::MAX_INPUT));
    }
    let token = {
        let st = app.state::<AppState>();
        let mut g = st.picky.assistants.site.lock();
        if g.running {
            return Err("正在起草另一个英文版，等它写完或先停止。".into());
        }
        g.token += 1;
        *g = Inner { running: true, token: g.token, slug: Some(slug), status: Some("正在准备…".into()), ..Default::default() };
        g.token
    };
    let task = tauri::async_runtime::spawn(run(app.clone(), token, title, summary, body));
    set(&app, token, |g| {
        if g.running {
            g.task = Some(task);
        }
    });
    Ok(view(&app))
}

/// Stops; what was written stays.
#[tauri::command]
pub fn picky_site_draft_stop(app: AppHandle) -> View {
    let token = {
        let st = app.state::<AppState>();
        let mut g = st.picky.assistants.site.lock();
        if let Some(t) = g.task.take() {
            t.abort();
        }
        g.token
    };
    finish(&app, token, None);
    view(&app)
}

/// The page took the result.
#[tauri::command]
pub fn picky_site_draft_clear(app: AppHandle) -> View {
    {
        let st = app.state::<AppState>();
        let mut g = st.picky.assistants.site.lock();
        if !g.running {
            let token = g.token;
            *g = Inner { token, ..Default::default() };
        }
    }
    view(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_come_back_clean() {
        assert_eq!(clean_title("# A Spring Walk\n"), "A Spring Walk");
        assert_eq!(clean_title("\n“Big Fish”"), "Big Fish");
        assert_eq!(clean_title("Hundred Tales: Big Fish\nextra"), "Hundred Tales: Big Fish");
    }

    /// The mock model through the same pipeline: the body's image and link survive (the mock echoes).
    #[test]
    fn drafts_with_the_mock_model() {
        use super::super::super::cloud::Think;
        use super::super::super::provider::Target;
        let route = super::super::super::Route { target: Target::Mock { thinking: true }, model: "local:mock".into(), name: "Mock".into(), window: 16_384, think: Think::Switch };
        let mut s = translate::Settings::default();
        (s.source, s.target, s.prompt) = ("zh".into(), "en".into(), NOTE.into());
        let body = "## 河边\n\n![樱花](/images/wander/a/b.jpg)\n\n见[相册](https://example.com)。\n";
        let mut out = String::new();
        tauri::async_runtime::block_on(translate_with(&route, &s, body, |u| {
            if let Update::Content(t) = u {
                out.push_str(&t)
            }
        }))
        .unwrap();
        assert!(out.starts_with("[Mock English]") && out.contains("/images/wander/a/b.jpg") && out.contains("https://example.com"));
    }

    /// The real local model (8080, started by hand) on a made-up work: 补全 (title + summary) and the
    /// English draft of a body with a heading, image, link, list and a Markdoc tag.
    ///   POMELO_REAL_LLM=1 [POMELO_THINK=1] cargo test real_site_draft -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_site_draft() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        use super::super::super::cloud::Think;
        use super::super::super::provider::Target;
        let model = std::env::var("POMELO_MODEL").unwrap_or_else(|_| "qwen3.5-9b".into());
        let route = super::super::super::Route { target: Target::Local { model }, model: "local:qwen359b".into(), name: "Qwen3.5 9B".into(), window: 32_768, think: Think::Switch };
        let title = "春天的散步";
        let body = "## 河边\n\n沿着河边走了一圈，樱花开了一半，风里还有一点凉。**最好看**的是桥下那一段，水很清。\n\n![桥下的河水和樱花](/images/wander/spring-walk/river.jpg)\n\n- 早上七点出门\n- 带了相机和一本书\n\n{% sharedImage image=\"wander-spring-card\" /%}\n\n更多照片见[相册](https://example.com/album)。\n";
        let t = std::time::Instant::now();
        let english = tauri::async_runtime::block_on(assist::site_english_with(&route, title, "", body)).unwrap();
        println!("===== 补全（{:.1}s）\n{english:?}", t.elapsed().as_secs_f64());
        let mut s = translate::Settings { thinking: std::env::var("POMELO_THINK").as_deref() == Ok("1"), ..Default::default() };
        (s.source, s.target, s.prompt) = ("zh".into(), "en".into(), NOTE.into());
        let t = std::time::Instant::now();
        let (mut out, mut head) = (String::new(), String::new());
        tauri::async_runtime::block_on(translate_with(&route, &s, title, |u| if let Update::Content(c) = u { head.push_str(&c) })).unwrap();
        tauri::async_runtime::block_on(translate_with(&route, &s, body, |u| if let Update::Content(c) = u { out.push_str(&c) })).unwrap();
        println!("===== 起草（{:.1}s）\n标题：{}\n{out}", t.elapsed().as_secs_f64(), clean_title(&head));
        assert!(out.contains("](/images/wander/spring-walk/river.jpg)") && out.contains("](https://example.com/album)") && out.contains("{% sharedImage") && out.contains("## "));
    }

    #[test]
    fn the_note_reaches_the_prompt() {
        let mut s = translate::Settings { prompt: "人名不译".into(), ..Default::default() };
        (s.source, s.target) = ("zh".into(), "en".into());
        s.prompt = format!("{}\n{NOTE}", s.prompt);
        let (from, to) = translate::direction(&s, "中文").unwrap();
        let sys = translate::instructions(&s, from, to);
        assert!(sys.contains("从简体中文翻译成英语") && sys.contains("人名不译") && sys.contains("地址都不改"));
    }
}
