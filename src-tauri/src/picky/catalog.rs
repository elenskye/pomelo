//! Chat models from the LLM manifest (`~/Documents/LLM/config/chat-models.json`), shared with
//! `llmctl.sh`. Same validation as the script (and the old Picky): a manifest accepted here is one
//! the script can start. Read-only: Pomelo never edits the LLM folder.

use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashSet;

pub const SUPPORTED_VERSION: i64 = 1;
const CACHE_TYPES: &[&str] = &["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"];
const CONTEXT: std::ops::RangeInclusive<i64> = 512..=1_048_576;

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

pub fn parse(text: &str) -> Result<Vec<LocalModel>, String> {
    parse_inner(text).map_err(|e| format!("模型清单有误：{e}"))
}

fn parse_inner(text: &str) -> Result<Vec<LocalModel>, String> {
    let root: Value = serde_json::from_str(text).map_err(|_| "不是有效的 JSON".to_string())?;
    let obj = root.as_object().ok_or("不是有效的 JSON")?;
    let version = obj.get("version").ok_or("清单缺少“version”")?.as_i64().ok_or("清单的 version 的类型不对")?;
    if version != SUPPORTED_VERSION {
        return Err(format!("不支持版本 {version}，应为 {SUPPORTED_VERSION}"));
    }
    let models = obj.get("models").ok_or("清单缺少“models”")?.as_array().ok_or("清单的 models 的类型不对")?;
    if models.is_empty() {
        return Err("没有列出任何模型".into());
    }
    let mut out = Vec::new();
    let mut names = HashSet::new();
    for (i, v) in models.iter().enumerate() {
        let place = format!("第 {} 个模型", i + 1);
        let m = v.as_object().ok_or_else(|| format!("{place}的类型不对"))?;
        let s = |k: &str| -> Result<String, String> { req(m, k, &place)?.as_str().map(String::from).ok_or_else(|| format!("{place}的 {k} 的类型不对")) };
        let opt = |k: &str| -> Result<Option<String>, String> {
            match m.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(x)) => Ok(Some(x.clone())),
                _ => Err(format!("{place}的 {k} 的类型不对")),
            }
        };
        let model = LocalModel {
            key: s("key")?,
            name: s("name")?,
            title: s("title")?,
            note: opt("note")?,
            quant: s("quant")?,
            file: s("file")?,
            alias: s("alias")?,
            // as_i64 is None for fractions (8192.5) and strings
            context: req(m, "context", &place)?.as_i64().ok_or_else(|| format!("{place}的 context 的类型不对"))?,
            cache_type: s("cacheType")?,
            thinking: req(m, "thinking", &place)?.as_bool().ok_or_else(|| format!("{place}的 thinking 的类型不对"))?,
            listed: true,
        };
        let mmproj = opt("mmproj")?;
        for (field, value) in [("key", &model.key), ("alias", &model.alias)] {
            if !valid_name(value) {
                return Err(format!("{place}的 {field}“{value}”只能用字母、数字和 . _ -，且不能以 - 或 . 开头"));
            }
        }
        if model.name.is_empty() || model.title.is_empty() {
            return Err(format!("{place}需要 name 和 title"));
        }
        if !relative_path(&model.file) {
            return Err(format!("{place}的 file 必须是 LLM 目录内的相对路径"));
        }
        if let Some(p) = mmproj.filter(|p| !p.is_empty()) {
            if !relative_path(&p) {
                return Err(format!("{place}的 mmproj 必须是 LLM 目录内的相对路径"));
            }
        }
        if !CONTEXT.contains(&model.context) {
            return Err(format!("{place}的 context 应在 {}～{} 之间", CONTEXT.start(), CONTEXT.end()));
        }
        if !CACHE_TYPES.contains(&model.cache_type.as_str()) {
            return Err(format!("{place}的 cacheType“{}”不是 llama.cpp 支持的缓存类型", model.cache_type));
        }
        let own: HashSet<&String> = [&model.key, &model.alias].into_iter().collect();
        for n in own {
            if !names.insert(n.clone()) {
                return Err(format!("“{n}”被多个模型使用"));
            }
        }
        out.push(model);
    }
    Ok(out)
}

fn req<'a>(m: &'a Map<String, Value>, k: &str, place: &str) -> Result<&'a Value, String> {
    match m.get(k) {
        None => Err(format!("{place}缺少“{k}”")),
        Some(Value::Null) => Err(format!("{place}的 {k} 不能为空")),
        Some(v) => Ok(v),
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

/// Mock mode's list: the real models' shape, never read from the LLM folder.
pub fn preview() -> Vec<LocalModel> {
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
    vec![
        m("qwen14b", "Qwen3 14B", "Qwen3 14B Abliterated", "Q4_K_M", "qwen3-14b-abliterated", 16_384, true),
        m("qwen359b", "Qwen3.5 9B", "Qwen3.5 9B", "Q8_0", "qwen3.5-9b", 32_768, true),
        m("qwen35heretic9b", "Qwen3.5 9B Heretic", "Qwen3.5 9B Heretic ARA Refusals5", "Q8_0", "qwen3.5-9b-heretic-ara-refusals5", 32_768, true),
        m("ministral14b", "Ministral 3 14B", "Ministral 3 14B Abliterated", "Q4_K_M", "ministral-3-14b-abliterated", 16_384, false),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"version":1,"models":[
      {"key":"qwen14b","name":"Qwen3 14B","title":"Qwen3 14B Abliterated","note":"x","quant":"Q4_K_M",
       "file":"models/a/a.gguf","alias":"qwen3-14b","context":16384,"cacheType":"q8_0","thinking":true},
      {"key":"mini","name":"Mini","title":"Mini","quant":"Q8_0","file":"models/b.gguf","mmproj":"",
       "alias":"mini-alias","context":8192,"cacheType":"f16","thinking":false}]}"#;

    #[test]
    fn valid_manifest() {
        let ms = parse(VALID).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!((ms[0].key.as_str(), ms[0].alias.as_str(), ms[0].context, ms[0].thinking), ("qwen14b", "qwen3-14b", 16384, true));
        assert_eq!(ms[1].note, None);
    }

    /// The old Picky's manifest fixtures (read-only reference repo; skipped when absent) get the same verdicts.
    #[test]
    fn same_rules_as_picky_fixtures() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../picky/Tests/Fixtures/chat-models");
        let Ok(valid) = std::fs::read_to_string(fixtures.join("valid.json")) else { return };
        parse(&valid).unwrap();
        for e in std::fs::read_dir(fixtures.join("invalid")).unwrap() {
            let p = e.unwrap().path();
            let text = std::fs::read_to_string(&p).unwrap();
            assert!(parse(&text).is_err(), "{} should be refused", p.display());
        }
    }

    #[test]
    fn invalid_manifests_explain_why() {
        let cases = [
            ("nope", "不是有效的 JSON"),
            (r#"{"version":2,"models":[]}"#, "不支持版本 2"),
            (r#"{"version":1,"models":[]}"#, "没有列出任何模型"),
            (&VALID.replace(r#""key":"mini","#, ""), "第 2 个模型缺少“key”"),
            (&VALID.replace(r#""context":8192"#, r#""context":"8192""#), "第 2 个模型的 context 的类型不对"),
            (&VALID.replace(r#""context":8192"#, r#""context":8192.5"#), "第 2 个模型的 context 的类型不对"),
            (&VALID.replace(r#""context":8192"#, r#""context":100"#), "context 应在"),
            (&VALID.replace("q8_0", "q9"), "cacheType“q9”"),
            (&VALID.replace("models/b.gguf", "/abs/b.gguf"), "file 必须是"),
            (&VALID.replace("models/b.gguf", "../b.gguf"), "file 必须是"),
            (&VALID.replace(r#""mmproj":"""#, r#""mmproj":"../x""#), "mmproj 必须是"),
            (&VALID.replace("mini-alias", "qwen14b"), "“qwen14b”被多个模型使用"),
            (&VALID.replace(r#""key":"mini""#, r#""key":"-mini""#), "只能用字母"),
            (&VALID.replace(r#""thinking":false"#, r#""thinking":"no""#), "thinking 的类型不对"),
        ];
        for (text, want) in cases {
            let e = parse(text).unwrap_err();
            assert!(e.starts_with("模型清单有误：") && e.contains(want), "{want}: {e}");
        }
    }

    #[test]
    fn preview_is_valid_shape() {
        let p = preview();
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|m| valid_name(&m.key) && valid_name(&m.alias)));
    }
}
