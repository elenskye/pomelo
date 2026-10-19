//! One "send messages, stream the reply" interface over three backends:
//! - local llama-server (127.0.0.1:8080, OpenAI-compatible; never through a proxy),
//! - cloud: OpenRouter / DeepSeek / OpenAI at their fixed official HTTPS endpoints (bearer key,
//!   redirects refused so a key is never forwarded; system proxy allowed),
//! - mock: canned replies (dev builds / tests), no network.
//! SSE framing is byte-based (split UTF-8 safe), bounded (4 MB per frame). Server error text is
//! never shown or stored for cloud services — only fixed Chinese messages.
//! Tools (v0.3.5, picky/data.rs): OpenAI-style `tools` on the request; streamed `tool_calls` deltas
//! are put together and arrive as one `Event::ToolCalls` before the finish. DeepSeek (thinking on by
//! default) wants every assistant message's `reasoning_content` back when tools are on (else 400).
//! Thinking (`Options::thinking`, what the work wants): a cloud model set as 能开关 gets the switch
//! (DeepSeek `thinking`, OpenRouter `reasoning.enabled`, OpenAI `reasoning_effort: none` when off);
//! 不能 / 只能思考 ones get none. One that turns the switch down anyway (400) is asked again without it,
//! remembered for the run of the app. Local: the template's `enable_thinking`.
//! OpenRouter: the reasoning blocks (`reasoning_details`: Gemini's thought signatures, Claude's signed
//! thinking) of a response that called tools go back unchanged with its assistant message (else the
//! next round is refused).
//! Google (OpenAI-compatible endpoint): a tool call's `extra_content` (Gemini's thought signature)
//! goes back on that call; `reasoning_effort: none` when off; a stream may end without [DONE].
//! Anthropic (Messages API, `anthropic_body` / `Flavor::Anthropic`): leading system messages → `system`, tool
//! results → `tool_result` blocks. A response's whole content (signed thinking, text, tool calls, in
//! order) goes back verbatim within the run's tool rounds; earlier turns carry no thinking (they are
//! rebuilt from saved text). Thinking: adaptive with summaries and `drop_block` (a later round's
//! changed tool list must not turn the request down), `disabled` when off.

use super::cloud::{CloudModel, Service, Think};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Msg {
    pub role: String,
    pub content: String,
    /// Assistant messages that asked for tools.
    pub tool_calls: Vec<ToolCall>,
    /// `tool` messages: the call they answer.
    pub tool_call_id: Option<String>,
    /// Assistant messages in a tool exchange: their reasoning (sent back to DeepSeek only).
    pub reasoning: Option<String>,
    /// Assistant messages in a tool exchange: OpenRouter's reasoning blocks, Google's per-call extras
    /// (`EXTRA` entries) or Anthropic's whole content — each sent back to its own service only.
    pub reasoning_details: Vec<Value>,
    /// User messages: images sent with the text (cloud models only; chat.rs decides).
    pub images: Vec<Image>,
}

/// An image as sent: its media type and the bytes in base64.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Image {
    pub media_type: String,
    pub data: String,
}

/// A `reasoning_details` entry holding a Google tool call's `extra_content` (by the call's position).
const EXTRA: &str = "pomelo.tool_call_extra";

/// Per-service extras on the wire.
#[derive(Clone, Copy, Default)]
struct Wire {
    /// DeepSeek with tools: `reasoning_content` on assistant messages.
    reasoning: bool,
    /// OpenRouter: `reasoning_details` back, and no empty text beside tool calls.
    details: bool,
    /// Google: each tool call's `extra_content` back on it.
    extras: bool,
}

impl Msg {
    pub fn new(role: &str, content: &str) -> Self {
        Msg { role: role.into(), content: content.into(), ..Default::default() }
    }
    pub fn tool(call_id: &str, content: &str) -> Self {
        Msg { role: "tool".into(), content: content.into(), tool_call_id: Some(call_id.into()), ..Default::default() }
    }

    /// The message as sent.
    fn wire(&self, w: Wire) -> Value {
        let mut v = json!({ "role": self.role, "content": self.content });
        if self.role == "user" && !self.images.is_empty() {
            // OpenAI-style parts (OpenAI, OpenRouter, Google's compatible endpoint): the text, then data URLs
            let mut parts = vec![json!({ "type": "text", "text": self.content })];
            parts.extend(self.images.iter().map(|i| json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{}", i.media_type, i.data) } })));
            v["content"] = json!(parts);
        }
        if !self.tool_calls.is_empty() {
            v["tool_calls"] = self.tool_calls.iter().map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } })).collect();
            if w.details && self.content.is_empty() {
                v["content"] = Value::Null;
            }
        }
        if let Some(id) = &self.tool_call_id {
            v["tool_call_id"] = json!(id);
        }
        if w.reasoning && self.role == "assistant" {
            v["reasoning_content"] = json!(self.reasoning.as_deref().unwrap_or(""));
        }
        let own: Vec<&Value> = self.reasoning_details.iter().filter(|d| d["type"] != EXTRA).collect();
        if w.details && self.role == "assistant" && !own.is_empty() {
            v["reasoning_details"] = json!(own);
        }
        if w.extras {
            for d in self.reasoning_details.iter().filter(|d| d["type"] == EXTRA) {
                if let Some(call) = d["index"].as_u64().and_then(|i| v["tool_calls"].get_mut(i as usize)) {
                    call["extra_content"] = d["extra_content"].clone();
                }
            }
        }
        v
    }
}

/// One tool call the model asked for (`arguments`: JSON text, as the model wrote it).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Reasoning(String),
    Content(String),
    /// The calls of this response, complete (before `Finished`).
    ToolCalls(Vec<ToolCall>),
    /// OpenRouter: this response's reasoning blocks, put together (before its tool calls).
    ReasoningDetails(Vec<Value>),
    /// stop | length | content_filter | tool_calls
    Finished(String),
    Usage { prompt: i64, completion: i64 },
    /// true = waiting: for the local server (it is answering another request), or before asking a
    /// cloud service again (a rate limit, a moment's outage); false = going on now.
    Queued(bool),
}

#[derive(Clone)]
pub enum Target {
    /// `model`: the name the running server serves.
    Local { model: String },
    Cloud { model: CloudModel, key: String },
    Mock { thinking: bool },
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Reply cap; None = the model's default (cloud: the model's max output).
    pub max_tokens: Option<i64>,
    /// None keeps the model's default.
    pub thinking: Option<bool>,
    /// Tool definitions (OpenAI `tools` entries); empty = none.
    pub tools: Vec<Value>,
    /// With tools: forbid calls this time (the last round must answer).
    pub no_calls: bool,
}

const LOCAL_URL: &str = "http://127.0.0.1:8080/v1/chat/completions";
const LOCAL_ROOT: &str = "http://127.0.0.1:8080";
const FRAME_LIMIT: usize = 4_000_000;

pub mod failure {
    pub const LOCAL_CONNECT: &str = "连不上本地模型，请确认它已经启动。";
    pub const LOCAL_CUT: &str = "模型回复意外中断。";
    pub const TOO_LARGE: &str = "模型响应的一段数据过大。";
    pub const UNAUTHORIZED: &str = "云端接口认证失败，请检查 API Key。";
    pub const CREDITS: &str = "云端接口余额不足或无权使用此模型。";
    pub const LIMITED: &str = "云端接口请求过于频繁或额度已用尽。";
    /// OpenRouter: the model's provider limits everyone's requests together.
    pub const UPSTREAM_LIMITED: &str = "这个模型在 OpenRouter 上被限流了（它的供应商限的是所有人加起来的请求）：请过一会儿再试，或者换个模型。";
    pub const REQUEST: &str = "云端接口拒绝了请求，请检查模型名和令牌上限。";
    pub const UNAVAILABLE: &str = "云端模型暂时不可用，请稍后重试。";
    pub const NETWORK: &str = "无法连接云端接口，请检查网络。";
    pub const INTERRUPTED: &str = "云端回复意外中断。";
    pub const RESPONSE: &str = "云端接口返回了无法处理的回复。";
}

/// The service turned the request itself down (e.g. a model or server that takes no tools).
pub fn rejected(e: &str) -> bool {
    e == failure::REQUEST || e.starts_with("本地模型接口返回 HTTP 4") || e.starts_with("本地模型接口返回 HTTP 5")
}

/// A failure worth asking again (rate limits, a service down for a moment, a dropped connection).
fn transient(e: &str) -> bool {
    [failure::LIMITED, failure::UNAVAILABLE, failure::NETWORK].contains(&e)
}

/// How long to wait before asking again: `Retry-After` when given (at most 20 s), else 2 / 5 / 10 s.
fn backoff(attempt: usize, retry_after: Option<u64>) -> Duration {
    Duration::from_secs(retry_after.map_or([2, 5, 10][attempt.min(2)], |s| s.clamp(1, 20)))
}

/// Cloud requests asked again when they fail before anything was shown.
const RETRIES: usize = 3;

fn http_failure(status: u16) -> &'static str {
    match status {
        401 => failure::UNAUTHORIZED,
        402 | 403 => failure::CREDITS,
        429 => failure::LIMITED,
        400 | 404 | 413 | 422 => failure::REQUEST,
        500..=599 => failure::UNAVAILABLE,
        _ => failure::RESPONSE,
    }
}

// ---------- Anthropic ----------

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// `thinking.block_binding` (keep going without the blocks a changed request no longer fits).
const BETA_BINDING: &str = "thinking-binding-controls-2026-08-01";
/// `fallbacks: "default"`: a classifier decline is answered by the model Anthropic picks.
const BETA_FALLBACK: &str = "server-side-fallback-2026-07-01";
/// Models documented to take `fallbacks: "default"` on the Claude API.
const FALLBACK_MODELS: &[&str] = &["claude-fable-5-1", "claude-fable-5", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"];

/// Thinking is on for this request (a 能开关 model unless asked not to; None = on, as the newer
/// models' own default and as a model that turned `disabled` down needs).
fn anthropic_thinks(model: &CloudModel, opts: &Options) -> bool {
    match model.thinking {
        Think::Never => false,
        Think::Always => true,
        Think::Switch => opts.thinking != Some(false),
    }
}

/// The `anthropic-beta` header the body needs (empty = none).
fn anthropic_betas(model: &CloudModel, opts: &Options) -> Vec<&'static str> {
    let mut b = vec![];
    if anthropic_thinks(model, opts) {
        b.push(BETA_BINDING);
    }
    if FALLBACK_MODELS.contains(&model.api_model.as_str()) {
        b.push(BETA_FALLBACK);
    }
    b
}

/// An assistant message's content blocks: the response's own blocks when they hold its calls,
/// else rebuilt (text, then tool_use).
fn anthropic_assistant(m: &Msg) -> Vec<Value> {
    let own = !m.reasoning_details.is_empty()
        && m.reasoning_details.iter().all(|b| b["type"].is_string() && b["type"] != EXTRA)
        && m.tool_calls.iter().all(|c| m.reasoning_details.iter().any(|b| b["type"] == "tool_use" && b["id"] == c.id.as_str()));
    if own {
        return m.reasoning_details.clone();
    }
    let mut blocks = vec![];
    if !m.content.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": m.content }));
    }
    for c in &m.tool_calls {
        let input = serde_json::from_str::<Value>(&c.arguments).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
        blocks.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }));
    }
    blocks
}

fn anthropic_body(model: &CloudModel, messages: &[Msg], opts: &Options, max: i64) -> Value {
    let lead = messages.iter().take_while(|m| m.role == "system").count();
    let system: Vec<&str> = messages[..lead].iter().map(|m| m.content.as_str()).filter(|s| !s.trim().is_empty()).collect();
    let mut out: Vec<Value> = vec![];
    let mut results: Vec<Value> = vec![];
    let flush = |out: &mut Vec<Value>, results: &mut Vec<Value>| {
        if !results.is_empty() {
            out.push(json!({ "role": "user", "content": std::mem::take(results) }));
        }
    };
    for m in &messages[lead..] {
        if m.role == "tool" {
            let mut r = json!({ "type": "tool_result", "tool_use_id": m.tool_call_id.as_deref().unwrap_or("") });
            if !m.content.is_empty() {
                r["content"] = json!(m.content);
            }
            results.push(r);
            continue;
        }
        // images looked at during a tool exchange go with its results (one user turn)
        if m.role == "user" && !m.images.is_empty() && !results.is_empty() {
            results.extend(m.images.iter().map(|i| json!({ "type": "image", "source": { "type": "base64", "media_type": i.media_type, "data": i.data } })));
            results.push(json!({ "type": "text", "text": m.content }));
            continue;
        }
        flush(&mut out, &mut results);
        if m.role == "assistant" {
            let blocks = anthropic_assistant(m);
            // the conversation starts with the user (a history cut can leave a reply first)
            if !blocks.is_empty() && !out.is_empty() {
                out.push(json!({ "role": "assistant", "content": blocks }));
            }
        } else if !m.images.is_empty() {
            let mut blocks: Vec<Value> = m.images.iter().map(|i| json!({ "type": "image", "source": { "type": "base64", "media_type": i.media_type, "data": i.data } })).collect();
            blocks.push(json!({ "type": "text", "text": m.content }));
            out.push(json!({ "role": "user", "content": blocks }));
        } else if !m.content.trim().is_empty() {
            out.push(json!({ "role": "user", "content": m.content }));
        }
    }
    flush(&mut out, &mut results);
    let mut b = json!({ "model": model.api_model, "max_tokens": max, "messages": out, "stream": true, "cache_control": { "type": "ephemeral" } });
    if !system.is_empty() {
        b["system"] = json!(system.join("\n\n"));
    }
    if anthropic_thinks(model, opts) {
        b["thinking"] = json!({ "type": "adaptive", "display": "summarized", "block_binding": { "prefix_mismatch_behavior": "drop_block" } });
    } else if model.thinking == Think::Switch {
        b["thinking"] = json!({ "type": "disabled" });
    }
    if FALLBACK_MODELS.contains(&model.api_model.as_str()) {
        b["fallbacks"] = json!("default");
    }
    if !opts.tools.is_empty() {
        let tools: Vec<Value> = opts
            .tools
            .iter()
            .map(|t| {
                let f = &t["function"];
                let mut tool = json!({ "name": f["name"], "input_schema": f.get("parameters").filter(|p| p.is_object()).cloned().unwrap_or_else(|| json!({ "type": "object", "properties": {} })) });
                if let Some(d) = f["description"].as_str() {
                    tool["description"] = json!(d);
                }
                tool
            })
            .collect();
        b["tools"] = json!(tools);
        if opts.no_calls {
            b["tool_choice"] = json!({ "type": "none" });
        }
    }
    b
}

/// The JSON body for a request (exposed for tests: the wire contract without any secret).
pub fn body(target: &Target, messages: &[Msg], opts: &Options) -> Value {
    if let Target::Cloud { model, .. } = target {
        if model.service == Service::Anthropic {
            let max = opts.max_tokens.unwrap_or(model.max_output_tokens).clamp(1, model.max_output_tokens);
            return anthropic_body(model, messages, opts, max);
        }
    }
    let service = |s: Service| matches!(target, Target::Cloud { model, .. } if model.service == s);
    let w = Wire { reasoning: !opts.tools.is_empty() && service(Service::DeepSeek), details: service(Service::OpenRouter), extras: service(Service::Google) };
    let messages: Vec<Value> = messages.iter().map(|m| m.wire(w)).collect();
    let mut b = match target {
        Target::Local { model } => {
            let mut b = json!({ "model": model, "messages": messages, "stream": true, "stream_options": { "include_usage": true } });
            if let Some(n) = opts.max_tokens {
                b["max_tokens"] = json!(n);
            }
            if let Some(t) = opts.thinking {
                b["chat_template_kwargs"] = json!({ "enable_thinking": t });
            }
            b
        }
        Target::Cloud { model, .. } => {
            let max = opts.max_tokens.unwrap_or(model.max_output_tokens).clamp(1, model.max_output_tokens);
            // only a model that can switch gets the switch (one that can't may turn the request down)
            let thinking = if model.thinking == Think::Switch { opts.thinking } else { None };
            let mut b = json!({ "model": model.api_model, "messages": messages, "stream": true, "stream_options": { "include_usage": true } });
            match model.service {
                Service::OpenAi => {
                    b["max_completion_tokens"] = json!(max);
                    b["store"] = json!(false);
                    // on: the model's own effort
                    if thinking == Some(false) {
                        b["reasoning_effort"] = json!("none");
                    }
                }
                Service::DeepSeek => {
                    b["max_tokens"] = json!(max);
                    if let Some(t) = thinking {
                        b["thinking"] = json!({ "type": if t { "enabled" } else { "disabled" } });
                    }
                }
                Service::OpenRouter => {
                    b["max_tokens"] = json!(max);
                    if let Some(t) = thinking {
                        b["reasoning"] = json!({ "enabled": t });
                    }
                }
                Service::Google => {
                    b["max_tokens"] = json!(max);
                    // on: the model's own level (Gemini 3 can't think less than its least)
                    if thinking == Some(false) {
                        b["reasoning_effort"] = json!("none");
                    }
                }
                Service::Anthropic => unreachable!("anthropic_body"),
            }
            b
        }
        Target::Mock { .. } => return Value::Null,
    };
    if !opts.tools.is_empty() {
        b["tools"] = json!(opts.tools);
        if opts.no_calls {
            b["tool_choice"] = json!("none");
        }
    }
    b
}

/// The local server answers one request at a time (llmctl `--parallel 1`: each keeps the whole
/// context), so requests to it take turns here, whoever sends them (conversations, 定时任务,
/// 简报, 助手, 润色补全…). Cloud models answer side by side.
static LOCAL_TURN: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

/// Waits for the local server's turn, telling `on` when it has to wait.
async fn local_turn(on: &mut impl FnMut(Event)) -> tokio::sync::SemaphorePermit<'static> {
    let turn = LOCAL_TURN.get_or_init(|| tokio::sync::Semaphore::new(1));
    if let Ok(p) = turn.try_acquire() {
        return p;
    }
    on(Event::Queued(true));
    let p = turn.acquire().await.expect("the turn is never closed");
    on(Event::Queued(false));
    p
}

/// Streams one reply, calling `on` for every event in arrival order. Ok(()) = the stream ended
/// properly ([DONE]; cloud also needs a finish reason). Cancel by dropping / aborting the task.
pub async fn stream(target: Target, messages: Vec<Msg>, opts: Options, mut on: impl FnMut(Event) + Send) -> Result<(), String> {
    let _turn = match &target {
        Target::Local { .. } => Some(local_turn(&mut on).await),
        _ => None,
    };
    match &target {
        Target::Mock { thinking } => mock_stream(*thinking && opts.thinking != Some(false), &messages, &opts, &mut on).await,
        Target::Local { .. } => {
            let client = reqwest::Client::builder()
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(Duration::from_secs(600))
                .build()
                .map_err(|e| e.to_string())?;
            let resp = client
                .post(LOCAL_URL)
                .header("Accept", "text/event-stream")
                .json(&body(&target, &messages, &opts))
                .send()
                .await
                .map_err(|_| failure::LOCAL_CONNECT.to_string())?;
            if resp.status() != 200 {
                return Err(format!("本地模型接口返回 HTTP {}", resp.status().as_u16()));
            }
            read_sse(resp, Parser::new(Flavor::Local), &mut on).await
        }
        Target::Cloud { model, key } => {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(600))
                .timeout(Duration::from_secs(1200))
                .build()
                .map_err(|e| e.to_string())?;
            let send = |opts: &Options| {
                let req = if model.service == Service::Anthropic {
                    let betas = anthropic_betas(model, opts);
                    let req = client.post(format!("{}/messages", model.service.base_url())).header("x-api-key", key.as_str()).header("anthropic-version", ANTHROPIC_VERSION);
                    if betas.is_empty() { req } else { req.header("anthropic-beta", betas.join(",")) }
                } else {
                    client.post(format!("{}/chat/completions", model.service.base_url())).bearer_auth(key)
                };
                req.header("Accept", "text/event-stream").json(&body(&target, &messages, opts)).send()
            };
            let mut opts = opts;
            if fixed_reasoning(&model.api_model) {
                opts.thinking = None;
            }
            // a rate limit, a moment's outage or a dropped connection before anything was shown is
            // asked again (OpenRouter's upstream providers limit bursts: a tool exchange sends several
            // requests in a row)
            let mut attempt = 0;
            loop {
                let mut shown = false;
                let mut retry_after = None;
                let result = async {
                    let mut resp = send(&opts).await.map_err(|_| failure::NETWORK.to_string())?;
                    if resp.status() == 400 && switch_sent(model, &opts) {
                        // reasoning that can't be switched: again with the model's own setting
                        opts.thinking = None;
                        resp = send(&opts).await.map_err(|_| failure::NETWORK.to_string())?;
                        if resp.status() == 200 {
                            remember_fixed_reasoning(&model.api_model);
                        }
                    }
                    if resp.status() != 200 {
                        let status = resp.status().as_u16();
                        retry_after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
                        if cfg!(test) {
                            eprintln!("[cloud] HTTP {status}: {}", resp.text().await.unwrap_or_default().chars().take(2000).collect::<String>());
                        }
                        return Err(http_failure(status).to_string());
                    }
                    let sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.to_lowercase().starts_with("text/event-stream"));
                    if !sse {
                        return Err(failure::RESPONSE.to_string());
                    }
                    let parser = match model.service {
                        Service::Anthropic => Parser::new(Flavor::Anthropic),
                        Service::Google => Parser::new(Flavor::Cloud).google(),
                        _ => Parser::new(Flavor::Cloud),
                    };
                    let mut watch = |e: Event| {
                        shown |= matches!(e, Event::Reasoning(_) | Event::Content(_) | Event::ToolCalls(_));
                        on(e)
                    };
                    read_sse(resp, parser, &mut watch).await
                }
                .await;
                match result {
                    Err(e) if !shown && attempt < RETRIES && transient(&e) => {
                        if cfg!(test) {
                            eprintln!("[cloud] {e} — asking again ({})", attempt + 1);
                        }
                        on(Event::Queued(true));
                        tokio::time::sleep(backoff(attempt, retry_after)).await;
                        on(Event::Queued(false));
                        attempt += 1;
                    }
                    Err(e) if e == failure::LIMITED && model.service == Service::OpenRouter => return Err(failure::UPSTREAM_LIMITED.into()),
                    r => return r,
                }
            }
        }
    }
}

/// The body carries a thinking switch a model set as 能开关 may still turn down (OpenRouter's
/// `reasoning`, OpenAI's / Google's `reasoning_effort`, Anthropic's `disabled`): a 400 is worth
/// asking again without it.
fn switch_sent(model: &CloudModel, opts: &Options) -> bool {
    model.thinking == Think::Switch
        && match model.service {
            Service::OpenRouter => opts.thinking.is_some(),
            Service::OpenAi | Service::Google | Service::Anthropic => opts.thinking == Some(false),
            Service::DeepSeek => false,
        }
}

/// Cloud models that turned down a request for its thinking switch (this run of the app).
static FIXED_REASONING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn fixed_reasoning(api_model: &str) -> bool {
    FIXED_REASONING.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|m| m == api_model)
}

fn remember_fixed_reasoning(api_model: &str) {
    let mut l = FIXED_REASONING.lock().unwrap_or_else(|e| e.into_inner());
    if !l.iter().any(|m| m == api_model) {
        l.push(api_model.to_string());
    }
}

fn local_client(timeout: u64) -> Option<reqwest::Client> {
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(timeout)).build().ok()
}

/// The running local server's context window (per slot), if it says.
pub async fn local_window() -> Option<i64> {
    let v: Value = local_client(3)?.get(format!("{LOCAL_ROOT}/props")).send().await.ok().filter(|r| r.status() == 200)?.json().await.ok()?;
    v["default_generation_settings"]["n_ctx"].as_i64().or_else(|| v["n_ctx"].as_i64()).filter(|n| *n > 0)
}

/// The exact size of `text` for the loaded local model, if the server can count.
pub async fn local_tokens(text: &str) -> Option<i64> {
    let body = json!({ "content": text, "add_special": false });
    let v: Value = local_client(10)?.post(format!("{LOCAL_ROOT}/tokenize")).json(&body).send().await.ok().filter(|r| r.status() == 200)?.json().await.ok()?;
    v["tokens"].as_array().map(|t| t.len() as i64)
}

async fn read_sse(mut resp: reqwest::Response, mut p: Parser, on: &mut (impl FnMut(Event) + Send)) -> Result<(), String> {
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(_) => return Err(p.flavor.cut().into()),
        };
        for e in p.push(&chunk)? {
            on(e);
        }
        if p.done {
            break;
        }
    }
    for e in p.finish()? {
        on(e);
    }
    p.verdict()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Local,
    /// OpenAI-style chat completion chunks.
    Cloud,
    /// Messages API events (message_start … message_stop).
    Anthropic,
}

impl Flavor {
    fn cut(self) -> &'static str {
        match self {
            Flavor::Local => failure::LOCAL_CUT,
            Flavor::Cloud | Flavor::Anthropic => failure::INTERRUPTED,
        }
    }
}

/// An Anthropic content block being streamed.
struct Block {
    kind: String,
    /// text / thinking so far; tool_use: the input JSON so far.
    text: String,
    signature: String,
    /// The block as it started (id, name, redacted data…).
    start: Value,
}

/// SSE → events. Lines are split on bytes (a newline never sits inside a UTF-8 sequence); a blank
/// line ends an event; `data:` lines join with "\n"; comments and other fields are ignored.
pub struct Parser {
    flavor: Flavor,
    line: Vec<u8>,
    payload: Vec<String>,
    payload_bytes: usize,
    pub done: bool,
    saw_finish: bool,
    /// Tool calls being streamed, by their `index`.
    calls: Vec<ToolCall>,
    /// OpenRouter's reasoning blocks so far (fragments of one block joined) and their size;
    /// Anthropic: the response's finished content blocks, in order.
    details: Vec<Value>,
    details_bytes: usize,
    /// Google: each call's `extra_content` (by `index`); its stream may end without [DONE].
    google: bool,
    extras: Vec<Option<Value>>,
    /// Anthropic: the block being streamed, the prompt size from message_start, and whether text /
    /// thinking was already shown (a later block starts on a new paragraph).
    block: Option<Block>,
    prompt: i64,
    shown: (bool, bool),
}

impl Parser {
    pub fn new(flavor: Flavor) -> Self {
        Parser {
            flavor,
            line: vec![],
            payload: vec![],
            payload_bytes: 0,
            done: false,
            saw_finish: false,
            calls: vec![],
            details: vec![],
            details_bytes: 0,
            google: false,
            extras: vec![],
            block: None,
            prompt: 0,
            shown: (false, false),
        }
    }

    pub fn google(mut self) -> Self {
        self.google = true;
        self
    }

    /// The reasoning blocks, then the finished tool calls, once (unnamed fragments dropped).
    fn take_calls(&mut self) -> Vec<Event> {
        let mut out = vec![];
        let extras = std::mem::take(&mut self.extras);
        let mut calls: Vec<ToolCall> = vec![];
        for (i, c) in std::mem::take(&mut self.calls).into_iter().enumerate() {
            if c.name.is_empty() {
                continue;
            }
            if let Some(x) = extras.get(i).cloned().flatten() {
                self.details.push(json!({ "type": EXTRA, "index": calls.len(), "extra_content": x }));
            }
            calls.push(c);
        }
        if !self.details.is_empty() {
            out.push(Event::ReasoningDetails(std::mem::take(&mut self.details)));
        }
        if !calls.is_empty() {
            out.push(Event::ToolCalls(calls));
        }
        out
    }

    /// A streamed piece of a reasoning block: the next piece of the same block (same type and index,
    /// no other id) joins its text; anything else starts a block.
    fn add_detail(&mut self, d: &Value) -> Result<(), String> {
        self.details_bytes += d.to_string().len();
        if self.details_bytes > FRAME_LIMIT {
            return Err(failure::TOO_LARGE.into());
        }
        if let Some(last) = self.details.last_mut() {
            let same = d.get("index").is_some_and(|i| !i.is_null()) && last.get("index") == d.get("index") && last.get("type") == d.get("type") && (d["id"].is_null() || last["id"] == d["id"]);
            if same {
                for (k, v) in d.as_object().into_iter().flatten() {
                    match (k.as_str(), v.as_str()) {
                        ("text" | "summary" | "data", Some(piece)) => last[k] = json!(format!("{}{piece}", last[k].as_str().unwrap_or(""))),
                        _ if !v.is_null() => last[k] = v.clone(),
                        _ => {}
                    }
                }
                return Ok(());
            }
        }
        self.details.push(d.clone());
        Ok(())
    }

    fn add_call_delta(&mut self, d: &Value) -> Result<(), String> {
        let i = d["index"].as_u64().map_or(self.calls.len().saturating_sub(usize::from(d.get("id").is_none())), |i| i as usize);
        if i > 64 {
            return Err(failure::TOO_LARGE.into());
        }
        if self.calls.len() <= i {
            self.calls.resize(i + 1, ToolCall::default());
        }
        if self.google {
            if let Some(x) = d.get("extra_content").filter(|x| x.is_object()) {
                if self.extras.len() <= i {
                    self.extras.resize(i + 1, None);
                }
                self.extras[i] = Some(x.clone());
            }
        }
        let c = &mut self.calls[i];
        if let Some(id) = d["id"].as_str().filter(|s| !s.is_empty()) {
            c.id = id.into();
        }
        if let Some(n) = d["function"]["name"].as_str() {
            c.name.push_str(n);
        }
        if let Some(a) = d["function"]["arguments"].as_str() {
            c.arguments.push_str(a);
        }
        Ok(())
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, String> {
        let mut out = vec![];
        for &b in bytes {
            if self.done {
                break;
            }
            if b != b'\n' {
                self.line.push(b);
                if self.line.len() > FRAME_LIMIT {
                    return Err(failure::TOO_LARGE.into());
                }
                continue;
            }
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
            let text = String::from_utf8_lossy(&self.line).into_owned();
            self.line.clear();
            if text.is_empty() {
                out.extend(self.emit()?);
            } else {
                self.read(&text)?;
            }
        }
        Ok(out)
    }

    /// End of body: a last unterminated line / event still counts.
    pub fn finish(&mut self) -> Result<Vec<Event>, String> {
        if self.done {
            return Ok(vec![]);
        }
        if !self.line.is_empty() {
            if self.line.last() == Some(&b'\r') {
                self.line.pop();
            }
            let text = String::from_utf8_lossy(&self.line).into_owned();
            self.line.clear();
            self.read(&text)?;
        }
        let mut out = self.emit()?;
        out.extend(self.take_calls());
        Ok(out)
    }

    pub fn verdict(&self) -> Result<(), String> {
        match self.flavor {
            Flavor::Local if self.done => Ok(()),
            Flavor::Cloud | Flavor::Anthropic if (self.done || self.google) && self.saw_finish => Ok(()),
            f => Err(f.cut().into()),
        }
    }

    fn read(&mut self, text: &str) -> Result<(), String> {
        let Some(data) = text.strip_prefix("data:") else { return Ok(()) };
        let data = data.strip_prefix(' ').unwrap_or(data);
        self.payload_bytes += data.len();
        if self.payload_bytes > FRAME_LIMIT {
            return Err(failure::TOO_LARGE.into());
        }
        self.payload.push(data.trim_end().to_string());
        Ok(())
    }

    fn emit(&mut self) -> Result<Vec<Event>, String> {
        if self.payload.is_empty() {
            return Ok(vec![]);
        }
        let text = self.payload.join("\n");
        self.payload.clear();
        self.payload_bytes = 0;
        if text.trim() == "[DONE]" {
            self.done = true;
            return Ok(self.take_calls());
        }
        let cloud = self.flavor != Flavor::Local;
        let v: Value = serde_json::from_str(&text).map_err(|_| if cloud { failure::RESPONSE.to_string() } else { "本地模型返回了无法处理的数据。".to_string() })?;
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            if cfg!(test) && cloud {
                eprintln!("[cloud] error in the stream: {}", text.chars().take(2000).collect::<String>());
            }
            // OpenRouter passes its upstream provider's status on (429: a burst limit)
            let code = e["code"].as_u64().or_else(|| e["code"].as_str().and_then(|c| c.parse().ok())).map_or(500, |c| c as u16);
            return Err(if cloud { http_failure(code).into() } else { format!("本地模型报错：{}", e["message"].as_str().unwrap_or("未知错误")) });
        }
        if self.flavor == Flavor::Anthropic {
            return self.anthropic(&v);
        }
        let mut out = vec![];
        for c in v["choices"].as_array().into_iter().flatten() {
            if cloud && c.get("index").and_then(Value::as_i64).is_some_and(|i| i != 0) {
                continue;
            }
            let reason = c["finish_reason"].as_str();
            if cloud && reason.is_some_and(|r| ["error", "aborted", "insufficient_system_resource"].contains(&r)) {
                return Err(failure::INTERRUPTED.into());
            }
            let d = &c["delta"];
            let reasoning = d["reasoning_content"].as_str().or(if cloud { d["reasoning"].as_str() } else { None });
            if let Some(r) = reasoning.filter(|s| !s.is_empty()) {
                out.push(Event::Reasoning(r.into()));
            }
            if let Some(t) = d["content"].as_str().filter(|s| !s.is_empty()) {
                out.push(Event::Content(t.into()));
            }
            for call in d["tool_calls"].as_array().into_iter().flatten() {
                self.add_call_delta(call)?;
            }
            if cloud {
                for detail in d["reasoning_details"].as_array().into_iter().flatten() {
                    self.add_detail(detail)?;
                }
            }
            if let Some(r) = reason {
                if cloud {
                    if self.saw_finish {
                        continue;
                    }
                    // fixed reasons only: arbitrary server text never reaches stored data
                    if !["stop", "length", "content_filter", "tool_calls"].contains(&r) {
                        return Err(failure::RESPONSE.into());
                    }
                    self.saw_finish = true;
                }
                out.extend(self.take_calls());
                out.push(Event::Finished(r.into()));
            }
        }
        let usage = (v["usage"]["prompt_tokens"].as_i64(), v["usage"]["completion_tokens"].as_i64());
        let timings = (v["timings"]["prompt_n"].as_i64(), v["timings"]["predicted_n"].as_i64());
        let pair = match (usage, cloud) {
            ((Some(p), Some(c)), _) => Some((p, c)),
            (_, false) => match timings {
                (Some(p), Some(c)) => Some((p, c)),
                _ => None,
            },
            _ => None,
        };
        if let Some((p, c)) = pair.filter(|(p, c)| *p >= 0 && *c >= 0) {
            out.push(Event::Usage { prompt: p, completion: c });
        }
        Ok(out)
    }

    /// One Messages API event. Finished blocks collect in `details` (what goes back verbatim);
    /// a `fallback` block (another model took over mid-reply) leaves only the text before it.
    fn anthropic(&mut self, v: &Value) -> Result<Vec<Event>, String> {
        let mut out = vec![];
        let input = |u: &Value| ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"].iter().filter_map(|k| u[*k].as_i64()).sum::<i64>();
        match v["type"].as_str().unwrap_or("") {
            "message_start" => self.prompt = input(&v["message"]["usage"]),
            "content_block_start" => {
                let b = &v["content_block"];
                let kind = b["type"].as_str().unwrap_or("").to_string();
                let text = b[if kind == "thinking" { "thinking" } else { "text" }].as_str().unwrap_or("").to_string();
                self.block = Some(Block { kind, text: String::new(), signature: b["signature"].as_str().unwrap_or("").into(), start: b.clone() });
                if !text.is_empty() {
                    out.extend(self.anthropic_piece(&text));
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => out.extend(self.anthropic_piece(d["text"].as_str().unwrap_or(""))),
                    "thinking_delta" => out.extend(self.anthropic_piece(d["thinking"].as_str().unwrap_or(""))),
                    "signature_delta" => {
                        if let Some(b) = self.block.as_mut() {
                            b.signature.push_str(d["signature"].as_str().unwrap_or(""));
                        }
                    }
                    "input_json_delta" => {
                        if let Some(b) = self.block.as_mut() {
                            b.text.push_str(d["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(b) = self.block.take() else { return Ok(out) };
                let done = match b.kind.as_str() {
                    "text" if !b.text.is_empty() => Some(json!({ "type": "text", "text": b.text })),
                    "thinking" => Some(json!({ "type": "thinking", "thinking": b.text, "signature": b.signature })),
                    "redacted_thinking" => Some(b.start),
                    "tool_use" => {
                        let arguments = if b.text.trim().is_empty() { "{}".to_string() } else { b.text };
                        let input = serde_json::from_str::<Value>(&arguments).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
                        self.calls.push(ToolCall { id: b.start["id"].as_str().unwrap_or("").into(), name: b.start["name"].as_str().unwrap_or("").into(), arguments });
                        Some(json!({ "type": "tool_use", "id": b.start["id"], "name": b.start["name"], "input": input }))
                    }
                    "fallback" => {
                        self.details.retain(|d| d["type"] == "text");
                        self.calls.clear();
                        None
                    }
                    _ => None,
                };
                if let Some(d) = done {
                    self.details_bytes += d.to_string().len();
                    if self.details_bytes > FRAME_LIMIT {
                        return Err(failure::TOO_LARGE.into());
                    }
                    self.details.push(d);
                }
            }
            "message_delta" => {
                let u = &v["usage"];
                if input(u) > 0 {
                    self.prompt = input(u);
                }
                if let Some(r) = v["delta"]["stop_reason"].as_str() {
                    if !self.saw_finish {
                        self.saw_finish = true;
                        // fixed reasons only: arbitrary server text never reaches stored data
                        let reason = match r {
                            "max_tokens" | "model_context_window_exceeded" => "length",
                            "tool_use" => "tool_calls",
                            "refusal" => "content_filter",
                            _ => "stop",
                        };
                        out.extend(self.take_calls());
                        out.push(Event::Finished(reason.into()));
                    }
                }
                if let Some(c) = u["output_tokens"].as_i64().filter(|c| *c >= 0) {
                    out.push(Event::Usage { prompt: self.prompt, completion: c });
                }
            }
            "message_stop" => self.done = true,
            _ => {} // ping and newer events
        }
        Ok(out)
    }

    /// Streamed text or thinking: shown as it comes (a later block of the same kind starts a new
    /// paragraph), kept for the block.
    fn anthropic_piece(&mut self, piece: &str) -> Vec<Event> {
        let Some(b) = self.block.as_mut() else { return vec![] };
        if piece.is_empty() || !(b.kind == "text" || b.kind == "thinking") {
            return vec![];
        }
        let thinking = b.kind == "thinking";
        let first = b.text.is_empty();
        b.text.push_str(piece);
        let shown = if thinking { &mut self.shown.1 } else { &mut self.shown.0 };
        let mut out = vec![];
        if first && *shown {
            out.push(if thinking { Event::Reasoning("\n\n".into()) } else { Event::Content("\n\n".into()) });
        }
        *shown = true;
        out.push(if thinking { Event::Reasoning(piece.into()) } else { Event::Content(piece.into()) });
        out
    }
}

// ---------- mock ----------

pub const MOCK_REASONING: &str = "用户想检查排版。先列出要覆盖的格式：标题、列表、引用、表格、代码，\n再放几处不该生效的输入，比如原始 HTML。";

pub const MOCK_REPLY: &str = "## 排版自检\n\n这是开发版的**模拟回复**，用来检查消息的显示；正式版才会连接真正的模型。\n这一行紧跟在上一行后面。\n\n### 列表\n\n1. 第一项：包含 `行内代码`\n2. 第二项，下面还有子项：\n   - 子项 A\n   - 子项 B，带一个[链接](https://example.com)\n3. 第三项\n\n> 引用段落：阅读旧消息时，界面不会被拉到底部。\n\n| 模型 | 量化 | 上下文 |\n| --- | --- | --- |\n| Qwen3 14B | Q4_K_M | 16K |\n| Qwen3.5 9B | Q8_0 | 32K |\n\n```rust\nfn greet(name: &str) -> String {\n    format!(\"你好，{name}\")\n}\n```\n\n行内公式 $E = mc^2$，金额 $5 和 $10 不是公式。独立公式：\n\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\n\n这些应该原样显示：<b>不是粗体</b>。\n\n最后一段。";

/// Mock: canned text; with tools it looks things up for some words of the question (data.rs
/// `mock_calls`), then answers from what came back (`mock_answer`).
async fn mock_stream(thinking: bool, messages: &[Msg], opts: &Options, on: &mut (impl FnMut(Event) + Send)) -> Result<(), String> {
    let last_msg = messages.last();
    let last = last_msg.map_or("", |m| m.content.as_str());
    if last.contains("失败测试") {
        tokio::time::sleep(Duration::from_millis(300)).await;
        return Err("模拟的失败：模型回复意外中断。".into());
    }
    let system = messages.first().filter(|m| m.role == "system").map_or("", |m| m.content.as_str());
    let summary = system == super::context::SUMMARY_INSTRUCTIONS;
    let flat: String = last.chars().filter(|c| !c.is_whitespace()).collect();
    let tools = !opts.tools.is_empty();
    if tools && !opts.no_calls && last_msg.is_some_and(|m| m.role == "user") {
        let named: Vec<&str> = opts.tools.iter().filter_map(|t| t["function"]["name"].as_str()).collect();
        let calls = super::data::mock_calls(last, system, &named);
        if !calls.is_empty() {
            if thinking {
                on(Event::Reasoning("先查一下相关的数据。".into()));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            on(Event::ToolCalls(calls));
            on(Event::Finished("tool_calls".into()));
            return Ok(());
        }
    }
    let reply: String = if system.starts_with(super::assist::MARK) {
        super::assist::mock_reply(system, last)
    } else if system.starts_with(super::assistants::translate::MARK) {
        super::assistants::translate::mock_reply(system, last)
    } else if tools && last_msg.is_some_and(|m| m.role == "tool") {
        super::data::mock_answer(messages)
    } else if let Some(page) = last.split("【我正在看的页面】\n").nth(1).filter(|_| last_msg.is_some_and(|m| m.role == "user")) {
        // a question from the 小窗: say what page came with it
        format!("这是开发版的**模拟回答**。我看到你正在看：\n\n> {}\n", page.trim().replace('\n', "\n> "))
    } else if system.ends_with(super::reading::EXTRACTION) {
        let piece = flat.split("】：").nth(1).unwrap_or("").chars().take(12).collect::<String>();
        format!("模拟要点：这一段约 {} 字，讲到「{piece}」。", last.chars().count())
    } else if system.ends_with(super::reading::COMPRESSION) {
        format!("模拟合并：{} 条要点合在一起。", last.matches("模拟").count().max(1))
    } else if summary {
        let n = last.chars().count();
        let start: String = last.chars().filter(|c| !c.is_whitespace()).skip_while(|c| *c != '：').skip(1).take(12).collect();
        format!("模拟摘要：较早的对话约 {n} 字，从「{start}」聊起。")
    } else if last.contains("滚动测试") {
        (1..=60).map(|i| format!("第 {i} 段模拟回复：这里有一些用于检查长内容滚动的文字。\n\n")).collect()
    } else if last.contains(super::skills::SEARCHED_FIRST) {
        "这是开发版的**模拟回答**：网上搜到的里，最相关的是这几条[1][2]。".into()
    } else if system.starts_with(super::brief::MARK) {
        "这是开发版的**模拟简报**：今天有几件待办要做，记得看看过期的那几件；有一份投递该跟进了。最近的笔记里常提到读书。".into()
    } else if system.starts_with(super::yearly::MONTH_MARK) {
        format!("模拟要点：这个月的记录约 {} 字。", last.chars().count())
    } else if system.starts_with(super::yearly::PIECE_MARK) {
        format!("模拟要点：这一段约 {} 字。", last.chars().count())
    } else if system.contains(super::yearly::YEAR_MARK) {
        let numbers: Vec<&str> = last.split("【这一年的数字】\n").nth(1).unwrap_or("").split("\n\n").next().unwrap_or("").lines().take(6).collect();
        let how = if last.contains("【每个月的要点】") { format!("读了 {} 个月的要点", last.matches("### ").count()) } else { "一次读完了全年的记录".into() };
        format!(
            "**这一年的关键词**：模拟 · 开发版 · 柚儿园\n\n这是开发版的**模拟年度总结**（{how}）。正式版会写成一封给你的信：一段总述、这一年的数字、按主题分块、你写下的话、这一年之最，最后是写给明年。\n\n## 这一年的数字\n\n{}\n\n## 写给明年\n\n明年也慢慢来。",
            numbers.join("\n")
        )
    } else if system.contains(super::schedule::NOTE_MARK) {
        "这是开发版的**模拟回答**：定时任务到点写好了。正式版会先查柚儿园的数据（只读），再写成这样一段。".into()
    } else if last.contains("【[1] ") {
        format!("{MOCK_REPLY}（参考了检索到的资料 [1]）")
    } else {
        MOCK_REPLY.into()
    };
    let mut parts: Vec<(bool, &str)> = vec![];
    if thinking {
        parts.push((true, MOCK_REASONING));
    }
    parts.push((false, &reply));
    for (is_reasoning, text) in parts {
        let chars: Vec<char> = text.chars().collect();
        let (mut i, mut step) = (0, 0usize);
        while i < chars.len() {
            let n = (3 + step % 5).min(chars.len() - i);
            let piece: String = chars[i..i + n].iter().collect();
            on(if is_reasoning { Event::Reasoning(piece) } else { Event::Content(piece) });
            i += n;
            step += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    let prompt: usize = messages.iter().map(|m| m.content.chars().count()).sum();
    on(Event::Finished("stop".into()));
    on(Event::Usage { prompt: prompt as i64, completion: reply.chars().count() as i64 });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Local requests take turns: the second waits (Queued true, then false) until the first is done.
    #[test]
    fn local_requests_take_turns() {
        tauri::async_runtime::block_on(async {
            let first = local_turn(&mut |_| {}).await;
            let seen = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
            let log = seen.clone();
            let second = tauri::async_runtime::spawn(async move {
                let _p = local_turn(&mut |e| if let Event::Queued(q) = e { log.lock().unwrap().push(q) }).await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(*seen.lock().unwrap(), [true]); // still waiting
            drop(first);
            second.await.unwrap();
            assert_eq!(*seen.lock().unwrap(), [true, false]);
            let _again = local_turn(&mut |_| panic!("free again: no waiting")).await;
        });
    }

    fn parse_all(flavor: Flavor, chunks: &[&[u8]]) -> (Vec<Event>, Result<(), String>) {
        let mut p = Parser::new(flavor);
        let mut out = vec![];
        for c in chunks {
            match p.push(c) {
                Ok(e) => out.extend(e),
                Err(e) => return (out, Err(e)),
            }
        }
        match p.finish() {
            Ok(e) => out.extend(e),
            Err(e) => return (out, Err(e)),
        }
        (out, p.verdict())
    }

    #[test]
    fn local_stream_with_reasoning_usage_and_split_utf8() {
        let body = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\r\n\r\n: comment\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"timings\":{\"prompt_n\":5,\"predicted_n\":2}}\n\ndata: [DONE]\n\n";
        let bytes = body.as_bytes();
        // split inside the 3-byte "你"
        let cut = body.find("你").unwrap() + 1;
        let (events, ok) = parse_all(Flavor::Local, &[&bytes[..cut], &bytes[cut..]]);
        ok.unwrap();
        assert_eq!(
            events,
            [Event::Reasoning("想".into()), Event::Content("你好".into()), Event::Finished("stop".into()), Event::Usage { prompt: 5, completion: 2 }]
        );
    }

    #[test]
    fn multiline_data_and_missing_trailing_blank_line() {
        let (e, ok) = parse_all(Flavor::Local, &[b"data: {\"choices\":\ndata: [{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DONE]"]);
        ok.unwrap();
        assert_eq!(e, [Event::Content("a".into())]);
    }

    #[test]
    fn cut_streams_and_errors() {
        let (_, ok) = parse_all(Flavor::Local, &[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n"]);
        assert_eq!(ok.unwrap_err(), failure::LOCAL_CUT);
        let (_, ok) = parse_all(Flavor::Local, &[b"data: {\"error\":{\"message\":\"boom\"}}\n\n"]);
        assert_eq!(ok.unwrap_err(), "本地模型报错：boom");
        // cloud: server error text is never shown
        let (_, ok) = parse_all(Flavor::Cloud, &[b"data: {\"error\":{\"message\":\"secret detail\"}}\n\n"]);
        assert_eq!(ok.unwrap_err(), failure::UNAVAILABLE);
        // OpenRouter passes the upstream status on: a burst limit reads as one, and is asked again
        let (_, ok) = parse_all(Flavor::Cloud, &[b"data: {\"choices\":[],\"error\":{\"code\":429,\"message\":\"Provider returned error\"}}\n\n"]);
        assert_eq!(ok.unwrap_err(), failure::LIMITED);
        assert!(transient(failure::LIMITED) && transient(failure::UNAVAILABLE) && transient(failure::NETWORK) && !transient(failure::REQUEST) && !transient(failure::UNAUTHORIZED));
        assert_eq!((backoff(0, None), backoff(5, None), backoff(0, Some(60)), backoff(1, Some(3))), (Duration::from_secs(2), Duration::from_secs(10), Duration::from_secs(20), Duration::from_secs(3)));
        // cloud needs a finish reason before [DONE]
        let (_, ok) = parse_all(Flavor::Cloud, &[b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\ndata: [DONE]\n\n"]);
        assert_eq!(ok.unwrap_err(), failure::INTERRUPTED);
        let (_, ok) = parse_all(Flavor::Cloud, &[b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"weird\"}]}\n\n"]);
        assert_eq!(ok.unwrap_err(), failure::RESPONSE);
        let big = vec![b'x'; FRAME_LIMIT + 1];
        let (_, ok) = parse_all(Flavor::Cloud, &[&big]);
        assert_eq!(ok.unwrap_err(), failure::TOO_LARGE);
    }

    #[test]
    fn cloud_reasoning_field_and_other_choices() {
        let (e, ok) = parse_all(
            Flavor::Cloud,
            &[b"data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"r\",\"content\":\"c\"}},{\"index\":1,\"delta\":{\"content\":\"x\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4}}\n\ndata: [DONE]\n\n"],
        );
        ok.unwrap();
        assert_eq!(e, [Event::Reasoning("r".into()), Event::Content("c".into()), Event::Finished("length".into()), Event::Usage { prompt: 3, completion: 4 }]);
    }

    #[test]
    fn request_bodies_per_service() {
        let msgs = [Msg::new("user", "hi")];
        let local = body(&Target::Local { model: "qwen3.5-9b".into() }, &msgs, &Options { max_tokens: Some(100), thinking: Some(false), ..Default::default() });
        assert_eq!(local["model"], "qwen3.5-9b");
        assert_eq!(local["max_tokens"], 100);
        assert_eq!(local["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(local["stream_options"]["include_usage"], true);
        let model = |service| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192, thinking: Think::Switch };
        let oa = body(&Target::Cloud { model: model(Service::OpenAi), key: "k".into() }, &msgs, &Options { max_tokens: Some(99_999), thinking: None, ..Default::default() });
        assert_eq!((oa["max_completion_tokens"].as_i64(), oa["store"].as_bool(), oa.get("max_tokens")), (Some(8192), Some(false), None));
        let ds = body(&Target::Cloud { model: model(Service::DeepSeek), key: "k".into() }, &msgs, &Options { max_tokens: None, thinking: Some(true), ..Default::default() });
        assert_eq!((ds["max_tokens"].as_i64(), ds["thinking"]["type"].as_str()), (Some(8192), Some("enabled")));
        let or = body(&Target::Cloud { model: model(Service::OpenRouter), key: "k".into() }, &msgs, &Options { max_tokens: None, thinking: Some(false), ..Default::default() });
        assert_eq!(or["reasoning"]["enabled"], false);
        assert!(!oa.to_string().contains("\"k\"")); // the key is never in the body
    }

    #[test]
    fn streamed_tool_calls_come_whole_before_the_finish() {
        // OpenAI / DeepSeek style: id and name first, arguments in pieces, finish_reason tool_calls
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"查一下\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"pomelo_jobs\",\"arguments\":\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"view\\\":\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"stats\\\"}\"}},{\"index\":1,\"id\":\"call_2\",\"function\":{\"name\":\"pomelo_finance\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        );
        let (e, ok) = parse_all(Flavor::Cloud, &[body.as_bytes()]);
        ok.unwrap();
        let calls = vec![
            ToolCall { id: "call_1".into(), name: "pomelo_jobs".into(), arguments: r#"{"view":"stats"}"#.into() },
            ToolCall { id: "call_2".into(), name: "pomelo_finance".into(), arguments: "{}".into() },
        ];
        assert_eq!(e, [Event::Reasoning("查一下".into()), Event::ToolCalls(calls.clone()), Event::Finished("tool_calls".into()), Event::Usage { prompt: 9, completion: 3 }]);
        // llama-server may end without a finish reason for the calls: they still come at [DONE]
        let local = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"function\":{\"name\":\"pomelo_jobs\",\"arguments\":\"{}\"}}]}}]}\n\ndata: [DONE]\n\n";
        let (e, ok) = parse_all(Flavor::Local, &[local.as_bytes()]);
        ok.unwrap();
        assert_eq!(e, [Event::ToolCalls(vec![ToolCall { id: String::new(), name: "pomelo_jobs".into(), arguments: "{}".into() }])]);
    }

    #[test]
    fn tool_requests_and_deepseek_reasoning() {
        let tools = vec![json!({ "type": "function", "function": { "name": "pomelo_jobs" } })];
        let call = ToolCall { id: "c1".into(), name: "pomelo_jobs".into(), arguments: "{}".into() };
        let msgs = [
            Msg::new("user", "旧问题"),
            Msg::new("assistant", "旧回答"),
            Msg::new("user", "q"),
            Msg { role: "assistant".into(), content: String::new(), tool_calls: vec![call], reasoning: Some("想".into()), ..Default::default() },
            Msg::tool("c1", "结果"),
        ];
        let model = |service| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192, thinking: Think::Switch };
        let opts = Options { tools: tools.clone(), ..Default::default() };
        let ds = body(&Target::Cloud { model: model(Service::DeepSeek), key: "k".into() }, &msgs, &opts);
        assert_eq!(ds["tools"], json!(tools));
        assert!(ds.get("tool_choice").is_none());
        let m = ds["messages"].as_array().unwrap();
        assert_eq!(m[1]["reasoning_content"], ""); // every assistant message carries it when tools are on
        assert_eq!(m[3]["reasoning_content"], "想");
        assert_eq!(m[3]["tool_calls"][0], json!({ "id": "c1", "type": "function", "function": { "name": "pomelo_jobs", "arguments": "{}" } }));
        assert_eq!((m[4]["role"].as_str(), m[4]["tool_call_id"].as_str(), m[4]["content"].as_str()), (Some("tool"), Some("c1"), Some("结果")));
        assert!(m[0].get("reasoning_content").is_none());
        // other services never get reasoning back; the last round may forbid calls
        let oa = body(&Target::Cloud { model: model(Service::OpenAi), key: "k".into() }, &msgs, &Options { no_calls: true, ..opts.clone() });
        assert!(oa["messages"][3].get("reasoning_content").is_none());
        assert_eq!(oa["tool_choice"], "none");
        let local = body(&Target::Local { model: "m".into() }, &msgs, &opts);
        assert_eq!(local["tools"], json!(tools));
        assert!(local["messages"][3].get("reasoning_content").is_none());
        // no tools: plain messages as before
        let plain = body(&Target::Cloud { model: model(Service::DeepSeek), key: "k".into() }, &msgs[..3], &Options::default());
        assert_eq!(plain["messages"], json!([{ "role": "user", "content": "旧问题" }, { "role": "assistant", "content": "旧回答" }, { "role": "user", "content": "q" }]));
        assert!(plain.get("tools").is_none());
        assert!(rejected(failure::REQUEST) && rejected("本地模型接口返回 HTTP 500") && !rejected(failure::NETWORK));
    }

    #[test]
    fn openrouter_reasoning_blocks_are_put_together_and_go_back_with_the_calls() {
        let body_text = concat!(
            ": OPENROUTER PROCESSING\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"想\",\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"想\",\"index\":0,\"format\":\"anthropic-claude-v1\"}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"一下\",\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"一下\",\"index\":0}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"\",\"signature\":\"sig\",\"index\":0}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.encrypted\",\"data\":\"abc\",\"id\":\"tool_1\",\"index\":1,\"format\":\"google-gemini-v1\"}],\"tool_calls\":[{\"index\":0,\"id\":\"tool_1\",\"function\":{\"name\":\"pomelo_jobs\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let details = vec![
            json!({ "type": "reasoning.text", "text": "想一下", "index": 0, "format": "anthropic-claude-v1", "signature": "sig" }),
            json!({ "type": "reasoning.encrypted", "data": "abc", "id": "tool_1", "index": 1, "format": "google-gemini-v1" }),
        ];
        let call = ToolCall { id: "tool_1".into(), name: "pomelo_jobs".into(), arguments: "{}".into() };
        let (e, ok) = parse_all(Flavor::Cloud, &[body_text.as_bytes()]);
        ok.unwrap();
        assert_eq!(e, [Event::Reasoning("想".into()), Event::Reasoning("一下".into()), Event::ReasoningDetails(details.clone()), Event::ToolCalls(vec![call.clone()]), Event::Finished("tool_calls".into())]);
        // the next round: unchanged on the assistant message, no empty text beside the calls (OpenRouter only)
        let msgs = [
            Msg::new("user", "q"),
            Msg { role: "assistant".into(), content: String::new(), tool_calls: vec![call], reasoning: Some("想一下".into()), reasoning_details: details.clone(), ..Default::default() },
            Msg::tool("tool_1", "结果"),
        ];
        let model = |service| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192, thinking: Think::Switch };
        let opts = Options { tools: vec![json!({ "type": "function", "function": { "name": "pomelo_jobs" } })], ..Default::default() };
        let or = body(&Target::Cloud { model: model(Service::OpenRouter), key: "k".into() }, &msgs, &opts);
        assert_eq!((or["messages"][1]["reasoning_details"].clone(), or["messages"][1]["content"].clone()), (json!(details), Value::Null));
        assert!(or["messages"][1].get("reasoning_content").is_none() && or["messages"][0].get("reasoning_details").is_none());
        for service in [Service::DeepSeek, Service::OpenAi] {
            let b = body(&Target::Cloud { model: model(service), key: "k".into() }, &msgs, &opts);
            assert!(b["messages"][1].get("reasoning_details").is_none() && b["messages"][1]["content"] == "");
        }
        assert!(body(&Target::Local { model: "m".into() }, &msgs, &opts)["messages"][1].get("reasoning_details").is_none());
        // the local server's reasoning_details (if any) are not kept
        let (e, _) = parse_all(Flavor::Local, &[b"data: {\"choices\":[{\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"x\"}]}}]}\n\ndata: [DONE]\n\n"]);
        assert!(e.is_empty());
    }

    #[test]
    fn thinking_follows_what_the_model_can_and_a_refused_switch_is_dropped() {
        let (on, off) = (Options { thinking: Some(true), ..Default::default() }, Options { thinking: Some(false), ..Default::default() });
        let model = |service, thinking| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192, thinking };
        let sent = |service, thinking, o: &Options| body(&Target::Cloud { model: model(service, thinking), key: "k".into() }, &[Msg::new("user", "hi")], o);
        assert_eq!(sent(Service::DeepSeek, Think::Switch, &off)["thinking"]["type"], "disabled");
        assert_eq!(sent(Service::OpenRouter, Think::Switch, &on)["reasoning"]["enabled"], true);
        assert_eq!(sent(Service::OpenAi, Think::Switch, &off)["reasoning_effort"], "none");
        assert!(sent(Service::OpenAi, Think::Switch, &on).get("reasoning_effort").is_none());
        for t in [Think::Never, Think::Always] {
            assert!(sent(Service::DeepSeek, t, &off).get("thinking").is_none() && sent(Service::OpenRouter, t, &on).get("reasoning").is_none() && sent(Service::OpenAi, t, &off).get("reasoning_effort").is_none());
        }
        assert!(switch_sent(&model(Service::OpenRouter, Think::Switch), &off) && switch_sent(&model(Service::OpenRouter, Think::Switch), &on));
        assert!(switch_sent(&model(Service::OpenAi, Think::Switch), &off) && !switch_sent(&model(Service::OpenAi, Think::Switch), &on));
        assert!(!switch_sent(&model(Service::OpenRouter, Think::Always), &on) && !switch_sent(&model(Service::DeepSeek, Think::Switch), &off));
        assert!(!switch_sent(&model(Service::OpenRouter, Think::Switch), &Options::default()));
        assert!(Think::Switch.allows(true) && Think::Switch.allows(false) && !Think::Never.allows(true) && !Think::Always.allows(false));
        assert!(Think::Always.thinks(false) && !Think::Never.thinks(true) && Think::Switch.thinks(true) && !Think::Switch.thinks(false));
        assert!(!fixed_reasoning("google/gemini-x"));
        remember_fixed_reasoning("google/gemini-x");
        remember_fixed_reasoning("google/gemini-x");
        assert!(fixed_reasoning("google/gemini-x") && !fixed_reasoning("anthropic/claude-x"));
        assert_eq!(FIXED_REASONING.lock().unwrap().iter().filter(|m| *m == "google/gemini-x").count(), 1);
    }

    fn claude(api_model: &str, thinking: Think) -> CloudModel {
        CloudModel { id: "c".into(), name: "Claude".into(), api_model: api_model.into(), service: Service::Anthropic, context_window: 1_048_576, max_output_tokens: 131_072, thinking }
    }

    #[test]
    fn images_go_in_each_services_format() {
        let img = Image { media_type: "image/png".into(), data: "iVBOR".into() };
        let msgs = vec![Msg::new("system", "说中文"), Msg { role: "user".into(), content: "这是什么".into(), images: vec![img], ..Default::default() }];
        let opts = Options::default();
        let a = body(&Target::Cloud { model: claude("claude-opus-5-5", Think::Always), key: "k".into() }, &msgs, &opts);
        assert_eq!(a["messages"][0]["content"], json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBOR" } }, { "type": "text", "text": "这是什么" }]));
        let o = body(&Target::Local { model: "x".into() }, &msgs, &opts);
        assert_eq!(o["messages"][1]["content"], json!([{ "type": "text", "text": "这是什么" }, { "type": "image_url", "image_url": { "url": "data:image/png;base64,iVBOR" } }]));
        assert_eq!(o["messages"][0]["content"], "说中文");
        // an image looked at in a tool exchange: its own user message (OpenAI style), with the results (Anthropic)
        let call = ToolCall { id: "c1".into(), name: "pomelo_kb".into(), arguments: "{}".into() };
        let looked = vec![
            Msg::new("user", "白板上写了什么"),
            Msg { role: "assistant".into(), tool_calls: vec![call], ..Default::default() },
            Msg::tool("c1", "[1] 图片《白板.png》附在下面那条消息里"),
            Msg { role: "user".into(), content: "（这是刚才用 pomelo_kb 看的 1 张图片）".into(), images: vec![Image { media_type: "image/png".into(), data: "iVBOR".into() }], ..Default::default() },
        ];
        let or = body(&Target::Cloud { model: CloudModel { service: Service::OpenRouter, ..claude("x", Think::Switch) }, key: "k".into() }, &looked, &opts);
        assert_eq!((or["messages"][2]["role"].as_str(), or["messages"][3]["role"].as_str(), or["messages"][3]["content"][1]["type"].as_str()), (Some("tool"), Some("user"), Some("image_url")));
        let a = body(&Target::Cloud { model: claude("claude-opus-5-5", Think::Always), key: "k".into() }, &looked, &opts);
        let last = &a["messages"][2];
        assert_eq!(a["messages"].as_array().unwrap().len(), 3);
        assert_eq!((last["role"].as_str(), last["content"][0]["type"].as_str(), last["content"][1]["type"].as_str(), last["content"][2]["type"].as_str()), (Some("user"), Some("tool_result"), Some("image"), Some("text")));
    }

    #[test]
    fn anthropic_bodies() {
        let tools = vec![json!({ "type": "function", "function": { "name": "pomelo_todo", "description": "查待办", "parameters": { "type": "object", "properties": { "day": { "type": "string" } } } } })];
        let blocks = vec![
            json!({ "type": "thinking", "thinking": "先查", "signature": "sig-1" }),
            json!({ "type": "text", "text": "我查一下。" }),
            json!({ "type": "tool_use", "id": "toolu_1", "name": "pomelo_todo", "input": { "day": "today" } }),
            json!({ "type": "tool_use", "id": "toolu_2", "name": "pomelo_todo", "input": {} }),
        ];
        let calls = vec![
            ToolCall { id: "toolu_1".into(), name: "pomelo_todo".into(), arguments: r#"{"day":"today"}"#.into() },
            ToolCall { id: "toolu_2".into(), name: "pomelo_todo".into(), arguments: "{}".into() },
        ];
        let msgs = vec![
            Msg::new("system", "说中文"),
            Msg::new("system", "较早的对话摘要"),
            Msg::new("assistant", "（被截掉的历史开头）"),
            Msg::new("user", "第一问"),
            Msg::new("assistant", "第一答"),
            Msg::new("user", "今天有什么事"),
            Msg { role: "assistant".into(), content: "我查一下。".into(), tool_calls: calls.clone(), reasoning: Some("先查".into()), reasoning_details: blocks.clone(), ..Default::default() },
            Msg::tool("toolu_1", "两件"),
            Msg::tool("toolu_2", ""),
        ];
        let opts = Options { max_tokens: Some(500_000), thinking: Some(true), tools: tools.clone(), no_calls: false };
        let b = body(&Target::Cloud { model: claude("claude-opus-5-5", Think::Always), key: "sk-ant".into() }, &msgs, &opts);
        assert_eq!((b["model"].as_str(), b["max_tokens"].as_i64(), b["stream"].as_bool()), (Some("claude-opus-5-5"), Some(131_072), Some(true)));
        assert_eq!(b["system"], "说中文\n\n较早的对话摘要");
        let m = b["messages"].as_array().unwrap();
        // starts with the user; the run's response goes back verbatim; both results in one message
        assert_eq!(m.iter().map(|x| x["role"].as_str().unwrap()).collect::<Vec<_>>(), ["user", "assistant", "user", "assistant", "user"]);
        assert_eq!(m[1]["content"], json!([{ "type": "text", "text": "第一答" }]));
        assert_eq!(m[3]["content"], json!(blocks));
        assert_eq!(m[4]["content"], json!([{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "两件" }, { "type": "tool_result", "tool_use_id": "toolu_2" }]));
        assert_eq!(b["thinking"], json!({ "type": "adaptive", "display": "summarized", "block_binding": { "prefix_mismatch_behavior": "drop_block" } }));
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["tools"], json!([{ "name": "pomelo_todo", "description": "查待办", "input_schema": tools[0]["function"]["parameters"] }]));
        assert!(b.get("tool_choice").is_none() && !b.to_string().contains("sk-ant"));
        assert_eq!(anthropic_betas(&claude("claude-opus-5-5", Think::Always), &opts), [BETA_BINDING, BETA_FALLBACK]);
        // without its own blocks (another service's run): rebuilt from text and calls
        let plain = Msg { reasoning_details: vec![], ..msgs[6].clone() };
        assert_eq!(anthropic_assistant(&plain), json!([{ "type": "text", "text": "我查一下。" }, { "type": "tool_use", "id": "toolu_1", "name": "pomelo_todo", "input": { "day": "today" } }, { "type": "tool_use", "id": "toolu_2", "name": "pomelo_todo", "input": {} }]).as_array().unwrap().clone());
        // thinking per model; the last round must answer
        let off = Options { thinking: Some(false), tools: tools.clone(), no_calls: true, ..Default::default() };
        let sw = claude("claude-sonnet-4-6", Think::Switch);
        let b = body(&Target::Cloud { model: sw.clone(), key: "k".into() }, &msgs[3..4], &off);
        assert_eq!((b["thinking"].clone(), b["tool_choice"].clone(), b.get("fallbacks").is_none()), (json!({ "type": "disabled" }), json!({ "type": "none" }), true));
        assert!(anthropic_betas(&sw, &off).is_empty() && switch_sent(&sw, &off));
        // a model that turned `disabled` down is asked again with thinking on
        assert_eq!(body(&Target::Cloud { model: sw.clone(), key: "k".into() }, &msgs[3..4], &Options::default())["thinking"]["type"], "adaptive");
        let never = body(&Target::Cloud { model: claude("claude-haiku-4-5", Think::Never), key: "k".into() }, &msgs[3..4], &Options { thinking: Some(true), ..Default::default() });
        assert!(never.get("thinking").is_none() && never.get("system").is_none());
    }

    #[test]
    fn anthropic_stream_thinking_text_and_tool_calls() {
        let sse = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":900,\"output_tokens\":1}}}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"先查待\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"办\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"EqQB\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"我查一下。\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"查今天的\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"Zz\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"pomelo_todo\",\"input\":{}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"day\\\":\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"今天\\\"}\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":3}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":42}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        // split inside a multi-byte character
        let bytes = sse.as_bytes();
        let cut = sse.find("待").unwrap() + 1;
        let (events, verdict) = parse_all(Flavor::Anthropic, &[&bytes[..cut], &bytes[cut..]]);
        verdict.unwrap();
        let reasoning: String = events.iter().filter_map(|e| if let Event::Reasoning(t) = e { Some(t.as_str()) } else { None }).collect();
        let content: String = events.iter().filter_map(|e| if let Event::Content(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!((reasoning.as_str(), content.as_str()), ("先查待办\n\n查今天的", "我查一下。"));
        let details = events.iter().find_map(|e| if let Event::ReasoningDetails(d) = e { Some(d.clone()) } else { None }).unwrap();
        assert_eq!(
            details,
            vec![
                json!({ "type": "thinking", "thinking": "先查待办", "signature": "EqQB" }),
                json!({ "type": "text", "text": "我查一下。" }),
                json!({ "type": "thinking", "thinking": "查今天的", "signature": "Zz" }),
                json!({ "type": "tool_use", "id": "toolu_1", "name": "pomelo_todo", "input": { "day": "今天" } }),
            ]
        );
        let tail: Vec<&Event> = events.iter().skip_while(|e| !matches!(e, Event::ReasoningDetails(_))).collect();
        assert_eq!(tail[1], &Event::ToolCalls(vec![ToolCall { id: "toolu_1".into(), name: "pomelo_todo".into(), arguments: r#"{"day":"今天"}"#.into() }]));
        assert_eq!(tail[2], &Event::Finished("tool_calls".into()));
        assert_eq!(tail[3], &Event::Usage { prompt: 1000, completion: 42 });
        // replayed next round exactly as received
        let next = Msg { role: "assistant".into(), content: content.clone(), tool_calls: vec![ToolCall { id: "toolu_1".into(), name: "pomelo_todo".into(), arguments: "{}".into() }], reasoning_details: details.clone(), ..Default::default() };
        assert_eq!(anthropic_assistant(&next), details);
    }

    #[test]
    fn anthropic_stream_endings() {
        let start = "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n";
        let text = |t: &str| format!("data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{t}\"}}}}\n\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n");
        let stop = |r: &str| format!("data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{r}\"}},\"usage\":{{\"output_tokens\":3}}}}\n\ndata: {{\"type\":\"message_stop\"}}\n\n");
        for (r, want) in [("end_turn", "stop"), ("max_tokens", "length"), ("refusal", "content_filter"), ("pause_turn", "stop"), ("something_new", "stop")] {
            let all = format!("{start}{}{}", text("好"), stop(r));
            let (events, verdict) = parse_all(Flavor::Anthropic, &[all.as_bytes()]);
            verdict.unwrap();
            assert!(events.contains(&Event::Finished(want.into())), "{r}");
        }
        // cut before message_stop / message_delta
        let (_, verdict) = parse_all(Flavor::Anthropic, &[format!("{start}{}", text("好")).as_bytes()]);
        assert_eq!(verdict.unwrap_err(), failure::INTERRUPTED);
        // an error event (overloaded) shows only the fixed text
        let err = format!("{start}event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}}}\n\n");
        let (_, verdict) = parse_all(Flavor::Anthropic, &[err.as_bytes()]);
        assert_eq!(verdict.unwrap_err(), failure::UNAVAILABLE);
        // another model took over mid-reply: only the text before it stays with the response
        let fallback = format!(
            "{start}data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}}}\n\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"signature_delta\",\"signature\":\"s\"}}}}\n\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n{}data: {{\"type\":\"content_block_start\",\"index\":2,\"content_block\":{{\"type\":\"fallback\",\"from\":{{\"model\":\"a\"}},\"to\":{{\"model\":\"b\"}}}}}}\n\ndata: {{\"type\":\"content_block_stop\",\"index\":2}}\n\n{}{}",
            text("前半"),
            text("后半"),
            stop("end_turn")
        );
        let (events, verdict) = parse_all(Flavor::Anthropic, &[fallback.as_bytes()]);
        verdict.unwrap();
        let details = events.iter().find_map(|e| if let Event::ReasoningDetails(d) = e { Some(d.clone()) } else { None }).unwrap();
        assert_eq!(details, vec![json!({ "type": "text", "text": "前半" }), json!({ "type": "text", "text": "后半" })]);
    }

    #[test]
    fn google_signatures_go_back_on_their_calls() {
        let body_sse = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"pomelo_todo\",\"arguments\":\"{}\"},\"extra_content\":{\"google\":{\"thought_signature\":\"SIG\"}}},{\"index\":1,\"id\":\"c2\",\"type\":\"function\",\"function\":{\"name\":\"pomelo_notes\",\"arguments\":\"{}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2}}\n\n",
        );
        // no [DONE]: Google's stream may just end
        let mut p = Parser::new(Flavor::Cloud).google();
        let mut events = p.push(body_sse.as_bytes()).unwrap();
        events.extend(p.finish().unwrap());
        p.verdict().unwrap();
        assert!(Parser::new(Flavor::Cloud).verdict().is_err());
        let details = events.iter().find_map(|e| if let Event::ReasoningDetails(d) = e { Some(d.clone()) } else { None }).unwrap();
        let calls = events.iter().find_map(|e| if let Event::ToolCalls(c) = e { Some(c.clone()) } else { None }).unwrap();
        let model = CloudModel { id: "g".into(), name: "Gemini".into(), api_model: "gemini-3-flash".into(), service: Service::Google, context_window: 1_048_576, max_output_tokens: 65_536, thinking: Think::Switch };
        let msgs = [Msg::new("user", "今天"), Msg { role: "assistant".into(), tool_calls: calls, reasoning_details: details.clone(), ..Default::default() }, Msg::tool("c1", "两件"), Msg::tool("c2", "无")];
        let b = body(&Target::Cloud { model: model.clone(), key: "k".into() }, &msgs, &Options { thinking: Some(false), ..Default::default() });
        assert_eq!(b["messages"][1]["tool_calls"][0]["extra_content"], json!({ "google": { "thought_signature": "SIG" } }));
        assert!(b["messages"][1]["tool_calls"][1].get("extra_content").is_none() && b["messages"][1].get("reasoning_details").is_none());
        assert_eq!((b["max_tokens"].as_i64(), b["reasoning_effort"].as_str()), (Some(65_536), Some("none")));
        assert!(switch_sent(&model, &Options { thinking: Some(false), ..Default::default() }));
        // OpenRouter never gets Google's entries back, and its own stream keeps no extras
        let or = CloudModel { service: Service::OpenRouter, ..model };
        assert!(body(&Target::Cloud { model: or, key: "k".into() }, &msgs, &Options::default())["messages"][1].get("reasoning_details").is_none());
        let mut p = Parser::new(Flavor::Cloud);
        let events = p.push(body_sse.as_bytes()).unwrap();
        assert!(!events.iter().any(|e| matches!(e, Event::ReasoningDetails(_))));
    }

    /// The real HTTP path against a fake SSE server on a random local port.
    #[test]
    fn local_http_stream_end_to_end() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let events = ["data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n", "data: {\"choices\":[{\"delta\":{\"content\":\"好\"},\"finish_reason\":\"stop\"}]}\n\n", "data: [DONE]\n\n"];
            write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            for e in events {
                s.write_all(e.as_bytes()).unwrap();
                s.flush().unwrap();
                std::thread::sleep(Duration::from_millis(10));
            }
            req
        });
        let got = tauri::async_runtime::block_on(async move {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let resp = client.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).json(&body(&Target::Local { model: "m".into() }, &[Msg::new("user", "hi")], &Options::default())).send().await.unwrap();
            let mut events = vec![];
            read_sse(resp, Parser::new(Flavor::Local), &mut |e| events.push(e)).await.map(|_| events)
        })
        .unwrap();
        assert_eq!(got, [Event::Content("你".into()), Event::Content("好".into()), Event::Finished("stop".into())]);
        let req = server.join().unwrap();
        assert!(req.contains("\"stream\":true") && req.contains("\"model\":\"m\""), "{req}");
    }

    #[test]
    fn mock_streams_reasoning_then_reply() {
        let mut events = vec![];
        tauri::async_runtime::block_on(mock_stream(true, &[Msg::new("user", "hi")], &Options::default(), &mut |e| events.push(e))).unwrap();
        let reasoning: String = events.iter().filter_map(|e| if let Event::Reasoning(t) = e { Some(t.as_str()) } else { None }).collect();
        let content: String = events.iter().filter_map(|e| if let Event::Content(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!((reasoning.as_str(), content.as_str()), (MOCK_REASONING, MOCK_REPLY));
        let mut sink = |_| {};
        assert!(tauri::async_runtime::block_on(mock_stream(false, &[Msg::new("user", "失败测试")], &Options::default(), &mut sink)).is_err());
    }
}

