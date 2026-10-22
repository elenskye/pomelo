//! Cloud models (OpenRouter, DeepSeek, OpenAI, Anthropic, Google, Aliyun), as in the old Picky:
//! - Requests go only to each service's fixed official HTTPS endpoint (provider.rs). Anthropic
//!   speaks its own Messages API; Google and Aliyun (Qwen, the Beijing region: the user's account)
//!   their OpenAI-compatible endpoints; the rest chat completions.
//! - The model list (name, API model name, token limits) lives in picky.db `preferences`
//!   (`cloud.models`; its presence, even `[]`, marks the defaults as seeded — deleted defaults stay
//!   deleted). Keys never enter the database, backups, exports or logs: only the keychain
//!   (service "<data dir name> 小拾", account = service code, or a skill's key: tavily, tmdb). The
//!   release build is signed with a
//!   fixed self-signed certificate, so an update can still read them.
//! - Mock mode keeps keys in memory.
//! - Each model says whether it can think (`Think`; set by the user, default 能开关): only a 能开关 one
//!   gets the service's thinking switch; the others are sent without it (provider.rs).
//! - How hard a thinking model thinks (`Effort`, v0.6: 低 / 中 / 高) is asked per request; each
//!   service gets its own words for it, and one with only a switch (Aliyun, local models) just thinks.

use super::store::{pref_get, pref_set};
use crate::modules::common::err;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub const MODELS_KEY: &str = "cloud.models";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Service {
    #[serde(rename = "openRouter")]
    OpenRouter,
    #[serde(rename = "deepSeek")]
    DeepSeek,
    #[serde(rename = "openAI")]
    OpenAi,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "aliyun")]
    Aliyun,
}

impl Service {
    pub const ALL: [Service; 6] = [Service::OpenRouter, Service::DeepSeek, Service::OpenAi, Service::Anthropic, Service::Google, Service::Aliyun];

    pub fn name(self) -> &'static str {
        match self {
            Service::OpenRouter => "OpenRouter",
            Service::DeepSeek => "DeepSeek",
            Service::OpenAi => "OpenAI",
            Service::Anthropic => "Anthropic",
            Service::Google => "Google",
            Service::Aliyun => "Aliyun",
        }
    }
    /// The only place a key is ever sent.
    pub fn base_url(self) -> &'static str {
        match self {
            Service::OpenRouter => "https://openrouter.ai/api/v1",
            Service::DeepSeek => "https://api.deepseek.com/v1",
            Service::OpenAi => "https://api.openai.com/v1",
            Service::Anthropic => "https://api.anthropic.com/v1",
            Service::Google => "https://generativelanguage.googleapis.com/v1beta/openai",
            Service::Aliyun => "https://dashscope.aliyuncs.com/compatible-mode/v1",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Service::OpenRouter => "openRouter",
            Service::DeepSeek => "deepSeek",
            Service::OpenAi => "openAI",
            Service::Anthropic => "anthropic",
            Service::Google => "google",
            Service::Aliyun => "aliyun",
        }
    }
}

/// What a model can do about thinking: 不能思考 / 能开关 / 只能思考.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Think {
    Never,
    #[default]
    Switch,
    Always,
}

impl Think {
    /// A local model from the catalog (`thinking`: its template has the switch).
    pub fn local(thinking: bool) -> Think {
        if thinking { Think::Switch } else { Think::Never }
    }
    /// Whether a request that wants `on` can have it.
    pub fn allows(self, on: bool) -> bool {
        match self {
            Think::Switch => true,
            Think::Never => !on,
            Think::Always => on,
        }
    }
    /// Thinking actually happens when `on` is asked for.
    pub fn thinks(self, on: bool) -> bool {
        self == Think::Always || (self == Think::Switch && on)
    }
}

/// How hard to think when thinking (低 / 中 / 高); None in a request = the model's own level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    /// OpenAI's, Google's, OpenRouter's and Anthropic's word for it.
    pub fn word(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModel {
    pub id: String,
    pub name: String,
    pub api_model: String,
    pub service: Service,
    pub context_window: i64,
    pub max_output_tokens: i64,
    /// Saved before this existed: 能开关.
    #[serde(default)]
    pub thinking: Think,
}

pub const BAD_MODEL: &str = "请检查模型名称、接口模型名和令牌上限。";

impl CloudModel {
    pub fn validated(&self) -> Result<CloudModel, String> {
        let mut v = self.clone();
        v.name = v.name.trim().to_string();
        v.api_model = v.api_model.trim().to_string();
        let id_ok = !v.id.is_empty() && v.id.len() <= 100 && v.id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        let name_ok = !v.name.is_empty() && v.name.chars().count() <= 128 && !v.name.chars().any(char::is_control);
        let api_ok = !v.api_model.is_empty() && v.api_model.len() <= 256 && v.api_model.bytes().all(|b| (33..=126).contains(&b));
        let limits_ok = (2048..=4_194_304).contains(&v.context_window) && (16..=393_216).contains(&v.max_output_tokens) && v.max_output_tokens < v.context_window;
        if id_ok && name_ok && api_ok && limits_ok {
            Ok(v)
        } else {
            Err(BAD_MODEL.into())
        }
    }
}

pub fn defaults() -> Vec<CloudModel> {
    let m = |id: &str, name: &str| CloudModel { id: id.into(), name: name.into(), api_model: id.into(), service: Service::DeepSeek, context_window: 1_048_576, max_output_tokens: 8192, thinking: Think::Switch };
    vec![m("deepseek-flash", "DeepSeek Flash"), m("deepseek-v4-pro", "DeepSeek V4 Pro")]
}

fn validated_all(models: &[CloudModel]) -> Result<Vec<CloudModel>, String> {
    if models.len() > 200 || models.iter().map(|m| &m.id).collect::<HashSet<_>>().len() != models.len() {
        return Err(BAD_MODEL.into());
    }
    models.iter().map(CloudModel::validated).collect()
}

/// The list (seeding the defaults once).
pub fn models(conn: &Connection) -> Result<Vec<CloudModel>, String> {
    match pref_get::<Vec<CloudModel>>(conn, MODELS_KEY).map_err(|_| "无法读取云端模型配置。".to_string())? {
        Some(list) => validated_all(&list).map_err(|_| "无法读取云端模型配置。".into()),
        None => {
            let d = defaults();
            pref_set(conn, MODELS_KEY, &d)?;
            Ok(d)
        }
    }
}

pub fn model(conn: &Connection, id: &str) -> Result<Option<CloudModel>, String> {
    Ok(models(conn)?.into_iter().find(|m| m.id == id))
}

/// Edits (`id` given) or adds a model; returns the saved one.
pub fn save_model(conn: &Connection, mut input: CloudModel) -> Result<CloudModel, String> {
    let mut list = models(conn)?;
    if input.id.is_empty() {
        input.id = new_id();
    }
    let saved = input.validated()?;
    match list.iter_mut().find(|m| m.id == saved.id) {
        Some(m) => *m = saved.clone(),
        None => list.push(saved.clone()),
    }
    pref_set(conn, MODELS_KEY, &validated_all(&list)?)?;
    Ok(saved)
}

pub fn delete_model(conn: &Connection, id: &str) -> Result<(), String> {
    let list: Vec<CloudModel> = models(conn)?.into_iter().filter(|m| m.id != id).collect();
    pref_set(conn, MODELS_KEY, &list)
}

/// The list in `ids`' order (设置 → 云端模型, dragged); the model menu follows it.
pub fn reorder_models(conn: &Connection, ids: &[String]) -> Result<(), String> {
    let list = models(conn)?;
    let same = ids.len() == list.len() && ids.iter().collect::<HashSet<_>>().len() == ids.len() && ids.iter().all(|i| list.iter().any(|m| &m.id == i));
    if !same {
        return Err("云端模型列表已经变了，请重新拖一次".into());
    }
    let ordered: Vec<CloudModel> = ids.iter().filter_map(|i| list.iter().find(|m| &m.id == i).cloned()).collect();
    pref_set(conn, MODELS_KEY, &ordered)
}

fn new_id() -> String {
    let mut b = [0u8; 6];
    let _ = getrandom::fill(&mut b);
    format!("m-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

// ---------- keys ----------

pub fn checked_key(key: &str) -> Result<String, String> {
    let v = key.trim();
    if v.is_empty() || v.len() > 4096 || !v.bytes().all(|b| (33..=126).contains(&b)) {
        return Err("API Key 为空或格式无效。".into());
    }
    Ok(v.into())
}

/// Where API keys live. Keychain presence is cached (status is polled often); our own saves and
/// removals keep the cache right.
pub struct KeyStore {
    kind: KeyKind,
    present: Mutex<HashMap<String, bool>>,
}

enum KeyKind {
    Keychain { service: String },
    Memory(Mutex<HashMap<String, String>>),
}

impl KeyStore {
    pub fn keychain() -> Self {
        KeyStore { kind: KeyKind::Keychain { service: format!("{} 小拾", crate::paths::data_dir_name()) }, present: Mutex::default() }
    }
    pub fn memory() -> Self {
        KeyStore { kind: KeyKind::Memory(Mutex::default()), present: Mutex::default() }
    }

    /// `account`: a service's code (Service::code) or a skill key's account.
    pub fn has(&self, account: &str) -> bool {
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map(|m| m.contains_key(account)).unwrap_or(false),
            KeyKind::Keychain { service } => {
                let mut p = self.present.lock().unwrap_or_else(|e| e.into_inner());
                *p.entry(account.to_string()).or_insert_with(|| keychain::exists(service, account))
            }
        }
    }

    pub fn read(&self, account: &str) -> Result<String, String> {
        let missing = || "还没有保存这个 Key。".to_string();
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map_err(err)?.get(account).cloned().ok_or_else(missing),
            KeyKind::Keychain { service } => match keychain::read(service, account) {
                Some(bytes) => String::from_utf8(bytes).map_err(|_| "无法读取 Key，请重新保存。".to_string()),
                None => Err(missing()),
            },
        }
    }

    /// `label`: what Keychain Access shows (「Pomelo 小拾 · DeepSeek」).
    pub fn save(&self, account: &str, key: &str, label: &str) -> Result<(), String> {
        let key = checked_key(key)?;
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.insert(account.to_string(), key);
            }
            KeyKind::Keychain { service } => keychain::save(service, account, &key, label)?,
        }
        self.present.lock().map_err(err)?.insert(account.to_string(), true);
        Ok(())
    }

    pub fn remove(&self, account: &str) -> Result<(), String> {
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.remove(account);
            }
            KeyKind::Keychain { service } => keychain::remove(service, account)?,
        }
        self.present.lock().map_err(err)?.insert(account.to_string(), false);
        Ok(())
    }
}

#[cfg(not(test))]
mod keychain {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
    use security_framework::passwords::{delete_generic_password, set_generic_password_options, PasswordOptions};

    fn search(service: &str, account: &str, data: bool) -> Option<Vec<SearchResult>> {
        ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
            .account(account)
            .load_attributes(!data)
            .load_data(data)
            .skip_authenticated_items(true) // never show a keychain prompt; unreadable = missing
            .limit(Limit::Max(1))
            .search()
            .ok()
    }

    pub fn exists(service: &str, account: &str) -> bool {
        search(service, account, false).is_some_and(|r| !r.is_empty())
    }

    pub fn read(service: &str, account: &str) -> Option<Vec<u8>> {
        search(service, account, true)?.into_iter().find_map(|r| match r {
            SearchResult::Data(d) => Some(d),
            _ => None,
        })
    }

    pub fn save(service: &str, account: &str, key: &str, label: &str) -> Result<(), String> {
        let _ = delete_generic_password(service, account);
        let mut o = PasswordOptions::new_generic_password(service, account);
        o.set_label(label);
        set_generic_password_options(key.as_bytes(), o).map_err(|_| "无法将 API Key 保存到钥匙串。".to_string())
    }

    pub fn remove(service: &str, account: &str) -> Result<(), String> {
        match delete_generic_password(service, account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()), // errSecItemNotFound
            Err(_) => Err("无法移除已保存的 API Key。".into()),
        }
    }
}

/// Unit tests never touch the real keychain.
#[cfg(test)]
mod keychain {
    pub fn exists(_: &str, _: &str) -> bool {
        false
    }
    pub fn read(_: &str, _: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn save(_: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("tests use KeyStore::Memory".into())
    }
    pub fn remove(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picky::store::tests::memory_db;

    #[test]
    fn defaults_seed_once_and_deletions_stick() {
        let c = memory_db();
        assert_eq!(models(&c).unwrap().len(), 2);
        let ids: Vec<String> = models(&c).unwrap().iter().map(|m| m.id.clone()).rev().collect();
        reorder_models(&c, &ids).unwrap();
        assert_eq!(models(&c).unwrap().iter().map(|m| m.id.clone()).collect::<Vec<_>>(), ids);
        assert!(reorder_models(&c, &ids[1..]).is_err());
        delete_model(&c, "deepseek-flash").unwrap();
        delete_model(&c, "deepseek-v4-pro").unwrap();
        assert!(models(&c).unwrap().is_empty()); // [] still marks "seeded"
    }

    #[test]
    fn a_list_saved_before_thinking_reads_as_switchable() {
        let c = memory_db();
        c.execute("INSERT INTO preferences (key, value) VALUES ('cloud.models', ?1)", [r#"[{"id":"g","name":"Gemini","apiModel":"google/gemini","service":"openRouter","contextWindow":1048576,"maxOutputTokens":65536}]"#]).unwrap();
        assert_eq!(models(&c).unwrap()[0].thinking, Think::Switch);
        let saved = save_model(&c, CloudModel { thinking: Think::Always, ..models(&c).unwrap()[0].clone() }).unwrap();
        assert_eq!(model(&c, &saved.id).unwrap().unwrap().thinking, Think::Always);
    }

    #[test]
    fn save_validates_and_assigns_ids() {
        let c = memory_db();
        let base = CloudModel { id: String::new(), name: " GPT ".into(), api_model: "gpt-5".into(), service: Service::OpenAi, context_window: 200_000, max_output_tokens: 8192, thinking: Think::Switch };
        let saved = save_model(&c, base.clone()).unwrap();
        assert!(saved.id.starts_with("m-"));
        assert_eq!(saved.name, "GPT");
        assert_eq!(models(&c).unwrap().len(), 3);
        let edited = save_model(&c, CloudModel { name: "GPT 5".into(), ..saved.clone() }).unwrap();
        assert_eq!(model(&c, &saved.id).unwrap().unwrap().name, "GPT 5");
        assert_eq!(edited.id, saved.id);
        for bad in [
            CloudModel { api_model: "has space".into(), ..base.clone() },
            CloudModel { max_output_tokens: 300_000, ..base.clone() },
            CloudModel { context_window: 1000, ..base.clone() },
            CloudModel { name: "  ".into(), ..base.clone() },
        ] {
            assert_eq!(save_model(&c, bad).unwrap_err(), BAD_MODEL);
        }
    }

    #[test]
    fn service_codes_match_picky() {
        assert_eq!(serde_json::to_string(&Service::OpenAi).unwrap(), "\"openAI\"");
        assert_eq!((serde_json::to_string(&Service::Aliyun).unwrap(), Service::Aliyun.code(), Service::Aliyun.name()), ("\"aliyun\"".to_string(), "aliyun", "Aliyun"));
        assert_eq!(serde_json::from_str::<Effort>("\"high\"").unwrap(), Effort::High);
        let m: CloudModel = serde_json::from_str(r#"{"id":"x","name":"X","apiModel":"x","service":"openRouter","contextWindow":131072,"maxOutputTokens":8192}"#).unwrap();
        assert_eq!(m.service, Service::OpenRouter);
    }

    #[test]
    fn memory_keys() {
        let k = KeyStore::memory();
        let ds = Service::DeepSeek.code();
        assert!(!k.has(ds));
        assert!(k.read(ds).unwrap_err().contains("还没有保存"));
        assert!(k.save(ds, " bad key ", "t").is_err());
        k.save(ds, "  sk-abc123 ", "t").unwrap();
        assert_eq!(k.read(ds).unwrap(), "sk-abc123");
        k.save("tavily", "tvly-1", "t").unwrap(); // a skill's key beside it
        k.remove(ds).unwrap();
        assert!(!k.has(ds) && k.has("tavily"));
    }
}
