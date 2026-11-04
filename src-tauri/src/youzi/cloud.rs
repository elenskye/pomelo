//! Cloud models (OpenRouter, DeepSeek, OpenAI, Anthropic, Google, Aliyun):
//! - Requests go only to each service's fixed official HTTPS endpoint (provider.rs). Anthropic
//!   speaks its own Messages API; Google and Aliyun (Qwen, the Beijing region: the user's account)
//!   their OpenAI-compatible endpoints; the rest chat completions.
//! - The model list (name, API model name, token limits) lives in youzi.db `preferences`
//!   (`cloud.models`; its presence, even `[]`, marks the defaults as seeded — deleted defaults stay
//!   deleted). Keys never enter the database, backups, exports or logs: only the keychain
//!   (service "Pomelo OS Agent", labels 「Pomelo OS Agent · DeepSeek」; account = service code, or a
//!   skill's key: tavily, tmdb, Mathpix). The release build is signed with a fixed self-signed
//!   certificate, so an update can still read them. Keys saved under the old service 「Pomelo OS 小拾」
//!   are never read: the user saves them again.
//! - Mock mode keeps keys in memory.
//! - Each model says whether it can think (`Think`; set by the user, default 能开关): only a 能开关 one
//!   gets the service's thinking switch; the others are sent without it (provider.rs).
//! - How hard a thinking model thinks (思考强度): the user lists the levels a model takes, in its API's own words
//!   (`efforts`, none = it has none), and may fix one as its default (`default_effort`). provider.rs `stream` is the one
//!   place a request gets its level: what it asked if this model has it, else the model's default, else none (the model's
//!   own). So every call of 柚子 has the default without doing anything, and the few that let me choose (`Level`: Pub2MD,
//!   Importer; a conversation's own) override it. A local model has only the switch.

use super::store::{pref_get, pref_set};
use crate::modules::common::err;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};

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

/// A choice of thinking where I can make one (Pub2MD's two models, Importer): "off", "on" (at the model's default) or one
/// of its levels. A word this model doesn't have counts as "on".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Level(pub String);

impl Default for Level {
    fn default() -> Self {
        Level::off()
    }
}

impl Level {
    pub fn off() -> Level {
        Level("off".into())
    }
    pub fn on() -> Level {
        Level("on".into())
    }
    /// As it's kept where 思考 is a switch and a level (a conversation, a schedule, an assistant's choice).
    pub fn of(thinking: bool, effort: Option<&str>) -> Level {
        match effort {
            _ if !thinking => Level::off(),
            Some(word) => Level::word(word),
            None => Level::on(),
        }
    }
    pub fn word(word: &str) -> Level {
        Level(word.into())
    }
    pub fn thinking(&self) -> bool {
        self.0 != "off"
    }
    /// The level it asks for, if any.
    pub fn effort(&self) -> Option<String> {
        (self.0 != "off" && self.0 != "on").then(|| self.0.clone())
    }
    pub fn name(&self) -> String {
        match self.0.as_str() {
            "off" => "不思考".into(),
            "on" => "思考".into(),
            w => format!("思考 · {w}"),
        }
    }
}

/// What thinking at this level takes on top of a reply (Anthropic and OpenAI count it in the reply cap).
pub fn effort_room(word: Option<&str>) -> i64 {
    match word {
        None | Some("minimal" | "low") => 0,
        Some("medium") => 8_192,
        Some("high") => 16_384,
        Some(_) => 32_768,
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
    /// The 思考强度 it takes, in its API's words (none: it has none).
    #[serde(default)]
    pub efforts: Vec<String>,
    /// The level a request gets when it doesn't choose (none: the model's own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

pub const BAD_MODEL: &str = "请检查模型名称、接口模型名和令牌上限。";
pub const BAD_EFFORTS: &str = "思考强度写接口用的英文词（如 low、high、max），最多 12 个，不重复。";

impl CloudModel {
    /// The level a request gets: what it asked if this model has it, else the default.
    pub fn effort(&self, asked: Option<&str>) -> Option<String> {
        asked.filter(|a| self.efforts.iter().any(|e| e == a)).map(str::to_string).or_else(|| self.default_effort.clone())
    }
    pub fn validated(&self) -> Result<CloudModel, String> {
        let mut v = self.clone();
        v.name = v.name.trim().to_string();
        v.api_model = v.api_model.trim().to_string();
        v.efforts = if v.thinking == Think::Never { vec![] } else { v.efforts.iter().map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()).collect() };
        let words_ok = v.efforts.len() <= 12 && v.efforts.iter().collect::<HashSet<_>>().len() == v.efforts.len() && v.efforts.iter().all(|e| e.len() <= 20 && e.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'));
        if !words_ok || v.efforts.iter().any(|e| e == "off" || e == "on") {
            return Err(BAD_EFFORTS.into());
        }
        v.default_effort = v.default_effort.as_deref().map(|e| e.trim().to_lowercase()).filter(|e| v.efforts.contains(e));
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
    let m = |id: &str, name: &str| CloudModel { id: id.into(), name: name.into(), api_model: id.into(), service: Service::DeepSeek, context_window: 1_048_576, max_output_tokens: 8192, thinking: Think::Switch, efforts: ["low", "high", "max"].map(String::from).to_vec(), default_effort: None };
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

/// Where API keys live. In the keychain they are one item (service 「Pomelo OS Agent」, account
/// `keys`, a JSON map account → key): a new build (self-signed, no team id: macOS trusts each build
/// on its own) asks once, not once per key. It is read once (`load`, at launch) and kept in memory;
/// `has` never shows a prompt. A launch that finds the old one-item-per-key layout reads those
/// (one prompt each, the last time), saves them as the one item and deletes them where it may.
pub struct KeyStore {
    kind: KeyKind,
}

enum KeyKind {
    Keychain { service: String, state: Mutex<Loaded>, done: Condvar },
    Memory(Mutex<HashMap<String, String>>),
}

#[derive(Default)]
enum Loaded {
    #[default]
    NotYet,
    Loading,
    Ok(HashMap<String, String>),
    /// Turned down or unreadable: retried on the next read or save (something the user did).
    Failed,
}

/// The account of the one item.
pub const KEYS_ACCOUNT: &str = "keys";

impl KeyStore {
    pub fn keychain() -> Self {
        let service = format!("{} Agent", crate::paths::data_dir_name());
        KeyStore { kind: KeyKind::Keychain { service, state: Mutex::default(), done: Condvar::new() } }
    }
    pub fn memory() -> Self {
        KeyStore { kind: KeyKind::Memory(Mutex::default()) }
    }

    /// Reads the keychain item (may show the system prompt). At launch, on its own thread.
    pub fn load(&self) {
        let _ = self.loaded();
    }

    /// The keys, loading them first when not yet (or not) read; waits for a load in progress.
    fn loaded(&self) -> Result<HashMap<String, String>, String> {
        let KeyKind::Keychain { service, state, done } = &self.kind else { return Err("not a keychain".into()) };
        let mut s = state.lock().map_err(err)?;
        loop {
            match &*s {
                Loaded::Ok(m) => return Ok(m.clone()),
                Loaded::Loading => s = done.wait(s).map_err(err)?,
                Loaded::NotYet | Loaded::Failed => break,
            }
        }
        *s = Loaded::Loading;
        drop(s);
        let read = read_all(service);
        let mut s = state.lock().map_err(err)?;
        *s = match &read {
            Ok(m) => Loaded::Ok(m.clone()),
            Err(_) => Loaded::Failed,
        };
        done.notify_all();
        read
    }

    /// `account`: a service's code (Service::code) or a skill key's account. Never prompts.
    pub fn has(&self, account: &str) -> bool {
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map(|m| m.contains_key(account)).unwrap_or(false),
            KeyKind::Keychain { state, .. } => state.lock().map(|s| matches!(&*s, Loaded::Ok(m) if m.contains_key(account))).unwrap_or(false),
        }
    }

    pub fn read(&self, account: &str) -> Result<String, String> {
        let missing = || "还没有保存这个 Key。".to_string();
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map_err(err)?.get(account).cloned().ok_or_else(missing),
            KeyKind::Keychain { .. } => self.loaded()?.get(account).cloned().ok_or_else(missing),
        }
    }

    pub fn save(&self, account: &str, key: &str) -> Result<(), String> {
        let key = checked_key(key)?;
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.insert(account.to_string(), key);
                Ok(())
            }
            KeyKind::Keychain { .. } => self.change(|m| {
                m.insert(account.to_string(), key);
            }),
        }
    }

    pub fn remove(&self, account: &str) -> Result<(), String> {
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.remove(account);
                Ok(())
            }
            KeyKind::Keychain { .. } => self.change(|m| {
                m.remove(account);
            }),
        }
    }

    /// Writes the whole item, then the copy in memory.
    fn change(&self, f: impl FnOnce(&mut HashMap<String, String>)) -> Result<(), String> {
        let KeyKind::Keychain { service, state, .. } = &self.kind else { return Ok(()) };
        let mut keys = self.loaded().map_err(|_| "钥匙串没有允许读取 Key，这次没有保存。".to_string())?;
        f(&mut keys);
        write_all(service, &keys)?;
        *state.lock().map_err(err)? = Loaded::Ok(keys);
        Ok(())
    }
}

/// The one item; or, when there is none yet, the old items (each read, then deleted where allowed).
fn read_all(service: &str) -> Result<HashMap<String, String>, String> {
    if let Some(raw) = keychain::read(service, KEYS_ACCOUNT)? {
        return serde_json::from_slice(&raw).map_err(|_| "钥匙串里的 Key 读不出来，请重新保存。".to_string());
    }
    let old = keychain::accounts(service);
    if old.is_empty() {
        return Ok(HashMap::new());
    }
    let mut keys = HashMap::new();
    for account in &old {
        if let Ok(Some(raw)) = keychain::read(service, account) {
            if let Some(k) = String::from_utf8(raw).ok().and_then(|k| checked_key(&k).ok()) {
                keys.insert(account.clone(), k);
            }
        }
    }
    write_all(service, &keys)?;
    for account in old.iter().filter(|a| keys.contains_key(*a)) {
        keychain::remove(service, account);
    }
    Ok(keys)
}

fn write_all(service: &str, keys: &HashMap<String, String>) -> Result<(), String> {
    let raw = zeroize::Zeroizing::new(serde_json::to_vec(keys).map_err(err)?);
    keychain::write(service, KEYS_ACCOUNT, &raw).map_err(|_| "无法将 API Key 保存到钥匙串。".to_string())
}

#[cfg(not(test))]
mod keychain {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password_options, PasswordOptions};

    const NOT_FOUND: i32 = -25300; // errSecItemNotFound

    /// The item's bytes (None: no such item). May show the system prompt; turned down → Err.
    pub fn read(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        match get_generic_password(service, account) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(_) => Err("钥匙串没有允许读取 Key。".into()),
        }
    }

    /// Accounts under `service` other than the one item (attributes only: no prompt).
    pub fn accounts(service: &str) -> Vec<String> {
        let found = ItemSearchOptions::new().class(ItemClass::generic_password()).service(service).load_attributes(true).limit(Limit::All).search().unwrap_or_default();
        found
            .into_iter()
            .filter_map(|r| match r {
                SearchResult::Dict(_) => r.simplify_dict().and_then(|d| d.get("acct").cloned()),
                _ => None,
            })
            .filter(|a| a != super::KEYS_ACCOUNT)
            .collect()
    }

    /// Adds the item or updates it. If this build may not change it yet, reads it first (the
    /// prompt that lets this build in) and tries once more.
    pub fn write(service: &str, account: &str, data: &[u8]) -> Result<(), String> {
        let put = || {
            let mut o = PasswordOptions::new_generic_password(service, account);
            o.set_label(service);
            set_generic_password_options(data, o)
        };
        if put().is_ok() {
            return Ok(());
        }
        let _ = get_generic_password(service, account);
        put().map_err(|e| e.to_string())
    }

    /// Best effort: an item another build wrote may refuse (the user deletes it in 钥匙串访问).
    pub fn remove(service: &str, account: &str) {
        let _ = delete_generic_password(service, account);
    }
}

/// Unit tests never touch the real keychain: an in-memory one per service.
#[cfg(test)]
mod keychain {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub static ITEMS: Mutex<Vec<(String, String, Vec<u8>)>> = Mutex::new(Vec::new());
    /// Accounts whose read is turned down.
    pub static DENIED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub fn read(service: &str, account: &str) -> Result<Option<Vec<u8>>, String> {
        if DENIED.lock().unwrap().iter().any(|a| a == account) {
            return Err("钥匙串没有允许读取 Key。".into());
        }
        Ok(ITEMS.lock().unwrap().iter().find(|(s, a, _)| s == service && a == account).map(|(_, _, d)| d.clone()))
    }
    pub fn accounts(service: &str) -> Vec<String> {
        ITEMS.lock().unwrap().iter().filter(|(s, a, _)| s == service && a != super::KEYS_ACCOUNT).map(|(_, a, _)| a.clone()).collect()
    }
    pub fn write(service: &str, account: &str, data: &[u8]) -> Result<(), String> {
        let mut items = ITEMS.lock().unwrap();
        items.retain(|(s, a, _)| !(s == service && a == account));
        items.push((service.into(), account.into(), data.to_vec()));
        Ok(())
    }
    pub fn remove(service: &str, account: &str) {
        ITEMS.lock().unwrap().retain(|(s, a, _)| !(s == service && a == account));
    }
    #[allow(dead_code)]
    pub fn of(service: &str) -> HashMap<String, Vec<u8>> {
        ITEMS.lock().unwrap().iter().filter(|(s, ..)| s == service).map(|(_, a, d)| (a.clone(), d.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youzi::store::tests::memory_db;

    #[test]
    fn efforts_are_the_apis_words_and_the_default_is_one_of_them() {
        let base = CloudModel { id: "m".into(), name: "M".into(), api_model: "m".into(), service: Service::Anthropic, context_window: 200_000, max_output_tokens: 8192, thinking: Think::Switch, efforts: vec![" Low ".into(), "max".into(), " ".into()], default_effort: Some(" MAX ".into()) };
        let v = base.validated().unwrap();
        assert_eq!((v.efforts.clone(), v.default_effort.as_deref()), (vec!["low".to_string(), "max".to_string()], Some("max")));
        // a default not in the list goes; a model that can't think has none
        assert_eq!(CloudModel { default_effort: Some("xhigh".into()), ..base.clone() }.validated().unwrap().default_effort, None);
        assert!(CloudModel { thinking: Think::Never, ..base.clone() }.validated().unwrap().efforts.is_empty());
        for bad in [vec!["low".to_string(), "low".to_string()], vec!["很高".to_string()], vec!["off".to_string()], vec!["x".repeat(21)], (0..13).map(|i| format!("l{i}")).collect()] {
            assert_eq!(CloudModel { efforts: bad, ..base.clone() }.validated().unwrap_err(), BAD_EFFORTS);
        }
        // saved before these existed
        let old: CloudModel = serde_json::from_str(r#"{"id":"o","name":"O","apiModel":"o","service":"openAI","contextWindow":200000,"maxOutputTokens":8192,"thinking":"switch"}"#).unwrap();
        assert!(old.efforts.is_empty() && old.default_effort.is_none());
        assert_eq!(base.effort(Some("max")).as_deref(), Some("max"));
    }

    #[test]
    fn migration_32_lists_each_saved_models_levels() {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::configure(&c).unwrap();
        crate::db::migrate_with(&mut c, &crate::youzi::store::MIGRATIONS[..31], None).unwrap();
        let m = |id: &str, service: &str, thinking: &str| format!(r#"{{"id":"{id}","name":"{id}","apiModel":"{id}","service":"{service}","contextWindow":200000,"maxOutputTokens":8192,"thinking":"{thinking}"}}"#);
        let saved = format!("[{},{},{},{},{}]", m("ds", "deepSeek", "switch"), m("h", "anthropic", "switch"), m("or", "openRouter", "always"), m("q", "aliyun", "switch"), m("n", "openAI", "never"));
        c.execute("INSERT INTO preferences (key, value) VALUES ('cloud.models', ?1)", [saved]).unwrap();
        crate::db::migrate_with(&mut c, crate::youzi::store::MIGRATIONS, None).unwrap();
        let list = models(&c).unwrap();
        let levels = |id: &str| list.iter().find(|m| m.id == id).unwrap().efforts.join(",");
        assert_eq!((levels("ds"), levels("h"), levels("or"), levels("q"), levels("n")), ("low,high,max".to_string(), "low,medium,high,xhigh,max".to_string(), "low,medium,high".to_string(), String::new(), String::new()));
        assert_eq!(list[1].thinking, Think::Switch);
        assert!(list.iter().all(|m| m.default_effort.is_none()));
        c.query_row("SELECT effort FROM sessions LIMIT 0", [], |_| Ok(())).ok();
    }

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
        let base = CloudModel { id: String::new(), name: " GPT ".into(), api_model: "gpt-5".into(), service: Service::OpenAi, context_window: 200_000, max_output_tokens: 8192, thinking: Think::Switch, efforts: vec![], default_effort: None };
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
    fn service_codes_match_youzi() {
        assert_eq!(serde_json::to_string(&Service::OpenAi).unwrap(), "\"openAI\"");
        assert_eq!((serde_json::to_string(&Service::Aliyun).unwrap(), Service::Aliyun.code(), Service::Aliyun.name()), ("\"aliyun\"".to_string(), "aliyun", "Aliyun"));
        assert_eq!(serde_json::to_string(&Level::word("high")).unwrap(), "\"high\"");
        assert_eq!((serde_json::from_str::<Level>("\"high\"").unwrap().effort().as_deref(), Level::default().thinking(), Level::on().effort()), (Some("high"), false, None));
        assert_eq!((Level::off().name().as_str(), Level::on().name().as_str(), Level::word("max").name().as_str()), ("不思考", "思考", "思考 · max"));
        let m: CloudModel = serde_json::from_str(r#"{"id":"x","name":"X","apiModel":"x","service":"openRouter","contextWindow":131072,"maxOutputTokens":8192}"#).unwrap();
        assert_eq!(m.service, Service::OpenRouter);
    }

    #[test]
    fn memory_keys() {
        let k = KeyStore::memory();
        let ds = Service::DeepSeek.code();
        assert!(!k.has(ds));
        assert!(k.read(ds).unwrap_err().contains("还没有保存"));
        assert!(k.save(ds, " bad key ").is_err());
        k.save(ds, "  sk-abc123 ").unwrap();
        assert_eq!(k.read(ds).unwrap(), "sk-abc123");
        k.save("tavily", "tvly-1").unwrap(); // a skill's key beside it
        k.remove(ds).unwrap();
        assert!(!k.has(ds) && k.has("tavily"));
    }

    /// The keychain holds one item; the old one-per-key items move into it once (those read go).
    #[test]
    fn keychain_keys_are_one_item() {
        let k = KeyStore::keychain();
        let KeyKind::Keychain { service, .. } = &k.kind else { unreachable!() };
        let svc = service.clone();
        for (a, v) in [("deepSeek", "sk-1"), ("tavily", "tvly-2"), ("google", "g-3")] {
            keychain::write(&svc, a, v.as_bytes()).unwrap();
        }
        keychain::DENIED.lock().unwrap().push("google".into());
        assert!(!k.has("deepSeek")); // nothing read yet, and has() never reads
        k.load();
        assert!(k.has("deepSeek") && k.has("tavily") && !k.has("google"));
        let left = keychain::of(&svc);
        assert_eq!({ let mut v: Vec<_> = left.keys().cloned().collect(); v.sort(); v }, ["google", "keys"]); // the turned-down one stays
        let one: HashMap<String, String> = serde_json::from_slice(&left["keys"]).unwrap();
        assert_eq!((one["deepSeek"].as_str(), one["tavily"].as_str(), one.len()), ("sk-1", "tvly-2", 2));
        k.save("openAI", " sk-o ").unwrap();
        k.remove("tavily").unwrap();
        let one: HashMap<String, String> = serde_json::from_slice(&keychain::of(&svc)["keys"]).unwrap();
        assert_eq!(one.keys().map(String::as_str).collect::<HashSet<_>>(), HashSet::from(["deepSeek", "openAI"]));
        // a later launch reads the one item only
        let again = KeyStore::keychain();
        assert_eq!(again.read("openAI").unwrap(), "sk-o");
        assert!(again.read("tavily").unwrap_err().contains("还没有保存"));
        // turned down: has() stays false, a save is refused rather than overwriting what's there
        keychain::DENIED.lock().unwrap().push("keys".into());
        let denied = KeyStore::keychain();
        denied.load();
        assert!(!denied.has("deepSeek") && denied.save("aliyun", "sk-a").is_err());
        keychain::DENIED.lock().unwrap().clear();
        assert_eq!(denied.read("deepSeek").unwrap(), "sk-1"); // retried on the next read
        assert!(denied.has("deepSeek"));
        keychain::ITEMS.lock().unwrap().retain(|(s, ..)| s != &svc);
    }
}
