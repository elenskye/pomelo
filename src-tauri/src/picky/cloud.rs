//! Cloud models (OpenRouter, DeepSeek, OpenAI), as in the old Picky:
//! - Requests go only to each service's fixed official HTTPS endpoint (provider.rs).
//! - The model list (name, API model name, token limits) lives in picky.db `preferences`
//!   (`cloud.models`; its presence, even `[]`, marks the defaults as seeded — deleted defaults stay
//!   deleted). Keys never enter the database, backups, exports or logs: only the keychain
//!   (service "<data dir name> 小拾", account = service code). The release build is signed with a
//!   fixed self-signed certificate, so an update can still read them.
//! - Mock mode keeps keys in memory.

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
}

impl Service {
    pub const ALL: [Service; 3] = [Service::OpenRouter, Service::DeepSeek, Service::OpenAi];

    pub fn name(self) -> &'static str {
        match self {
            Service::OpenRouter => "OpenRouter",
            Service::DeepSeek => "DeepSeek",
            Service::OpenAi => "OpenAI",
        }
    }
    /// The only place a key is ever sent.
    pub fn base_url(self) -> &'static str {
        match self {
            Service::OpenRouter => "https://openrouter.ai/api/v1",
            Service::DeepSeek => "https://api.deepseek.com/v1",
            Service::OpenAi => "https://api.openai.com/v1",
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Service::OpenRouter => "openRouter",
            Service::DeepSeek => "deepSeek",
            Service::OpenAi => "openAI",
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
    let m = |id: &str, name: &str| CloudModel { id: id.into(), name: name.into(), api_model: id.into(), service: Service::DeepSeek, context_window: 1_048_576, max_output_tokens: 8192 };
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
    present: Mutex<HashMap<Service, bool>>,
}

enum KeyKind {
    Keychain { service: String },
    Memory(Mutex<HashMap<Service, String>>),
}

impl KeyStore {
    pub fn keychain() -> Self {
        KeyStore { kind: KeyKind::Keychain { service: format!("{} 小拾", crate::paths::data_dir_name()) }, present: Mutex::default() }
    }
    pub fn memory() -> Self {
        KeyStore { kind: KeyKind::Memory(Mutex::default()), present: Mutex::default() }
    }

    pub fn has(&self, s: Service) -> bool {
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map(|m| m.contains_key(&s)).unwrap_or(false),
            KeyKind::Keychain { service } => {
                let mut p = self.present.lock().unwrap_or_else(|e| e.into_inner());
                *p.entry(s).or_insert_with(|| keychain::exists(service, s.code()))
            }
        }
    }

    pub fn read(&self, s: Service) -> Result<String, String> {
        let missing = || "尚未保存这个云端接口的 API Key。".to_string();
        match &self.kind {
            KeyKind::Memory(m) => m.lock().map_err(err)?.get(&s).cloned().ok_or_else(missing),
            KeyKind::Keychain { service } => match keychain::read(service, s.code()) {
                Some(bytes) => String::from_utf8(bytes).map_err(|_| "无法读取 API Key，请重新保存。".to_string()),
                None => Err(missing()),
            },
        }
    }

    pub fn save(&self, s: Service, key: &str) -> Result<(), String> {
        let key = checked_key(key)?;
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.insert(s, key);
            }
            KeyKind::Keychain { service } => keychain::save(service, s.code(), &key, &format!("Pomelo 小拾 · {}", s.name()))?,
        }
        self.present.lock().map_err(err)?.insert(s, true);
        Ok(())
    }

    pub fn remove(&self, s: Service) -> Result<(), String> {
        match &self.kind {
            KeyKind::Memory(m) => {
                m.lock().map_err(err)?.remove(&s);
            }
            KeyKind::Keychain { service } => keychain::remove(service, s.code())?,
        }
        self.present.lock().map_err(err)?.insert(s, false);
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
        delete_model(&c, "deepseek-flash").unwrap();
        delete_model(&c, "deepseek-v4-pro").unwrap();
        assert!(models(&c).unwrap().is_empty()); // [] still marks "seeded"
    }

    #[test]
    fn save_validates_and_assigns_ids() {
        let c = memory_db();
        let base = CloudModel { id: String::new(), name: " GPT ".into(), api_model: "gpt-5".into(), service: Service::OpenAi, context_window: 200_000, max_output_tokens: 8192 };
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
        let m: CloudModel = serde_json::from_str(r#"{"id":"x","name":"X","apiModel":"x","service":"openRouter","contextWindow":131072,"maxOutputTokens":8192}"#).unwrap();
        assert_eq!(m.service, Service::OpenRouter);
    }

    #[test]
    fn memory_keys() {
        let k = KeyStore::memory();
        assert!(!k.has(Service::DeepSeek));
        assert!(k.read(Service::DeepSeek).unwrap_err().contains("尚未保存"));
        assert!(k.save(Service::DeepSeek, " bad key ").is_err());
        k.save(Service::DeepSeek, "  sk-abc123 ").unwrap();
        assert_eq!(k.read(Service::DeepSeek).unwrap(), "sk-abc123");
        k.remove(Service::DeepSeek).unwrap();
        assert!(!k.has(Service::DeepSeek));
    }
}
