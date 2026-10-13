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

use super::cloud::{CloudModel, Service};
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
}

impl Msg {
    pub fn new(role: &str, content: &str) -> Self {
        Msg { role: role.into(), content: content.into(), ..Default::default() }
    }
    pub fn tool(call_id: &str, content: &str) -> Self {
        Msg { role: "tool".into(), content: content.into(), tool_call_id: Some(call_id.into()), ..Default::default() }
    }

    /// The message as sent; `reasoning`: add `reasoning_content` to assistant messages.
    fn wire(&self, reasoning: bool) -> Value {
        let mut v = json!({ "role": self.role, "content": self.content });
        if !self.tool_calls.is_empty() {
            v["tool_calls"] = self.tool_calls.iter().map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } })).collect();
        }
        if let Some(id) = &self.tool_call_id {
            v["tool_call_id"] = json!(id);
        }
        if reasoning && self.role == "assistant" {
            v["reasoning_content"] = json!(self.reasoning.as_deref().unwrap_or(""));
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
    /// stop | length | content_filter | tool_calls
    Finished(String),
    Usage { prompt: i64, completion: i64 },
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

fn http_failure(status: u16) -> &'static str {
    match status {
        401 => failure::UNAUTHORIZED,
        402 | 403 => failure::CREDITS,
        429 => failure::LIMITED,
        400 | 404 | 422 => failure::REQUEST,
        500..=599 => failure::UNAVAILABLE,
        _ => failure::RESPONSE,
    }
}

/// The JSON body for a request (exposed for tests: the wire contract without any secret).
pub fn body(target: &Target, messages: &[Msg], opts: &Options) -> Value {
    let reasoning = !opts.tools.is_empty() && matches!(target, Target::Cloud { model, .. } if model.service == Service::DeepSeek);
    let messages: Vec<Value> = messages.iter().map(|m| m.wire(reasoning)).collect();
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
            let mut b = json!({ "model": model.api_model, "messages": messages, "stream": true, "stream_options": { "include_usage": true } });
            match model.service {
                Service::OpenAi => {
                    b["max_completion_tokens"] = json!(max);
                    b["store"] = json!(false);
                }
                Service::DeepSeek => {
                    b["max_tokens"] = json!(max);
                    if let Some(t) = opts.thinking {
                        b["thinking"] = json!({ "type": if t { "enabled" } else { "disabled" } });
                    }
                }
                Service::OpenRouter => {
                    b["max_tokens"] = json!(max);
                    if let Some(t) = opts.thinking {
                        b["reasoning"] = json!({ "enabled": t });
                    }
                }
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

/// Streams one reply, calling `on` for every event in arrival order. Ok(()) = the stream ended
/// properly ([DONE]; cloud also needs a finish reason). Cancel by dropping / aborting the task.
pub async fn stream(target: Target, messages: Vec<Msg>, opts: Options, mut on: impl FnMut(Event) + Send) -> Result<(), String> {
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
            read_sse(resp, Flavor::Local, &mut on).await
        }
        Target::Cloud { model, key } => {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(600))
                .timeout(Duration::from_secs(1200))
                .build()
                .map_err(|e| e.to_string())?;
            let resp = client
                .post(format!("{}/chat/completions", model.service.base_url()))
                .bearer_auth(key)
                .header("Accept", "text/event-stream")
                .json(&body(&target, &messages, &opts))
                .send()
                .await
                .map_err(|_| failure::NETWORK.to_string())?;
            if resp.status() != 200 {
                return Err(http_failure(resp.status().as_u16()).into());
            }
            let sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.to_lowercase().starts_with("text/event-stream"));
            if !sse {
                return Err(failure::RESPONSE.into());
            }
            read_sse(resp, Flavor::Cloud, &mut on).await
        }
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

async fn read_sse(mut resp: reqwest::Response, flavor: Flavor, on: &mut (impl FnMut(Event) + Send)) -> Result<(), String> {
    let mut p = Parser::new(flavor);
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(_) => return Err(flavor.cut().into()),
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
    Cloud,
}

impl Flavor {
    fn cut(self) -> &'static str {
        match self {
            Flavor::Local => failure::LOCAL_CUT,
            Flavor::Cloud => failure::INTERRUPTED,
        }
    }
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
}

impl Parser {
    pub fn new(flavor: Flavor) -> Self {
        Parser { flavor, line: vec![], payload: vec![], payload_bytes: 0, done: false, saw_finish: false, calls: vec![] }
    }

    /// The finished tool calls, once (unnamed fragments dropped).
    fn take_calls(&mut self) -> Option<Event> {
        let calls: Vec<ToolCall> = std::mem::take(&mut self.calls).into_iter().filter(|c| !c.name.is_empty()).collect();
        (!calls.is_empty()).then_some(Event::ToolCalls(calls))
    }

    fn add_call_delta(&mut self, d: &Value) -> Result<(), String> {
        let i = d["index"].as_u64().map_or(self.calls.len().saturating_sub(usize::from(d.get("id").is_none())), |i| i as usize);
        if i > 64 {
            return Err(failure::TOO_LARGE.into());
        }
        if self.calls.len() <= i {
            self.calls.resize(i + 1, ToolCall::default());
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
            Flavor::Cloud if self.done && self.saw_finish => Ok(()),
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
            return Ok(self.take_calls().into_iter().collect());
        }
        let cloud = self.flavor == Flavor::Cloud;
        let v: Value = serde_json::from_str(&text).map_err(|_| if cloud { failure::RESPONSE.to_string() } else { "本地模型返回了无法处理的数据。".to_string() })?;
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            return Err(if cloud { failure::UNAVAILABLE.into() } else { format!("本地模型报错：{}", e["message"].as_str().unwrap_or("未知错误")) });
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
        let model = |service| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192 };
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
        let model = |service| CloudModel { id: "x".into(), name: "X".into(), api_model: "api-x".into(), service, context_window: 131_072, max_output_tokens: 8192 };
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
            read_sse(resp, Flavor::Local, &mut |e| events.push(e)).await.map(|_| events)
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
