//! The local model manifest (`~/Documents/LLM/config/models.json`, version 2), shared with
//! `llmctl.sh`: chat models (8080, one at a time), embedding models (8081) and rerank models
//! (8082); the first of a kind is the one Pomelo uses. Same validation as the script: a manifest
//! accepted here is one the script can start. Pomelo only reads the LLM folder.

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub const SUPPORTED_VERSION: i64 = 2;
const CACHE_TYPES: &[&str] = &["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"];
const POOLING: &[&str] = &["none", "mean", "cls", "last"];
const KINDS: [&str; 3] = ["chat", "embedding", "rerank"];
const CONTEXT: std::ops::RangeInclusive<i64> = 512..=1_048_576;
/// The user's rule (llmctl.sh refuses more too): a chat model of 14B or more — or one whose name
/// doesn't say — gets at most 16K, a smaller one (9B) 32K. Embedding and rerank models aren't limited.
pub fn chat_context_max(name: &str) -> i64 {
    let size = name
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'B'))
        .filter_map(|w| w.strip_suffix('B').and_then(|n| n.parse::<f64>().ok()))
        .next_back();
    match size {
        Some(b) if b < 14.0 => 32_768,
        _ => 16_384,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// Control key for llmctl.sh (e.g. qwen14b).
    pub key: String,
    /// Short name for menus.
    pub name: String,
    pub title: String,
    pub note: Option<String>,
    pub quant: String,
    #[serde(skip)]
    pub file: String,
    /// Model name the API serves (`--alias`), sent with chat requests.
    pub alias: String,
    pub context: i64,
    #[serde(skip)]
    pub cache_type: String,
    pub thinking: bool,
    /// False for a running model no longer in the manifest (can still be shown and stopped).
    pub listed: bool,
}

impl LocalModel {
    pub fn unlisted(key: &str, alias: Option<&str>) -> Self {
        LocalModel {
            key: key.into(),
            name: key.into(),
            title: format!("{key}（不在模型清单中）"),
            note: None,
            quant: "—".into(),
            file: String::new(),
            alias: alias.unwrap_or(key).into(),
            context: 0,
            cache_type: String::new(),
            thinking: false,
            listed: false,
        }
    }
}

/// An embedding or rerank model; its API model name is its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceModel {
    pub key: String,
    pub name: String,
    pub title: String,
    pub note: Option<String>,
    pub quant: String,
    #[serde(skip)]
    pub file: String,
    pub context: i64,
    /// Embedding only: how a search question is phrased (`{task}`, `{text}`); None = as is.
    #[serde(skip)]
    pub query: Option<String>,
}

impl ServiceModel {
    /// The text to embed for a search question.
    pub fn question(&self, task: &str, text: &str) -> String {
        match &self.query {
            Some(q) => q.replace("{task}", task).replace("{text}", text),
            None => text.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub chat: Vec<LocalModel>,
    pub embedding: Vec<ServiceModel>,
    pub rerank: Vec<ServiceModel>,
}

pub fn parse(text: &str) -> Result<Catalog, String> {
    parse_inner(text).map_err(|e| format!("模型清单有误：{e}"))
}

fn parse_inner(text: &str) -> Result<Catalog, String> {
    let root: Value = serde_json::from_str(text).map_err(|_| "不是有效的 JSON".to_string())?;
    let obj = root.as_object().ok_or("不是有效的 JSON")?;
    let version = obj.get("version").ok_or("清单缺少“version”")?.as_i64().ok_or("清单的 version 的类型不对")?;
    if version != SUPPORTED_VERSION {
        return Err(format!("不支持版本 {version}，应为 {SUPPORTED_VERSION}"));
    }
    let runtime = obj.get("runtime").ok_or("清单缺少“runtime”")?.as_str().ok_or("清单的 runtime 的类型不对")?;
    if !relative_path(runtime) {
        return Err("runtime 必须是 LLM 目录内的相对路径".into());
    }
    let list = |kind: &str| -> Result<&Vec<Value>, String> { obj.get(kind).ok_or(format!("清单缺少“{kind}”"))?.as_array().ok_or(format!("清单的 {kind} 的类型不对")) };
    let mut cat = Catalog::default();
    let mut names = HashSet::new();
    for kind in KINDS {
        let label = match kind {
            "chat" => "聊天模型",
            "embedding" => "向量模型",
            _ => "重排模型",
        };
        for (i, v) in list(kind)?.iter().enumerate() {
            let place = format!("第 {} 个{label}", i + 1);
            let m = v.as_object().ok_or_else(|| format!("{place}的类型不对"))?;
            let e = Entry { m, place: &place };
            let (key, name, title, quant, file) = (e.s("key")?, e.s("name")?, e.s("title")?, e.s("quant")?, e.s("file")?);
            let note = e.opt("note")?;
            // as_i64 is None for fractions (8192.5) and strings
            let context = e.req("context")?.as_i64().ok_or_else(|| format!("{place}的 context 的类型不对"))?;
            let alias = if kind == "chat" { e.s("alias")? } else { key.clone() };
            for (field, value) in [("key", &key), ("alias", &alias)] {
                if !valid_name(value) {
                    return Err(format!("{place}的 {field}“{value}”只能用字母、数字和 . _ -，且不能以 - 或 . 开头"));
                }
                if KINDS.contains(&value.as_str()) {
                    return Err(format!("{place}的 {field}“{value}”是类名，不能用"));
                }
            }
            if name.is_empty() || title.is_empty() {
                return Err(format!("{place}需要 name 和 title"));
            }
            if !relative_path(&file) {
                return Err(format!("{place}的 file 必须是 LLM 目录内的相对路径"));
            }
            if !CONTEXT.contains(&context) {
                return Err(format!("{place}的 context 应在 {}～{} 之间", CONTEXT.start(), CONTEXT.end()));
            }
            if kind == "chat" && context > chat_context_max(&name) {
                return Err(format!("{place}的 context 最多 {}（14B 的聊天模型最多 16K，9B 的最多 32K）", chat_context_max(&name)));
            }
            let own: HashSet<&String> = [&key, &alias].into_iter().collect();
            for n in own {
                if !names.insert(n.clone()) {
                    return Err(format!("“{n}”被多个模型使用"));
                }
            }
            match kind {
                "chat" => {
                    let cache_type = e.s("cacheType")?;
                    let thinking = e.req("thinking")?.as_bool().ok_or_else(|| format!("{place}的 thinking 的类型不对"))?;
                    if let Some(p) = e.opt("mmproj")?.filter(|p| !p.is_empty()) {
                        if !relative_path(&p) {
                            return Err(format!("{place}的 mmproj 必须是 LLM 目录内的相对路径"));
                        }
                    }
                    if !CACHE_TYPES.contains(&cache_type.as_str()) {
                        return Err(format!("{place}的 cacheType“{cache_type}”不是 llama.cpp 支持的缓存类型"));
                    }
                    cat.chat.push(LocalModel { key, name, title, note, quant, file, alias, context, cache_type, thinking, listed: true });
                }
                _ => {
                    let mut query = None;
                    if kind == "embedding" {
                        let pooling = e.s("pooling")?;
                        if !POOLING.contains(&pooling.as_str()) {
                            return Err(format!("{place}的 pooling“{pooling}”应为 none、mean、cls 或 last"));
                        }
                        query = e.opt("query")?;
                        if query.as_ref().is_some_and(|q| !q.contains("{text}")) {
                            return Err(format!("{place}的 query 里要有 {{text}}"));
                        }
                    }
                    let model = ServiceModel { key, name, title, note, quant, file, context, query };
                    if kind == "embedding" {
                        cat.embedding.push(model);
                    } else {
                        cat.rerank.push(model);
                    }
                }
            }
        }
    }
    if cat.chat.is_empty() {
        return Err("没有列出任何聊天模型".into());
    }
    Ok(cat)
}

struct Entry<'a> {
    m: &'a Map<String, Value>,
    place: &'a str,
}

impl Entry<'_> {
    fn req(&self, k: &str) -> Result<&Value, String> {
        match self.m.get(k) {
            None => Err(format!("{}缺少“{k}”", self.place)),
            Some(Value::Null) => Err(format!("{}的 {k} 不能为空", self.place)),
            Some(v) => Ok(v),
        }
    }
    fn s(&self, k: &str) -> Result<String, String> {
        self.req(k)?.as_str().map(String::from).ok_or_else(|| format!("{}的 {k} 的类型不对", self.place))
    }
    fn opt(&self, k: &str) -> Result<Option<String>, String> {
        match self.m.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(x)) => Ok(Some(x.clone())),
            _ => Err(format!("{}的 {k} 的类型不对", self.place)),
        }
    }
}

pub fn valid_name(v: &str) -> bool {
    match v.chars().next() {
        None | Some('-') | Some('.') => false,
        _ => v.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)),
    }
}

fn relative_path(v: &str) -> bool {
    !v.is_empty() && !v.starts_with('/') && !v.split('/').any(|p| p == ".." || p == ".")
}

/// Mock mode's manifest: the real models' shape, never read from the LLM folder. The mock
/// embedding key differs from the real one because mock vectors are a different model.
pub fn preview() -> Catalog {
    let m = |key: &str, name: &str, title: &str, quant: &str, alias: &str, context: i64, thinking: bool| LocalModel {
        key: key.into(),
        name: name.into(),
        title: title.into(),
        note: Some("模拟清单".into()),
        quant: quant.into(),
        file: format!("models/preview/{key}.gguf"),
        alias: alias.into(),
        context,
        cache_type: "f16".into(),
        thinking,
        listed: true,
    };
    let s = |key: &str, name: &str, query: Option<&str>| ServiceModel {
        key: key.into(),
        name: name.into(),
        title: name.into(),
        note: Some("模拟清单".into()),
        quant: "Q8_0".into(),
        file: format!("models/preview/{key}.gguf"),
        context: 2048,
        query: query.map(String::from),
    };
    Catalog {
        chat: vec![
            m("qwen14b", "Qwen3 14B", "Qwen3 14B Abliterated", "Q4_K_M", "qwen3-14b-abliterated", 16_384, true),
            m("qwen359b", "Qwen3.5 9B", "Qwen3.5 9B", "Q8_0", "qwen3.5-9b", 32_768, true),
            m("qwen35heretic9b", "Qwen3.5 9B Heretic", "Qwen3.5 9B Heretic ARA Refusals5", "Q8_0", "qwen3.5-9b-heretic-ara-refusals5", 32_768, true),
            m("ministral14b", "Ministral 3 14B", "Ministral 3 14B Abliterated", "Q4_K_M", "ministral-3-14b-abliterated", 16_384, false),
        ],
        embedding: vec![s("mock-embedding", "Qwen3-Embedding-0.6B", Some("Instruct: {task}\nQuery:{text}"))],
        rerank: vec![s("mock-reranker", "Qwen3-Reranker-0.6B", None)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"version":2,"runtime":"runtimes/llama-b1",
      "chat":[
       {"key":"qwen14b","name":"Qwen3 14B","title":"Qwen3 14B Abliterated","note":"x","quant":"Q4_K_M",
        "file":"models/a/a.gguf","alias":"qwen3-14b","context":16384,"cacheType":"q8_0","thinking":true},
       {"key":"mini","name":"Mini","title":"Mini","quant":"Q8_0","file":"models/b.gguf","mmproj":"",
        "alias":"mini-alias","context":8192,"cacheType":"f16","thinking":false}],
      "embedding":[{"key":"emb","name":"Emb","title":"Emb","quant":"Q8_0","file":"models/e.gguf","context":2048,
        "pooling":"last","query":"Instruct: {task}\nQuery:{text}"}],
      "rerank":[{"key":"rr","name":"Rr","title":"Rr","quant":"Q8_0","file":"models/r.gguf","context":2048}]}"#;

    #[test]
    fn valid_manifest() {
        let c = parse(VALID).unwrap();
        assert_eq!(c.chat.len(), 2);
        assert_eq!((c.chat[0].key.as_str(), c.chat[0].alias.as_str(), c.chat[0].context, c.chat[0].thinking), ("qwen14b", "qwen3-14b", 16384, true));
        assert_eq!(c.chat[1].note, None);
        assert_eq!((c.embedding[0].key.as_str(), c.rerank[0].key.as_str()), ("emb", "rr"));
        assert_eq!(c.embedding[0].question("Find notes", "找房子"), "Instruct: Find notes\nQuery:找房子");
        assert_eq!(c.rerank[0].question("x", "找房子"), "找房子");
        // empty service lists are allowed: those services just aren't available
        let c = parse(&VALID.replace(r#""rerank":[{"#, r#""rerank":[],"x":[{"#)).unwrap();
        assert!(c.rerank.is_empty());
    }

    #[test]
    fn invalid_manifests_explain_why() {
        let cases = [
            ("nope", "不是有效的 JSON"),
            (&VALID.replace(r#""version":2"#, r#""version":1"#), "不支持版本 1"),
            (&VALID.replace(r#""runtime":"runtimes/llama-b1","#, ""), "清单缺少“runtime”"),
            (&VALID.replace("runtimes/llama-b1", "/abs"), "runtime 必须是"),
            (&VALID.replace(r#""rerank":"#, r#""rr":"#), "清单缺少“rerank”"),
            (&VALID.replace(r#""key":"mini","#, ""), "第 2 个聊天模型缺少“key”"),
            (&VALID.replace(r#""context":8192"#, r#""context":"8192""#), "第 2 个聊天模型的 context 的类型不对"),
            (&VALID.replace(r#""context":8192"#, r#""context":8192.5"#), "第 2 个聊天模型的 context 的类型不对"),
            (&VALID.replace(r#""context":8192"#, r#""context":100"#), "context 应在"),
            (&VALID.replace(r#""context":8192"#, r#""context":65536"#), "context 最多"),
            (&VALID.replace("q8_0", "q9"), "cacheType“q9”"),
            (&VALID.replace("models/b.gguf", "/abs/b.gguf"), "file 必须是"),
            (&VALID.replace("models/r.gguf", "../r.gguf"), "第 1 个重排模型的 file 必须是"),
            (&VALID.replace(r#""mmproj":"""#, r#""mmproj":"../x""#), "mmproj 必须是"),
            (&VALID.replace("mini-alias", "qwen14b"), "“qwen14b”被多个模型使用"),
            (&VALID.replace(r#""key":"rr""#, r#""key":"emb""#), "“emb”被多个模型使用"),
            (&VALID.replace(r#""key":"rr""#, r#""key":"rerank""#), "“rerank”是类名"),
            (&VALID.replace(r#""key":"mini""#, r#""key":"-mini""#), "只能用字母"),
            (&VALID.replace(r#""thinking":false"#, r#""thinking":"no""#), "thinking 的类型不对"),
            (&VALID.replace(r#""pooling":"last""#, r#""pooling":"max""#), "pooling“max”"),
            (&VALID.replace(r#""pooling":"last","#, ""), "第 1 个向量模型缺少“pooling”"),
            (&VALID.replace("Query:{text}", "Query:"), "query 里要有 {text}"),
            (&VALID.replace(r#""context":2048}]}"#, r#""context":2048,"note":3}]}"#), "第 1 个重排模型的 note 的类型不对"),
        ];
        for (text, want) in cases {
            let e = parse(text).unwrap_err();
            assert!(e.starts_with("模型清单有误：") && e.contains(want), "{want}: {e}");
        }
        let no_chat = VALID.split(r#""chat":["#).next().unwrap().to_string() + r#""chat":[],"embedding":[],"rerank":[]}"#;
        assert!(parse(&no_chat).unwrap_err().contains("没有列出任何聊天模型"));
    }

    /// The real manifest in the LLM folder (read-only; skipped when absent) parses.
    #[test]
    fn the_llm_folder_manifest_parses() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let Ok(text) = std::fs::read_to_string(std::path::Path::new(&home).join("Documents/LLM/config/models.json")) else { return };
        let c = parse(&text).unwrap();
        assert!(!c.chat.is_empty() && !c.embedding.is_empty() && !c.rerank.is_empty());
    }

    #[test]
    fn preview_is_valid_shape() {
        let p = preview();
        assert_eq!((p.chat.len(), p.embedding.len(), p.rerank.len()), (4, 1, 1));
        assert!(p.chat.iter().all(|m| valid_name(&m.key) && valid_name(&m.alias)));
    }

    #[test]
    fn chat_context_follows_model_size() {
        assert_eq!(chat_context_max("Qwen3.5 9B"), 32_768);
        assert_eq!(chat_context_max("Qwen3.5 9B Heretic"), 32_768);
        assert_eq!(chat_context_max("Qwen3 14B"), 16_384);
        assert_eq!(chat_context_max("Ministral 3 14B"), 16_384);
        assert_eq!(chat_context_max("Mini"), 16_384); // size unknown
    }
}
