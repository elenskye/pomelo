//! Local model services (from the old Picky; v0.4 adds the rerank service and idle stops):
//! - Everything goes through `~/Documents/LLM/scripts/llmctl.sh` and the manifest
//!   `config/models.json` (catalog.rs): chat models one at a time on 127.0.0.1:8080; the embedding
//!   (8081) and rerank (8082) services beside it, each running the first model of its kind.
//! - Status: each local API is polled every few seconds (`/health`, `/v1/models`); `llmctl.sh
//!   status` runs only on first use, when the API changes state, every 60 s (10 s while the two
//!   disagree) and after operations. Only models the script manages count as running.
//! - Pomelo starts every model with `--by pomelo` and only ever stops those on its own (status
//!   line "By: pomelo"): the embedding and rerank services after 30 minutes without use, and all
//!   three kinds when Pomelo quits. Anything started another way is left alone. `ensure` starts
//!   a service on demand and waits for it.
//! - Mock mode (dev builds, tests, scratch data dirs): the built-in catalog and simulated
//!   operations — never a script, never a request.
//! These controls serve the UI and the menu bar only; they are never AI tools.

use super::catalog::{self, Catalog, LocalModel, ServiceModel};
use super::Mode;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

const CHAT_ROOT: &str = "http://127.0.0.1:8080";
/// A service Pomelo started stops after this long without use.
pub const IDLE: Duration = Duration::from_secs(30 * 60);
const READY_TIMEOUT: Duration = Duration::from_secs(45);
/// `llmctl.sh start … --by OWNER`: marks what Pomelo may stop on its own.
const OWNER: &str = "pomelo";
const SCRIPT_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

#[derive(Debug, Clone)]
pub struct LlmPaths {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub llmctl: PathBuf,
    pub server_log: PathBuf,
    pub logs: PathBuf,
}

impl LlmPaths {
    pub fn new(home: &Path) -> Self {
        let root = home.join("Documents/LLM");
        LlmPaths { manifest: root.join("config/models.json"), llmctl: root.join("scripts/llmctl.sh"), server_log: root.join("logs/llama-server.log"), logs: root.join("logs"), root }
    }
}

/// The two services beside the chat model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Svc {
    Embedding,
    Rerank,
}

impl Svc {
    pub const ALL: [Svc; 2] = [Svc::Embedding, Svc::Rerank];

    /// The kind name llmctl.sh and the manifest use.
    pub fn arg(self) -> &'static str {
        match self {
            Svc::Embedding => "embedding",
            Svc::Rerank => "rerank",
        }
    }
    pub fn port(self) -> u16 {
        match self {
            Svc::Embedding => 8081,
            Svc::Rerank => 8082,
        }
    }
    pub fn root(self) -> String {
        format!("http://127.0.0.1:{}", self.port())
    }
    pub fn label(self) -> &'static str {
        match self {
            Svc::Embedding => "向量服务",
            Svc::Rerank => "重排服务",
        }
    }
    pub fn noun(self) -> &'static str {
        match self {
            Svc::Embedding => "向量模型",
            Svc::Rerank => "重排模型",
        }
    }
    fn idx(self) -> usize {
        self as usize
    }
    fn models(self, c: &Catalog) -> &[ServiceModel] {
        match self {
            Svc::Embedding => &c.embedding,
            Svc::Rerank => &c.rerank,
        }
    }
    fn issue_kind(self) -> IssueKind {
        match self {
            Svc::Embedding => IssueKind::Embedding,
            Svc::Rerank => IssueKind::Rerank,
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

/// A service as `llmctl.sh status <kind>` reports it: which model runs and who started it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ServiceScript {
    running: Option<String>,
    ours: bool,
}

impl ServiceScript {
    fn parse(out: &str) -> Self {
        match Managed::parse(out) {
            Managed::None => ServiceScript::default(),
            Managed::Running(key) => ServiceScript { running: Some(key), ours: out.lines().any(|l| l.trim() == format!("By: {OWNER}")) },
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
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
}

impl ServiceState {
    /// `loaded`: the script reports a running process; None = not asked yet.
    fn resolve(probe: &Probe, loaded: Option<bool>) -> Self {
        match (loaded, probe) {
            (Some(false), _) => ServiceState::Stopped,
            (Some(true), Probe::Ready(_)) => ServiceState::Running,
            (Some(true), _) => ServiceState::Starting,
            (None, Probe::Ready(_)) => ServiceState::Running,
            (None, Probe::Loading) => ServiceState::Starting,
            (None, Probe::Offline) => ServiceState::Stopped,
        }
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
    OpenServiceLog { service: Svc },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    /// A failed start / switch / stop; cleared by the next operation or dismissed.
    Operation,
    /// A failed status check; cleared once a later check succeeds.
    StatusCheck,
    Catalog,
    /// Any problem of that service (a failed status check of it clears itself too).
    Embedding,
    Rerank,
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

fn explain<S: AsRef<str>>(output: &str, rules: &[(&str, S)], fallback: &str) -> String {
    let lower = output.to_lowercase();
    rules.iter().find(|(needle, _)| lower.contains(&needle.to_lowercase())).map_or(fallback.into(), |(_, e)| e.as_ref().into())
}

const NO_SCRIPT: &str = "没能运行控制脚本 llmctl.sh。请确认 ~/Documents/LLM 目录完好，再重试。";

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
        let message = if result.is_none() { NO_SCRIPT.to_string() } else { explain_model_control(output) };
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
            message: format!("{}。修改 ~/Documents/LLM/config/models.json 并保存后会自动重新读取。", detail.trim_start_matches("模型清单有误：")),
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

    /// `action`: start | stop | status; `command`: what ran (for the details).
    fn service_command(svc: Svc, action: &str, command: &str, result: Option<&ControlResult>, error: Option<&str>) -> Self {
        let title = match action {
            "stop" => format!("无法停止{}", svc.label()),
            "status" => format!("无法查询{}状态", svc.label()),
            _ => format!("无法启动{}", svc.label()),
        };
        let output = result.map_or("", |r| r.output.as_str());
        let message = if result.is_none() { NO_SCRIPT.to_string() } else { explain_service_control(svc, output) };
        let mut actions = vec![Action::OpenServiceLog { service: svc }];
        if output.contains("Model manifest") {
            actions.push(Action::OpenCatalog);
        }
        Issue { kind: svc.issue_kind(), title, message, details: Some(details(command, result, error)), actions }
    }

    fn no_service_model(svc: Svc) -> Self {
        Issue {
            kind: svc.issue_kind(),
            title: format!("无法启动{}", svc.label()),
            message: format!("模型清单里没有{}。在 ~/Documents/LLM/config/models.json 里加上后会自动重新读取。", svc.noun()),
            details: None,
            actions: vec![Action::OpenCatalog],
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

pub fn explain_service_control(svc: Svc, output: &str) -> String {
    let (label, noun, port) = (svc.label(), svc.noun(), svc.port());
    explain(
        output,
        &[
            ("is occupied", format!("{port} 端口已被其他程序占用，{label}没有启动。可以先退出占用端口的程序，再重试。")),
            (&format!("Another {}", svc.arg()), format!("另一个{noun}已经在 {port} 端口运行（不是柚儿园启动的）。先停掉它再重试。")),
            ("Another llmctl operation is running", format!("{label}的另一个操作还在进行。请稍等片刻再试。")),
            ("did not stop within", format!("{label} 60 秒内没有停下来。请稍后重试。")),
            ("exited during startup", format!("{label}启动后立刻退出了。可以打开服务日志查看原因，常见原因是内存不足或模型文件损坏。")),
            ("Model or mmproj file is missing", format!("找不到{noun}文件。请检查模型清单里的路径是否正确。")),
            (&format!("No {} model in", svc.arg()), format!("模型清单里没有{noun}。")),
            ("Model manifest", "模型清单有误，所以没有执行操作。请打开清单修改后重试。".to_string()),
            ("llama-server is missing", "找不到 llama-server 运行环境。请检查 ~/Documents/LLM/runtimes。".to_string()),
            ("control lock", "模型控制锁处于异常状态。请稍等片刻再试。".to_string()),
        ],
        "控制脚本报告了错误。可以查看技术详情里的原始输出，或打开服务日志。",
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
pub struct ServiceStatus {
    pub state: ServiceState,
    pub working: bool,
    pub text: String,
    pub issue: Option<Issue>,
    /// The model this service runs (the one chosen in settings, else the manifest's first of its
    /// kind); None: the manifest lists none.
    pub model: Option<ServiceModel>,
    /// The manifest's models of this kind (the settings menu).
    pub models: Vec<ServiceModel>,
    /// Started by Pomelo: stopped after 30 idle minutes and when Pomelo quits.
    pub ours: bool,
}

/// The manifest as last read. A broken file keeps the last good lists so running models stay
/// usable while it is being edited.
#[derive(Debug, Default)]
struct Cat {
    catalog: Catalog,
    error: Option<String>,
    stamp: Option<(u64, SystemTime)>,
    checked: bool,
}

#[derive(Debug)]
struct Chat {
    active: Option<LocalModel>,
    readiness: Readiness,
    served: Option<String>,
    operation: Option<Operation>,
    issue: Option<Issue>,
    managed: Option<Managed>,
    /// The running chat model was started by Pomelo (stopped when Pomelo quits).
    ours: bool,
    schedule: Schedule,
    refreshing: bool,
}

#[derive(Debug)]
struct Service {
    state: ServiceState,
    working: bool,
    issue: Option<Issue>,
    /// The issue came from a status check (cleared by the next good check, not "需要处理").
    check_issue: bool,
    /// What the script last said (None: not asked yet).
    script: Option<ServiceScript>,
    last_used: Instant,
    schedule: Schedule,
    refreshing: bool,
}

impl Service {
    fn new() -> Self {
        Service { state: ServiceState::Stopped, working: false, issue: None, check_issue: false, script: None, last_used: Instant::now(), schedule: Schedule::default(), refreshing: false }
    }
    fn ours(&self) -> bool {
        self.script.as_ref().is_some_and(|s| s.ours)
    }
}

pub struct Runtime {
    mode: Mode,
    pub llm: LlmPaths,
    cat: Mutex<Cat>,
    chat: Mutex<Chat>,
    services: [Mutex<Service>; 2],
    /// The model chosen in settings for each service (picky.db `model.embedding` / `model.rerank`).
    chosen: Mutex<[Option<String>; 2]>,
    /// Real mode only: short-timeout client for the local status endpoints (never via a proxy).
    http: Option<reqwest::Client>,
}

impl Runtime {
    pub fn new(mode: Mode, llm: LlmPaths) -> Self {
        let http = (mode == Mode::Real).then(|| reqwest::Client::builder().no_proxy().timeout(Duration::from_millis(1500)).build().expect("http client"));
        let cat = if mode == Mode::Mock { Cat { catalog: catalog::preview(), checked: true, ..Cat::default() } } else { Cat::default() };
        Runtime {
            mode,
            llm,
            cat: Mutex::new(cat),
            chat: Mutex::new(Chat { active: None, readiness: Readiness::Idle, served: None, operation: None, issue: None, managed: None, ours: false, schedule: Schedule::default(), refreshing: false }),
            services: [Mutex::new(Service::new()), Mutex::new(Service::new())],
            chosen: Mutex::new([None, None]),
            http,
        }
    }

    pub fn is_mock(&self) -> bool {
        self.mode == Mode::Mock
    }

    fn chat(&self) -> std::sync::MutexGuard<'_, Chat> {
        self.chat.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn svc(&self, svc: Svc) -> std::sync::MutexGuard<'_, Service> {
        self.services[svc.idx()].lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The manifest (re-read when its size or modification time changed) and its problem, if any.
    fn catalog(&self) -> (Catalog, Option<String>) {
        let mut c = self.cat.lock().unwrap_or_else(|e| e.into_inner());
        if self.mode == Mode::Real {
            let stamp = std::fs::metadata(&self.llm.manifest).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
            if !c.checked || stamp != c.stamp {
                c.checked = true;
                c.stamp = stamp;
                match std::fs::read_to_string(&self.llm.manifest) {
                    Err(_) => c.error = Some(format!("模型清单有误：找不到或无法读取 {}", self.llm.manifest.display())),
                    Ok(text) => match catalog::parse(&text) {
                        Ok(list) => {
                            c.catalog = list;
                            c.error = None;
                        }
                        Err(e) => c.error = Some(e),
                    },
                }
            }
        }
        (c.catalog.clone(), c.error.clone())
    }

    /// The model a service runs: the chosen one while the manifest lists it, else its first.
    pub fn service_model(&self, svc: Svc) -> Option<ServiceModel> {
        let catalog = self.catalog().0;
        let models = svc.models(&catalog);
        let chosen = self.chosen.lock().unwrap_or_else(|e| e.into_inner())[svc.idx()].clone();
        chosen.and_then(|k| models.iter().find(|m| m.key == k)).or(models.first()).cloned()
    }

    pub fn choose(&self, svc: Svc, key: Option<String>) {
        self.chosen.lock().unwrap_or_else(|e| e.into_inner())[svc.idx()] = key;
    }

    /// The service's log: the running model's, else the manifest model's.
    pub fn service_log(&self, svc: Svc) -> Option<PathBuf> {
        let running = self.svc(svc).script.as_ref().and_then(|s| s.running.clone());
        running.or_else(|| self.service_model(svc).map(|m| m.key)).map(|k| self.llm.logs.join(format!("{k}.log")))
    }

    pub fn local_status(&self) -> LocalStatus {
        let (catalog, catalog_error) = self.catalog();
        let c = self.chat();
        let issue = c.issue.clone().or_else(|| catalog_error.as_deref().map(Issue::catalog));
        LocalStatus {
            models: catalog.chat,
            catalog_error,
            active: c.active.clone(),
            readiness: c.readiness,
            served: c.served.clone(),
            working: c.operation.is_some(),
            presentation: present(c.operation.as_ref(), issue.as_ref(), c.active.as_ref(), c.readiness),
            issue,
        }
    }

    pub fn service_status(&self, svc: Svc) -> ServiceStatus {
        let model = self.service_model(svc);
        let s = self.svc(svc);
        let text = if s.working {
            "正在处理…"
        } else if s.issue.is_some() && !s.check_issue {
            "需要处理"
        } else {
            match s.state {
                ServiceState::Stopped => "未运行",
                ServiceState::Starting => "正在启动…",
                ServiceState::Running => "已就绪",
            }
        };
        let models = svc.models(&self.catalog().0).to_vec();
        ServiceStatus { state: s.state, working: s.working, text: text.into(), issue: s.issue.clone(), model, models, ours: s.ours() }
    }

    /// The local model a reply can use now: (model, name the server serves).
    pub fn ready_model(&self) -> Option<(LocalModel, String)> {
        let c = self.chat();
        match (&c.active, c.readiness, &c.operation, &c.served) {
            (Some(m), Readiness::Ready, None, Some(s)) => Some((m.clone(), s.clone())),
            _ => None,
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
        let chat_models = self.catalog().0.chat;
        let mut c = self.chat();
        c.refreshing = false;
        if c.operation.is_some() {
            return;
        }
        match checked {
            Some(Ok(r)) if r.code == 0 => {
                c.managed = Some(Managed::parse(&r.output));
                c.ours = ServiceScript::parse(&r.output).ours;
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
        let snap = resolve(&probe, c.managed.as_ref(), &chat_models);
        Self::apply(&mut c, snap);
    }

    /// Starts (or switches to) a manifest model. Returns once the operation finished.
    pub async fn activate(&self, key: &str) {
        let chat_models = self.catalog().0.chat;
        let (model, verb) = {
            let mut c = self.chat();
            if c.operation.is_some() {
                return;
            }
            let Some(model) = chat_models.iter().find(|m| m.key == key).cloned() else {
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
            let res = run_script(&self.llm.llmctl, &[verb, &model.key, "--by", OWNER], &self.llm.root).await;
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
            let mut c = self.chat();
            Self::apply(&mut c, (None, Readiness::Idle, None));
            c.ours = false;
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
        let mut c = self.chat();
        Self::apply(&mut c, (Some(model.clone()), Readiness::Ready, Some(model.alias.clone())));
        c.ours = true;
    }

    pub fn dismiss(&self, kind: IssueKind) {
        match kind {
            IssueKind::Embedding => self.svc(Svc::Embedding).issue = None,
            IssueKind::Rerank => self.svc(Svc::Rerank).issue = None,
            _ => self.chat().issue = None,
        }
    }

    // ----- embedding and rerank services -----

    pub async fn service_refresh(&self, svc: Svc, confirm: bool) {
        if self.mode == Mode::Mock {
            return;
        }
        {
            let mut s = self.svc(svc);
            if s.working || s.refreshing {
                return;
            }
            s.refreshing = true;
        }
        let probe = self.probe(&svc.root(), false).await;
        let need = {
            let mut s = self.svc(svc);
            let unsettled = s.script.as_ref().is_some_and(|x| x.running.is_some()) && probe == Probe::Offline;
            let need = s.schedule.needs(&probe, unsettled, Instant::now()) || confirm;
            if need {
                s.schedule.checked(Instant::now());
            }
            need
        };
        let command = format!("llmctl.sh status {}", svc.arg());
        let checked = if need { Some(run_script(&self.llm.llmctl, &["status", svc.arg()], &self.llm.root).await) } else { None };
        let mut s = self.svc(svc);
        s.refreshing = false;
        if s.working {
            return;
        }
        match checked {
            Some(Ok(r)) if r.code == 0 => {
                s.script = Some(ServiceScript::parse(&r.output));
                if s.check_issue {
                    (s.issue, s.check_issue) = (None, false);
                }
            }
            Some(res) if s.issue.is_none() || s.check_issue => {
                s.issue = Some(match &res {
                    Ok(r) => Issue::service_command(svc, "status", &command, Some(r), None),
                    Err(e) => Issue::service_command(svc, "status", &command, None, Some(e)),
                });
                s.check_issue = true;
            }
            _ => {}
        }
        s.state = ServiceState::resolve(&probe, s.script.as_ref().map(|x| x.running.is_some()));
    }

    /// `action`: start | switch (to the chosen model, marked as Pomelo's) | stop.
    pub async fn service_control(&self, svc: Svc, action: &str) {
        let model = self.service_model(svc);
        {
            let mut s = self.svc(svc);
            if s.working {
                return;
            }
            if action != "stop" && model.is_none() {
                (s.issue, s.check_issue) = (Some(Issue::no_service_model(svc)), false);
                return;
            }
            s.working = true;
            s.issue = None;
        }
        let key = model.map(|m| m.key).unwrap_or_default();
        if self.mode == Mode::Mock {
            tokio::time::sleep(Duration::from_millis(600)).await;
            let mut s = self.svc(svc);
            if action != "stop" {
                s.state = ServiceState::Running;
                s.script = Some(ServiceScript { running: Some(key), ours: true });
                s.last_used = Instant::now();
            } else {
                s.state = ServiceState::Stopped;
                s.script = Some(ServiceScript::default());
            }
        } else {
            let args: Vec<&str> = if action == "stop" { vec!["stop", svc.arg()] } else { vec![action, &key, "--by", OWNER] };
            let command = format!("llmctl.sh {}", args.join(" "));
            let res = run_script(&self.llm.llmctl, &args, &self.llm.root).await;
            let failed = match &res {
                Ok(r) if r.code == 0 => None,
                Ok(r) => Some(Issue::service_command(svc, action, &command, Some(r), None)),
                Err(e) => Some(Issue::service_command(svc, action, &command, None, Some(e))),
            };
            let mut s = self.svc(svc);
            if failed.is_none() && action != "stop" {
                s.last_used = Instant::now();
            }
            (s.issue, s.check_issue) = (failed, false);
        }
        {
            let mut s = self.svc(svc);
            s.working = false;
            s.schedule.reset();
        }
        self.service_refresh(svc, false).await;
    }

    /// Something used the service (restarts its idle countdown).
    pub fn touch(&self, svc: Svc) {
        self.svc(svc).last_used = Instant::now();
    }

    /// Services Pomelo started that have gone unused for `IDLE` at `now`.
    pub fn idle_services(&self, now: Instant) -> Vec<Svc> {
        Svc::ALL
            .into_iter()
            .filter(|svc| {
                let s = self.svc(*svc);
                s.ours() && s.state != ServiceState::Stopped && !s.working && now.saturating_duration_since(s.last_used) >= IDLE
            })
            .collect()
    }

    /// What Pomelo started and still runs (llmctl.sh kinds): stopped when Pomelo quits.
    fn ours(&self) -> Vec<&'static str> {
        let chat = self.chat().ours.then_some("chat");
        chat.into_iter().chain(Svc::ALL.into_iter().filter(|s| self.svc(*s).ours()).map(Svc::arg)).collect()
    }

    /// Pomelo is quitting: stops the chat model and services it started without waiting for them.
    pub fn stop_ours_on_exit(&self) {
        if self.mode == Mode::Mock {
            return;
        }
        for kind in self.ours() {
            let _ = std::process::Command::new("/bin/sh")
                .arg(&self.llm.llmctl)
                .args(["stop", kind])
                .current_dir(&self.llm.root)
                .env("PATH", SCRIPT_PATH)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }

    /// Makes the service answer with the manifest's model, starting it if needed (waits up to 45 s).
    pub async fn ensure(&self, svc: Svc) -> Result<ServiceModel, String> {
        let model = self.service_model(svc).ok_or_else(|| format!("模型清单里没有{}，{}用不了。", svc.noun(), svc.label()))?;
        self.touch(svc);
        let failed = |i: Issue| format!("{}：{}", i.title, i.message);
        if self.mode == Mode::Mock {
            let running = self.svc(svc).script.as_ref().and_then(|s| s.running.clone());
            if self.service_status(svc).state != ServiceState::Running || running.as_deref() != Some(model.key.as_str()) {
                self.service_control(svc, "start").await;
            }
            return self.service_status(svc).issue.map_or(Ok(model), |i| Err(failed(i)));
        }
        let serves = |probe: Probe| match probe {
            Probe::Ready(Some(a)) if a != model.key => Some(Err(format!("{}（127.0.0.1:{}）运行的是 {a}，不是选的 {}。", svc.label(), svc.port(), model.key))),
            Probe::Ready(_) => Some(Ok(model.clone())),
            _ => None,
        };
        let action = match serves(self.probe(&svc.root(), true).await) {
            // another model of Pomelo's runs (the choice changed): switch it
            Some(Err(_)) if self.svc(svc).ours() => "switch",
            Some(r) => return r,
            None => "start",
        };
        self.service_control(svc, action).await;
        if let Some(i) = self.service_status(svc).issue.filter(|_| !self.svc(svc).check_issue) {
            return Err(failed(i));
        }
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(r) = serves(self.probe(&svc.root(), true).await) {
                self.touch(svc);
                return r;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(format!("{}（127.0.0.1:{}）没有在 45 秒内就绪。", svc.label(), svc.port()))
    }
}

/// Runs a control script with a plain PATH from the LLM folder (output never holds our pipes:
/// llmctl.sh starts servers with nohup into their own logs).
async fn run_script(script: &Path, args: &[&str], cwd: &Path) -> Result<ControlResult, String> {
    let (script, cwd) = (script.to_path_buf(), cwd.to_path_buf());
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .args(&args)
            .current_dir(&cwd)
            .env("PATH", SCRIPT_PATH)
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
        catalog::preview().chat
    }

    #[test]
    fn status_lines() {
        assert_eq!(Managed::parse("Running: qwen14b (PID 123)\nLog: x"), Managed::Running("qwen14b".into()));
        assert_eq!(Managed::parse("No managed model is running."), Managed::None);
        assert_eq!(Managed::parse(""), Managed::None);
        let s = ServiceScript::parse("Running: qwen3-reranker-0.6b (PID 9)\nBy: pomelo\nAPI: http://127.0.0.1:8082/v1");
        assert_eq!(s, ServiceScript { running: Some("qwen3-reranker-0.6b".into()), ours: true });
        assert!(!ServiceScript::parse("Running: e (PID 9)\nBy: claude-test").ours);
        assert_eq!(ServiceScript::parse("No managed embedding model is running."), ServiceScript::default());
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
    fn service_state() {
        assert_eq!(ServiceState::resolve(&Probe::Ready(None), Some(false)), ServiceState::Stopped);
        assert_eq!(ServiceState::resolve(&Probe::Offline, Some(true)), ServiceState::Starting);
        assert_eq!(ServiceState::resolve(&Probe::Ready(None), Some(true)), ServiceState::Running);
        assert_eq!(ServiceState::resolve(&Probe::Offline, None), ServiceState::Stopped);
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

        let r = ControlResult { code: 1, output: "Port 8082 is occupied; no model was started.".into() };
        let i = Issue::service_command(Svc::Rerank, "start", "llmctl.sh start rr --by pomelo", Some(&r), None);
        assert_eq!((i.kind, i.title.as_str()), (IssueKind::Rerank, "无法启动重排服务"));
        assert!(i.message.starts_with("8082 端口已被其他程序占用，重排服务没有启动"));
        assert_eq!(i.actions, vec![Action::OpenServiceLog { service: Svc::Rerank }]);
        let r = ControlResult { code: 1, output: "Another embedding model is running (x, PID 3). Use switch or stop first.".into() };
        assert!(explain_service_control(Svc::Embedding, &r.output).contains("另一个向量模型已经在 8081 端口运行"));
        assert!(explain_service_control(Svc::Rerank, "weird").contains("技术详情"));
        assert!(Issue::no_service_model(Svc::Embedding).message.contains("模型清单里没有向量模型"));
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
            assert!(rt.ours().is_empty());
            let s = rt.service_status(Svc::Rerank);
            assert_eq!((s.text.as_str(), s.model.unwrap().name.as_str(), s.ours), ("未运行", "Qwen3-Reranker-0.6B", false));
            rt.service_control(Svc::Embedding, "start").await;
            assert_eq!(rt.service_status(Svc::Embedding).text, "已就绪");
            assert_eq!(rt.ensure(Svc::Rerank).await.unwrap().key, "mock-reranker"); // started on demand
            assert!(rt.service_status(Svc::Rerank).ours);
            rt.activate("qwen14b").await;
            assert_eq!(rt.ours(), vec!["chat", "embedding", "rerank"]); // all stopped when Pomelo quits
            rt.service_control(Svc::Rerank, "stop").await;
            assert_eq!(rt.service_status(Svc::Rerank).state, ServiceState::Stopped);
        });
    }

    /// The real llmctl.sh and services (stopped again at the end; both must be stopped before).
    /// POMELO_REAL_LLM=1 cargo test real_services_start_and_stop -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_services_start_and_stop() {
        if std::env::var("POMELO_REAL_LLM").as_deref() != Ok("1") {
            return;
        }
        let home = std::env::var("HOME").unwrap();
        let rt = Runtime::new(Mode::Real, LlmPaths::new(Path::new(&home)));
        tauri::async_runtime::block_on(async {
            for svc in Svc::ALL {
                let t = Instant::now();
                let m = rt.ensure(svc).await.unwrap();
                rt.service_refresh(svc, true).await;
                let s = rt.service_status(svc);
                println!("{}: {} ready in {:.1} s, {:?}", svc.label(), m.key, t.elapsed().as_secs_f32(), s.text);
                assert!(s.ours && s.state == ServiceState::Running && s.issue.is_none());
                assert_eq!(rt.idle_services(Instant::now() + IDLE), vec![svc]);
                rt.service_control(svc, "stop").await;
                assert_eq!(rt.service_status(svc).state, ServiceState::Stopped);
            }
            rt.ensure(Svc::Embedding).await.unwrap();
            rt.service_refresh(Svc::Embedding, true).await;
            rt.stop_ours_on_exit();
            let t = Instant::now();
            while rt.probe(&Svc::Embedding.root(), false).await != Probe::Offline && t.elapsed() < Duration::from_secs(20) {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            assert_eq!(rt.probe(&Svc::Embedding.root(), false).await, Probe::Offline);
        });
    }

    #[test]
    fn services_pomelo_started_stop_when_idle() {
        tauri::async_runtime::block_on(async {
            let rt = Runtime::new(Mode::Mock, LlmPaths::new(Path::new("/nonexistent-home")));
            let now = Instant::now();
            assert!(rt.idle_services(now + IDLE * 2).is_empty()); // nothing running
            rt.ensure(Svc::Embedding).await.unwrap();
            rt.ensure(Svc::Rerank).await.unwrap();
            let later = Instant::now() + IDLE + Duration::from_secs(1);
            assert!(rt.idle_services(Instant::now()).is_empty());
            assert_eq!(rt.idle_services(later), vec![Svc::Embedding, Svc::Rerank]);
            // a later use restarts the countdown
            rt.svc(Svc::Rerank).last_used = later;
            assert_eq!(rt.idle_services(later), vec![Svc::Embedding]);
            // one Pomelo didn't start is never stopped
            rt.svc(Svc::Embedding).script = Some(ServiceScript { running: Some("mock-embedding".into()), ours: false });
            assert!(rt.idle_services(later + IDLE * 4).iter().all(|s| *s != Svc::Embedding));
        });
    }
}
