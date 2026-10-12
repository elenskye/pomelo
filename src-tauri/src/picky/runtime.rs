//! Local model services, as in the old Picky:
//! - Chat models: `~/Documents/LLM/scripts/llmctl.sh` starts / switches / stops them, one at a time,
//!   all on 127.0.0.1:8080. The embedding service (127.0.0.1:8081) has its own `embeddingctl.sh`.
//! - Status: the local API is polled every few seconds (`/health`, `/v1/models`); the script's
//!   `status` runs only on first use, when the API changes state, every 60 s (10 s while the two
//!   disagree) and after operations. Only models the script manages count as running.
//! - Mock mode (dev builds, tests, scratch data dirs): the built-in catalog and simulated
//!   operations — never a script, never a request.
//! These controls serve the UI and the menu bar only; they are never AI tools.

use super::catalog::{self, LocalModel};
use super::Mode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

const CHAT_ROOT: &str = "http://127.0.0.1:8080";
const EMBED_ROOT: &str = "http://127.0.0.1:8081";
pub const EMBED_MODEL: &str = "Qwen3-Embedding-0.6B";

#[derive(Debug, Clone)]
pub struct LlmPaths {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub llmctl: PathBuf,
    pub embeddingctl: PathBuf,
    pub server_log: PathBuf,
    pub embedding_log: PathBuf,
}

impl LlmPaths {
    pub fn new(home: &Path) -> Self {
        let root = home.join("Documents/LLM");
        LlmPaths {
            manifest: root.join("config/chat-models.json"),
            llmctl: root.join("scripts/llmctl.sh"),
            embeddingctl: root.join("scripts/embeddingctl.sh"),
            server_log: root.join("logs/llama-server.log"),
            embedding_log: root.join("logs/qwen3-embedding-0.6b.log"),
            root,
        }
    }
}

// ---------- status pieces ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Readiness {
    Idle,
    Loading,
    Ready,
}

/// What a llama-server answers on its root URL.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Probe {
    Offline,
    /// Answered but not ready (HTTP 503 while loading).
    Loading,
    /// `/health` 200; the alias from `/v1/models` when read.
    Ready(Option<String>),
}

/// The managed chat process as `llmctl.sh status` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Managed {
    None,
    Running(String),
}

impl Managed {
    /// First line: "Running: KEY (PID N)" or a "No managed …" line.
    fn parse(out: &str) -> Self {
        let first = out.lines().next().unwrap_or("");
        match first.strip_prefix("Running: ").and_then(|r| r.split(' ').next()) {
            Some(key) if !key.is_empty() => Managed::Running(key.into()),
            _ => Managed::None,
        }
    }
}

/// When the API-derived status must be confirmed by the control script.
#[derive(Debug, Default)]
struct Schedule {
    last_probe: Option<Probe>,
    last_check: Option<Instant>,
}

impl Schedule {
    const STEADY: Duration = Duration::from_secs(60);
    const UNSETTLED: Duration = Duration::from_secs(10);

    fn needs(&mut self, probe: &Probe, unsettled: bool, now: Instant) -> bool {
        let need = match self.last_check {
            None => true,
            Some(t) => self.last_probe.as_ref() != Some(probe) || now.duration_since(t) >= if unsettled { Self::UNSETTLED } else { Self::STEADY },
        };
        self.last_probe = Some(probe.clone());
        need
    }
    /// Records an attempt (successful or not) so a failing script isn't rerun every poll.
    fn checked(&mut self, now: Instant) {
        self.last_check = Some(now);
    }
    fn reset(&mut self) {
        *self = Schedule::default();
    }
}

/// Which model runs (script) and whether it is ready (API). `managed = None`: script not asked yet.
fn resolve(probe: &Probe, managed: Option<&Managed>, catalog: &[LocalModel]) -> (Option<LocalModel>, Readiness, Option<String>) {
    match managed {
        Some(Managed::None) => (None, Readiness::Idle, None), // an unrelated server on 8080 doesn't count
        Some(Managed::Running(key)) => {
            let alias = match probe {
                Probe::Ready(a) => a.clone(),
                _ => None,
            };
            let model = catalog.iter().find(|m| &m.key == key).cloned().unwrap_or_else(|| LocalModel::unlisted(key, alias.as_deref()));
            match probe {
                Probe::Ready(_) => {
                    let served = alias.unwrap_or_else(|| model.alias.clone());
                    (Some(model), Readiness::Ready, Some(served))
                }
                // offline right after start = port not open yet; a crash is caught by the unsettled re-check
                Probe::Loading | Probe::Offline => (Some(model), Readiness::Loading, None),
            }
        }
        None => match probe {
            Probe::Ready(Some(alias)) => match catalog.iter().find(|m| &m.alias == alias) {
                Some(m) => (Some(m.clone()), Readiness::Ready, Some(alias.clone())),
                None => (None, Readiness::Idle, None),
            },
            _ => (None, Readiness::Idle, None),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbState {
    Stopped,
    Starting,
    Running,
}

impl EmbState {
    /// `loaded`: launchd holds the service (status "Running:" / "Starting:"); None = not asked yet.
    fn resolve(probe: &Probe, loaded: Option<bool>) -> Self {
        match (loaded, probe) {
            (Some(false), _) => EmbState::Stopped,
            (Some(true), Probe::Ready(_)) => EmbState::Running,
            (Some(true), _) => EmbState::Starting,
            (None, Probe::Ready(_)) => EmbState::Running,
            (None, Probe::Loading) => EmbState::Starting,
            (None, Probe::Offline) => EmbState::Stopped,
        }
    }
    fn loaded(out: &str) -> bool {
        let first = out.lines().next().unwrap_or("");
        first.starts_with("Running:") || first.starts_with("Starting:")
    }
}

// ---------- issues ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    RetryStart { key: String },
    RetryStop,
    Refresh,
    OpenServerLog,
    OpenCatalog,
    OpenEmbeddingLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    /// A failed start / switch / stop; cleared by the next operation or dismissed.
    Operation,
    /// A failed status check; cleared once a later check succeeds.
    StatusCheck,
    Catalog,
    Embedding,
}

/// A problem phrased for the user: what happened, why, what next. Raw output stays in `details`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: IssueKind,
    pub title: String,
    pub message: String,
    pub details: Option<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone)]
pub struct ControlResult {
    pub code: i32,
    pub output: String,
}

fn details(command: &str, result: Option<&ControlResult>, error: Option<&str>) -> String {
    let mut lines = vec![command.to_string()];
    match (result, error) {
        (Some(r), _) => {
            if !r.output.is_empty() {
                lines.push(r.output.clone());
            }
            lines.push(format!("退出码 {}", r.code));
        }
        (None, Some(e)) => lines.push(e.into()),
        (None, None) => {}
    }
    lines.join("\n")
}

fn explain(output: &str, rules: &[(&str, &str)], fallback: &str) -> String {
    let lower = output.to_lowercase();
    rules.iter().find(|(needle, _)| lower.contains(&needle.to_lowercase())).map_or(fallback.into(), |(_, e)| (*e).into())
}

impl Issue {
    fn model_command(verb: &str, model: Option<&LocalModel>, result: Option<&ControlResult>, error: Option<&str>) -> Self {
        let name = model.map_or("模型".to_string(), |m| m.name.clone());
        let retry = || model.map(|m| Action::RetryStart { key: m.key.clone() }).into_iter().collect::<Vec<_>>();
        let (title, mut actions) = match verb {
            "stop" => (format!("无法停止{}", model.map_or("聊天模型".to_string(), |m| format!(" {}", m.name))), vec![Action::RetryStop]),
            "switch" => (format!("无法切换到 {name}"), retry()),
            _ => (format!("无法启动 {name}"), retry()),
        };
        let output = result.map_or("", |r| r.output.as_str());
        let message = if result.is_none() {
            "没能运行控制脚本 llmctl.sh。请确认 ~/Documents/LLM 目录完好，再重试。".to_string()
        } else {
            explain_model_control(output)
        };
        if verb != "stop" {
            actions.push(Action::OpenServerLog);
        }
        if output.contains("Model manifest") {
            actions.push(Action::OpenCatalog);
        }
        let command = if verb == "stop" { "llmctl.sh stop".to_string() } else { format!("llmctl.sh {verb} {}", model.map_or("", |m| m.key.as_str())) };
        Issue { kind: IssueKind::Operation, title, message, details: Some(details(&command, result, error)), actions }
    }

    fn status_check(result: Option<&ControlResult>, error: Option<&str>) -> Self {
        Issue {
            kind: IssueKind::StatusCheck,
            title: "无法查询聊天模型状态".into(),
            message: "控制脚本没有正常返回，显示的状态可能不准。会自动再试，也可以手动刷新。".into(),
            details: Some(details("llmctl.sh status", result, error)),
            actions: vec![Action::Refresh],
        }
    }

    fn catalog(detail: &str) -> Self {
        Issue {
            kind: IssueKind::Catalog,
            title: "模型清单有误".into(),
            message: format!("{}。修改 ~/Documents/LLM/config/chat-models.json 并保存后会自动重新读取。", detail.trim_start_matches("模型清单有误：")),
            details: Some(detail.into()),
            actions: vec![Action::OpenCatalog],
        }
    }

    fn not_listed(model: &LocalModel) -> Self {
        Issue {
            kind: IssueKind::Operation,
            title: format!("无法启动 {}", model.name),
            message: "这个模型已不在模型清单中，所以没有启动。可以在清单里加回它，或选择其他模型。".into(),
            details: None,
            actions: vec![Action::OpenCatalog],
        }
    }

    fn embedding_command(action: &str, result: Option<&ControlResult>, error: Option<&str>) -> Self {
        let title = match action {
            "stop" => "无法停止向量服务",
            "status" => "无法查询向量服务状态",
            _ => "无法启动向量服务",
        };
        let message = if result.is_none() {
            "没能运行控制脚本 embeddingctl.sh。请确认 ~/Documents/LLM 目录完好，再重试。".to_string()
        } else {
            explain_embedding_control(result.map_or("", |r| r.output.as_str()))
        };
        Issue {
            kind: if action == "status" { IssueKind::StatusCheck } else { IssueKind::Embedding },
            title: title.into(),
            message,
            details: Some(details(&format!("embeddingctl.sh {action}"), result, error)),
            actions: vec![Action::OpenEmbeddingLog],
        }
    }
}

/// Known `llmctl.sh` messages → reason and next step.
pub fn explain_model_control(output: &str) -> String {
    explain(
        output,
        &[
            ("Port 8080 is occupied", "8080 端口已被其他程序占用，所以没有启动新模型。可以先退出占用端口的程序，再重试。"),
            ("A managed model is already running", "已经有一个模型在运行。请先停止它，或直接切换到想用的模型。"),
            ("Another llmctl operation is running", "另一个模型操作还在进行。请稍等片刻再试。"),
            ("did not stop within 60 seconds", "当前模型 60 秒内没有停下来，所以没有启动新模型。请稍后重试。"),
            ("exited during startup", "模型服务启动后立刻退出了。可以打开服务日志查看原因，常见原因是内存不足或模型文件损坏。"),
            ("Model or mmproj file is missing", "找不到模型文件。请检查模型清单里的路径是否正确。"),
            ("Unknown model", "模型清单中没有这个模型。请检查清单，或选择其他模型。"),
            ("Model manifest", "模型清单有误，所以没有执行操作。请打开清单修改后重试。"),
            ("llama-server is missing", "找不到 llama-server 运行环境。请检查 ~/Documents/LLM/runtimes。"),
            ("Could not stop managed process", "无法停止模型进程。请稍后重试；仍然失败时，可以重启电脑后再试。"),
            ("control lock", "模型控制锁处于异常状态。请稍等片刻再试。"),
        ],
        "控制脚本报告了错误。可以查看技术详情里的原始输出，或打开服务日志。",
    )
}

pub fn explain_embedding_control(output: &str) -> String {
    explain(
        output,
        &[
            ("Port 8081 is occupied", "8081 端口已被其他程序占用，向量服务没有启动。可以先退出占用端口的程序，再重试。"),
            ("Missing model", "找不到向量模型文件。请检查 ~/Documents/LLM/embedding-models。"),
            ("Missing llama-server", "找不到 llama-server 运行环境。请检查 ~/Documents/LLM/runtimes。"),
            ("Missing service definition", "找不到向量服务的配置文件 config/qwen3-embedding-0.6b.plist。"),
            ("did not stop within", "向量服务 20 秒内没有停下来。请稍后重试。"),
        ],
        "控制脚本报告了错误。可以查看技术详情里的原始输出，或打开向量服务日志。",
    )
}

// ---------- presentation (one description for the page, settings and the menu bar) ----------

#[derive(Debug, Clone)]
enum Operation {
    Starting(LocalModel),
    Switching(LocalModel),
    Stopping(Option<LocalModel>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dot {
    Off,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Presentation {
    pub dot: Dot,
    /// Model button text.
    pub title: String,
    /// First line of the menu bar menu.
    pub menu_title: String,
    pub detail: String,
}

fn present(op: Option<&Operation>, issue: Option<&Issue>, model: Option<&LocalModel>, readiness: Readiness) -> Presentation {
    let p = |dot, title: String, menu_title: String, detail: &str| Presentation { dot, title, menu_title, detail: detail.into() };
    let loading = |m: &LocalModel| {
        let t = format!("正在加载 {}…", m.name);
        p(Dot::Loading, t.clone(), t, "正在加载")
    };
    match op {
        Some(Operation::Starting(m)) => return loading(m),
        Some(Operation::Switching(m)) => {
            let t = format!("正在切换到 {}…", m.name);
            return p(Dot::Loading, t.clone(), t, "正在切换");
        }
        Some(Operation::Stopping(m)) => {
            let t = format!("正在停止 {}…", m.as_ref().map_or("聊天模型", |m| m.name.as_str()));
            return p(Dot::Loading, t.clone(), t, "正在停止");
        }
        None => {}
    }
    // a failed operation always needs attention; other problems only replace the state when idle
    if let Some(i) = issue {
        if model.is_none() || i.kind == IssueKind::Operation {
            return p(Dot::Error, "需要处理".into(), "需要处理".into(), &i.title);
        }
    }
    let idle = || p(Dot::Off, "未运行聊天模型".into(), "未运行聊天模型".into(), "未运行");
    match (model, readiness) {
        (Some(m), Readiness::Ready) => p(Dot::Ready, m.name.clone(), format!("{} · 已就绪", m.name), "已就绪"),
        (Some(m), Readiness::Loading) => loading(m),
        _ => idle(),
    }
}

// ---------- the runtime ----------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub models: Vec<LocalModel>,
    pub catalog_error: Option<String>,
    pub active: Option<LocalModel>,
    pub readiness: Readiness,
    /// Model name the running server reports (sent with chat requests).
    pub served: Option<String>,
    /// An operation (start / switch / stop) is running.
    pub working: bool,
    pub issue: Option<Issue>,
    pub presentation: Presentation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbStatus {
    pub state: EmbState,
    pub working: bool,
    pub text: String,
    pub issue: Option<Issue>,
}

#[derive(Debug)]
struct Chat {
    catalog: Vec<LocalModel>,
    catalog_error: Option<String>,
    stamp: Option<(u64, SystemTime)>,
    checked: bool,
    active: Option<LocalModel>,
    readiness: Readiness,
    served: Option<String>,
    operation: Option<Operation>,
    issue: Option<Issue>,
    managed: Option<Managed>,
    schedule: Schedule,
    refreshing: bool,
}

#[derive(Debug)]
struct Emb {
    state: EmbState,
    working: bool,
    issue: Option<Issue>,
    loaded: Option<bool>,
    schedule: Schedule,
    refreshing: bool,
}

pub struct Runtime {
    mode: Mode,
    pub llm: LlmPaths,
    chat: Mutex<Chat>,
    emb: Mutex<Emb>,
    /// Real mode only: short-timeout client for the local status endpoints (never via a proxy).
    http: Option<reqwest::Client>,
}

impl Runtime {
    pub fn new(mode: Mode, llm: LlmPaths) -> Self {
        let http = (mode == Mode::Real).then(|| reqwest::Client::builder().no_proxy().timeout(Duration::from_millis(1500)).build().expect("http client"));
        let catalog = if mode == Mode::Mock { catalog::preview() } else { vec![] };
        Runtime {
            mode,
            llm,
            chat: Mutex::new(Chat {
                catalog,
                catalog_error: None,
                stamp: None,
                checked: false,
                active: None,
                readiness: Readiness::Idle,
                served: None,
                operation: None,
                issue: None,
                managed: None,
                schedule: Schedule::default(),
                refreshing: false,
            }),
            emb: Mutex::new(Emb { state: EmbState::Stopped, working: false, issue: None, loaded: None, schedule: Schedule::default(), refreshing: false }),
            http,
        }
    }

    fn chat(&self) -> std::sync::MutexGuard<'_, Chat> {
        self.chat.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn emb(&self) -> std::sync::MutexGuard<'_, Emb> {
        self.emb.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn local_status(&self) -> LocalStatus {
        let mut c = self.chat();
        self.reload_catalog(&mut c);
        let issue = c.issue.clone().or_else(|| c.catalog_error.as_deref().map(Issue::catalog));
        LocalStatus {
            models: c.catalog.clone(),
            catalog_error: c.catalog_error.clone(),
            active: c.active.clone(),
            readiness: c.readiness,
            served: c.served.clone(),
            working: c.operation.is_some(),
            presentation: present(c.operation.as_ref(), issue.as_ref(), c.active.as_ref(), c.readiness),
            issue,
        }
    }

    pub fn emb_status(&self) -> EmbStatus {
        let e = self.emb();
        let text = if e.working {
            "正在处理…"
        } else if e.issue.as_ref().is_some_and(|i| i.kind != IssueKind::StatusCheck) {
            "需要处理"
        } else {
            match e.state {
                EmbState::Stopped => "未运行",
                EmbState::Starting => "正在启动…",
                EmbState::Running => "已就绪",
            }
        };
        EmbStatus { state: e.state, working: e.working, text: text.into(), issue: e.issue.clone() }
    }

    /// The local model a reply can use now: (model, name the server serves).
    pub fn ready_model(&self) -> Option<(LocalModel, String)> {
        let c = self.chat();
        match (&c.active, c.readiness, &c.operation, &c.served) {
            (Some(m), Readiness::Ready, None, Some(s)) => Some((m.clone(), s.clone())),
            _ => None,
        }
    }

    /// Re-reads the manifest when its size or modification time changed. A broken manifest keeps
    /// the last good list so a running model stays usable while the file is being edited.
    fn reload_catalog(&self, c: &mut Chat) {
        if self.mode == Mode::Mock {
            return;
        }
        let stamp = std::fs::metadata(&self.llm.manifest).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
        if c.checked && stamp == c.stamp {
            return;
        }
        c.checked = true;
        c.stamp = stamp;
        match std::fs::read_to_string(&self.llm.manifest) {
            Err(_) => c.catalog_error = Some(format!("模型清单有误：找不到或无法读取 {}", self.llm.manifest.display())),
            Ok(text) => match catalog::parse(&text) {
                Ok(list) => {
                    c.catalog = list;
                    c.catalog_error = None;
                }
                Err(e) => c.catalog_error = Some(e),
            },
        }
    }

    fn apply(c: &mut Chat, snap: (Option<LocalModel>, Readiness, Option<String>)) {
        (c.active, c.readiness, c.served) = snap;
    }

    async fn probe(&self, root: &str, read_alias: bool) -> Probe {
        let Some(http) = &self.http else { return Probe::Offline };
        let Ok(r) = http.get(format!("{root}/health")).send().await else { return Probe::Offline };
        if r.status() != 200 {
            return Probe::Loading;
        }
        if !read_alias {
            return Probe::Ready(None);
        }
        let alias = async {
            let r = http.get(format!("{root}/v1/models")).send().await.ok()?;
            if r.status() != 200 {
                return None;
            }
            let v: serde_json::Value = serde_json::from_slice(&r.bytes().await.ok()?).ok()?;
            v["data"][0]["id"].as_str().map(String::from)
        }
        .await;
        Probe::Ready(alias)
    }

    /// Polls the API; runs `llmctl.sh status` only when the schedule asks or `confirm` is set.
    pub async fn refresh(&self, confirm: bool) {
        if self.mode == Mode::Mock {
            return;
        }
        {
            let mut c = self.chat();
            if c.operation.is_some() || c.refreshing {
                return;
            }
            c.refreshing = true;
            self.reload_catalog(&mut c);
        }
        let probe = self.probe(CHAT_ROOT, true).await;
        let need = {
            let mut c = self.chat();
            if c.operation.is_some() {
                c.refreshing = false;
                return;
            }
            let unsettled = matches!(c.managed, Some(Managed::Running(_))) && probe == Probe::Offline;
            let need = c.schedule.needs(&probe, unsettled, Instant::now()) || confirm;
            if need {
                c.schedule.checked(Instant::now());
            }
            need
        };
        let checked = if need { Some(run_script(&self.llm.llmctl, &["status"], &self.llm.root).await) } else { None };
        let mut c = self.chat();
        c.refreshing = false;
        if c.operation.is_some() {
            return;
        }
        match checked {
            Some(Ok(r)) if r.code == 0 => {
                c.managed = Some(Managed::parse(&r.output));
                if c.issue.as_ref().is_some_and(|i| i.kind == IssueKind::StatusCheck) {
                    c.issue = None;
                }
            }
            Some(res) if c.issue.is_none() || c.issue.as_ref().is_some_and(|i| i.kind == IssueKind::StatusCheck) => {
                c.issue = Some(match &res {
                    Ok(r) => Issue::status_check(Some(r), None),
                    Err(e) => Issue::status_check(None, Some(e)),
                });
            }
            _ => {}
        }
        let snap = resolve(&probe, c.managed.as_ref(), &c.catalog);
        Self::apply(&mut c, snap);
    }

    /// Starts (or switches to) a manifest model. Returns once the operation finished.
    pub async fn activate(&self, key: &str) {
        let (model, verb) = {
            let mut c = self.chat();
            if c.operation.is_some() {
                return;
            }
            self.reload_catalog(&mut c);
            let Some(model) = c.catalog.iter().find(|m| m.key == key).cloned() else {
                let gone = c.active.clone().filter(|m| m.key == key).unwrap_or_else(|| LocalModel::unlisted(key, None));
                c.issue = Some(Issue::not_listed(&gone));
                return;
            };
            let verb = if c.active.is_none() { "start" } else { "switch" };
            c.operation = Some(if verb == "start" { Operation::Starting(model.clone()) } else { Operation::Switching(model.clone()) });
            c.issue = None;
            (model, verb)
        };
        if self.mode == Mode::Mock {
            self.simulate_start(&model).await;
        } else {
            let res = run_script(&self.llm.llmctl, &[verb, &model.key], &self.llm.root).await;
            let failed = match &res {
                Ok(r) if r.code == 0 => None,
                Ok(r) => Some(Issue::model_command(verb, Some(&model), Some(r), None)),
                Err(e) => Some(Issue::model_command(verb, Some(&model), None, Some(e))),
            };
            self.chat().issue = failed;
        }
        {
            let mut c = self.chat();
            c.operation = None;
            c.schedule.reset();
        }
        self.refresh(false).await;
    }

    pub async fn stop(&self) {
        let stopping = {
            let mut c = self.chat();
            if c.operation.is_some() {
                return;
            }
            let m = c.active.clone();
            c.operation = Some(Operation::Stopping(m.clone()));
            c.issue = None;
            m
        };
        if self.mode == Mode::Mock {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Self::apply(&mut self.chat(), (None, Readiness::Idle, None));
        } else {
            let res = run_script(&self.llm.llmctl, &["stop"], &self.llm.root).await;
            let failed = match &res {
                Ok(r) if r.code == 0 => None,
                Ok(r) => Some(Issue::model_command("stop", stopping.as_ref(), Some(r), None)),
                Err(e) => Some(Issue::model_command("stop", stopping.as_ref(), None, Some(e))),
            };
            self.chat().issue = failed;
        }
        {
            let mut c = self.chat();
            c.operation = None;
            c.schedule.reset();
        }
        self.refresh(false).await;
    }

    async fn simulate_start(&self, model: &LocalModel) {
        Self::apply(&mut self.chat(), (None, Readiness::Idle, None));
        tokio::time::sleep(Duration::from_millis(700)).await;
        Self::apply(&mut self.chat(), (Some(model.clone()), Readiness::Loading, None));
        tokio::time::sleep(Duration::from_millis(1200)).await;
        Self::apply(&mut self.chat(), (Some(model.clone()), Readiness::Ready, Some(model.alias.clone())));
    }

    pub fn dismiss(&self, kind: IssueKind) {
        if kind == IssueKind::Embedding {
            self.emb().issue = None;
        } else {
            self.chat().issue = None;
        }
    }

    // ----- embedding service -----

    pub async fn emb_refresh(&self, confirm: bool) {
        if self.mode == Mode::Mock {
            return;
        }
        {
            let mut e = self.emb();
            if e.working || e.refreshing {
                return;
            }
            e.refreshing = true;
        }
        let probe = self.probe(EMBED_ROOT, false).await;
        let need = {
            let mut e = self.emb();
            let unsettled = e.loaded == Some(true) && probe == Probe::Offline;
            let need = e.schedule.needs(&probe, unsettled, Instant::now()) || confirm;
            if need {
                e.schedule.checked(Instant::now());
            }
            need
        };
        let checked = if need { Some(run_script(&self.llm.embeddingctl, &["status"], &self.llm.root).await) } else { None };
        let mut e = self.emb();
        e.refreshing = false;
        if e.working {
            return;
        }
        match checked {
            Some(Ok(r)) if r.code == 0 => {
                e.loaded = Some(EmbState::loaded(&r.output));
                if e.issue.as_ref().is_some_and(|i| i.kind == IssueKind::StatusCheck) {
                    e.issue = None;
                }
            }
            Some(res) if e.issue.is_none() || e.issue.as_ref().is_some_and(|i| i.kind == IssueKind::StatusCheck) => {
                e.issue = Some(match &res {
                    Ok(r) => Issue::embedding_command("status", Some(r), None),
                    Err(err) => Issue::embedding_command("status", None, Some(err)),
                });
            }
            _ => {}
        }
        e.state = EmbState::resolve(&probe, e.loaded);
    }

    /// `action`: start | stop.
    pub async fn emb_control(&self, action: &str) {
        {
            let mut e = self.emb();
            if e.working {
                return;
            }
            e.working = true;
            e.issue = None;
        }
        if self.mode == Mode::Mock {
            tokio::time::sleep(Duration::from_millis(600)).await;
            self.emb().state = if action == "start" { EmbState::Running } else { EmbState::Stopped };
        } else {
            let res = run_script(&self.llm.embeddingctl, &[action], &self.llm.root).await;
            let failed = match &res {
                Ok(r) if r.code == 0 => None,
                Ok(r) => Some(Issue::embedding_command(action, Some(r), None)),
                Err(e) => Some(Issue::embedding_command(action, None, Some(e))),
            };
            self.emb().issue = failed;
        }
        {
            let mut e = self.emb();
            e.working = false;
            e.schedule.reset();
        }
        self.emb_refresh(false).await;
    }
}

/// Runs a control script with a plain PATH from the LLM folder (output never holds our pipes:
/// llmctl.sh starts the server with nohup into its own log).
async fn run_script(script: &Path, args: &[&str], cwd: &Path) -> Result<ControlResult, String> {
    let (script, cwd) = (script.to_path_buf(), cwd.to_path_buf());
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .args(&args)
            .current_dir(&cwd)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        let mut text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let err = String::from_utf8_lossy(&out.stderr);
        if !err.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(err.trim());
        }
        Ok(ControlResult { code: out.status.code().unwrap_or(-1), output: text })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Vec<LocalModel> {
        catalog::preview()
    }

    #[test]
    fn managed_status_line() {
        assert_eq!(Managed::parse("Running: qwen14b (PID 123)\nLog: x"), Managed::Running("qwen14b".into()));
        assert_eq!(Managed::parse("No managed model is running."), Managed::None);
        assert_eq!(Managed::parse(""), Managed::None);
        assert!(EmbState::loaded("Starting: Qwen3-Embedding-0.6B (launchd is restarting it)"));
        assert!(!EmbState::loaded("Stopped"));
    }

    #[test]
    fn resolve_trusts_the_script_for_which_and_the_api_for_ready() {
        let c = cat();
        let (m, r, s) = resolve(&Probe::Ready(Some("served-x".into())), Some(&Managed::Running("qwen14b".into())), &c);
        assert_eq!((m.unwrap().key.as_str(), r, s.as_deref()), ("qwen14b", Readiness::Ready, Some("served-x")));
        let (m, r, _) = resolve(&Probe::Offline, Some(&Managed::Running("qwen14b".into())), &c);
        assert_eq!((m.unwrap().key.as_str(), r), ("qwen14b", Readiness::Loading));
        // something else on 8080 is not "running"
        let (m, r, _) = resolve(&Probe::Ready(Some("qwen3-14b-abliterated".into())), Some(&Managed::None), &c);
        assert_eq!((m, r), (None, Readiness::Idle));
        // before the script answered: trust a known alias
        let (m, r, _) = resolve(&Probe::Ready(Some("qwen3-14b-abliterated".into())), None, &c);
        assert_eq!((m.unwrap().key.as_str(), r), ("qwen14b", Readiness::Ready));
        // a running model removed from the manifest can still be shown
        let (m, _, s) = resolve(&Probe::Ready(None), Some(&Managed::Running("gone".into())), &c);
        let m = m.unwrap();
        assert!(!m.listed && m.title.contains("不在模型清单中"));
        assert_eq!(s.as_deref(), Some("gone"));
    }

    #[test]
    fn schedule_checks_on_change_and_by_interval() {
        let mut s = Schedule::default();
        let t0 = Instant::now();
        assert!(s.needs(&Probe::Offline, false, t0)); // first use
        s.checked(t0);
        assert!(!s.needs(&Probe::Offline, false, t0 + Duration::from_secs(5)));
        assert!(s.needs(&Probe::Loading, false, t0 + Duration::from_secs(6))); // state changed
        s.checked(t0 + Duration::from_secs(6));
        assert!(!s.needs(&Probe::Loading, true, t0 + Duration::from_secs(10)));
        assert!(s.needs(&Probe::Loading, true, t0 + Duration::from_secs(17))); // unsettled: 10 s
        s.checked(t0 + Duration::from_secs(17));
        assert!(!s.needs(&Probe::Loading, false, t0 + Duration::from_secs(70)));
        assert!(s.needs(&Probe::Loading, false, t0 + Duration::from_secs(78))); // steady: 60 s
    }

    #[test]
    fn embedding_state() {
        assert_eq!(EmbState::resolve(&Probe::Ready(None), Some(false)), EmbState::Stopped);
        assert_eq!(EmbState::resolve(&Probe::Offline, Some(true)), EmbState::Starting);
        assert_eq!(EmbState::resolve(&Probe::Ready(None), Some(true)), EmbState::Running);
        assert_eq!(EmbState::resolve(&Probe::Offline, None), EmbState::Stopped);
    }

    #[test]
    fn issues_explain_script_output() {
        let m = &cat()[0];
        let r = ControlResult { code: 1, output: "Port 8080 is occupied; no model was started.".into() };
        let i = Issue::model_command("start", Some(m), Some(&r), None);
        assert_eq!(i.title, "无法启动 Qwen3 14B");
        assert!(i.message.contains("8080 端口已被其他程序占用"));
        assert_eq!(i.actions, vec![Action::RetryStart { key: "qwen14b".into() }, Action::OpenServerLog]);
        assert_eq!(i.details.as_deref(), Some("llmctl.sh start qwen14b\nPort 8080 is occupied; no model was started.\n退出码 1"));
        let i = Issue::model_command("stop", None, None, Some("spawn failed"));
        assert_eq!((i.title.as_str(), i.actions.as_slice()), ("无法停止聊天模型", [Action::RetryStop].as_slice()));
        assert!(i.message.contains("没能运行控制脚本"));
        let r = ControlResult { code: 1, output: "Model manifest: bad".into() };
        assert!(Issue::model_command("switch", Some(m), Some(&r), None).actions.contains(&Action::OpenCatalog));
        assert!(explain_embedding_control("weird").contains("技术详情"));
    }

    #[test]
    fn presentation_matches_picky() {
        let m = cat()[0].clone();
        assert_eq!(present(None, None, None, Readiness::Idle).title, "未运行聊天模型");
        assert_eq!(present(None, None, Some(&m), Readiness::Ready).menu_title, "Qwen3 14B · 已就绪");
        assert_eq!(present(Some(&Operation::Switching(m.clone())), None, None, Readiness::Idle).title, "正在切换到 Qwen3 14B…");
        let check = Issue::status_check(None, Some("x"));
        // a status-check problem doesn't hide a running model
        assert_eq!(present(None, Some(&check), Some(&m), Readiness::Ready).dot, Dot::Ready);
        assert_eq!(present(None, Some(&check), None, Readiness::Idle).dot, Dot::Error);
        let op = Issue::model_command("start", Some(&m), None, Some("x"));
        assert_eq!(present(None, Some(&op), Some(&m), Readiness::Ready).title, "需要处理");
    }

    #[test]
    fn mock_mode_simulates_without_scripts() {
        tauri::async_runtime::block_on(async {
            let rt = Runtime::new(Mode::Mock, LlmPaths::new(Path::new("/nonexistent-home")));
            assert_eq!(rt.local_status().models.len(), 4);
            rt.activate("qwen359b").await;
            let s = rt.local_status();
            assert_eq!((s.active.unwrap().key.as_str(), s.readiness, s.working), ("qwen359b", Readiness::Ready, false));
            assert_eq!(rt.ready_model().unwrap().1, "qwen3.5-9b");
            rt.activate("nope").await;
            assert_eq!(rt.local_status().issue.unwrap().title, "无法启动 nope");
            rt.stop().await;
            assert!(rt.local_status().active.is_none());
            rt.emb_control("start").await;
            assert_eq!(rt.emb_status().text, "已就绪");
        });
    }
}
