//! 芝麻开门 (passwords). Rules (all here):
//! - Setup: a random 256-bit vault key; the master password (≥ 8 chars, never stored anywhere) →
//!   Argon2id → the key that wraps it (vault_meta). Changing it needs the old one; there is no reset.
//! - Entries: all typed fields are one encrypted JSON blob (XChaCha20-Poly1305). The DB, backups and
//!   anything reading files (agents included) see ciphertext only. Not in data files (export/import),
//!   and never to be exposed as an AI tool.
//! - Unlock: master password, or Touch ID → the keychain copy of the vault key. Touch ID only when
//!   biometrics are available, the copy was saved by this very build (secure.rs), Touch ID hasn't
//!   failed since the last master unlock, and the master password was typed within 3 days.
//!   A failed Touch ID (macOS already allows 3 tries per prompt) → master password required.
//! - Open = the vault key in memory. Locks after 5 minutes without use (wall clock, so sleep counts;
//!   status checks don't count as use), on 锁上, on restore. vault_meta is app state: not undoable.

use super::common::{self, err};
use crate::{secure, AppState};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key as AeadKey, XChaCha20Poly1305, XNonce};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use tauri::State;
use zeroize::Zeroizing;

pub const IDLE: Duration = Duration::from_secs(5 * 60);
pub const MASTER_DAYS: f64 = 3.0;
pub const MIN_PASSWORD: usize = 8;
pub const CLIPBOARD_SECS: u64 = 30;
pub const LOCKED: &str = "芝麻开门已经锁上了";
const AD_KEY: &[u8] = b"pomelo-vault-key";
const AD_CHECK: &[u8] = b"pomelo-vault-check";
const AD_ITEM: &[u8] = b"pomelo-vault-item";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";
/// Argon2id (memory KiB, iterations, lanes): 256 MiB × 3 ≈ 0.3 s per try on an M3, which is what an
/// offline guesser with a copied database pays too. Stored per vault, so it can be raised later.
/// Tests use the minimum.
#[cfg(not(test))]
const KDF: (u32, u32, u32) = (256 * 1024, 3, 1);
#[cfg(test)]
const KDF: (u32, u32, u32) = (8, 1, 1);

type Key = Zeroizing<[u8; 32]>;

// ---------- crypto ----------

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system random source");
    b
}

/// nonce ‖ ciphertext.
fn seal(key: &[u8; 32], msg: &[u8], aad: &[u8]) -> Vec<u8> {
    let nonce = random::<24>();
    let ct = XChaCha20Poly1305::new(AeadKey::from_slice(key))
        .encrypt(XNonce::from_slice(&nonce), Payload { msg, aad })
        .expect("encryption");
    [nonce.as_slice(), &ct].concat()
}

fn unseal(key: &[u8; 32], blob: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if blob.len() < 24 {
        return None;
    }
    let (nonce, ct) = blob.split_at(24);
    XChaCha20Poly1305::new(AeadKey::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .ok()
        .map(Zeroizing::new)
}

fn as_key(raw: &[u8]) -> Option<Key> {
    let arr: [u8; 32] = raw.try_into().ok()?;
    Some(Zeroizing::new(arr))
}

fn master_key(password: &str, salt: &[u8], (m, t, p): (u32, u32, u32)) -> Result<Key, String> {
    let params = Params::new(m, t, p, Some(32)).map_err(err)?;
    let mut out = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(err)?;
    Ok(out)
}

// ---------- session (the open vault key, in memory only) ----------

#[derive(Default)]
pub struct Session {
    key: Option<Key>,
    seen: Option<SystemTime>,
}

impl Session {
    pub fn lock(&mut self) {
        self.key = None;
        self.seen = None;
    }
    fn open(&mut self, key: Key) {
        self.key = Some(key);
        self.seen = Some(SystemTime::now());
    }
    /// Still open (locks if idle); does not count as use.
    fn peek(&mut self) -> bool {
        let fresh = self.seen.and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|d| d < IDLE);
        if !fresh || self.key.is_none() {
            self.lock();
        }
        self.key.is_some()
    }
    /// The key for a vault action (counts as use).
    fn key(&mut self) -> Result<Key, String> {
        if !self.peek() {
            return Err(LOCKED.into());
        }
        self.seen = Some(SystemTime::now());
        Ok(self.key.clone().expect("open"))
    }
    #[cfg(test)]
    fn idle_for(&mut self, d: Duration) {
        self.seen = self.seen.map(|t| t - d);
    }
}

// ---------- meta + status ----------

struct Meta {
    salt: Vec<u8>,
    kdf: (u32, u32, u32),
    wrapped: Vec<u8>,
    check: Vec<u8>,
    master_age_days: f64,
    touch_build: Option<String>,
    touch_account: Option<String>,
    touch_locked: bool,
}

fn meta(conn: &Connection) -> Result<Option<Meta>, String> {
    conn.query_row(
        "SELECT kdf_salt, kdf_m, kdf_t, kdf_p, wrapped_key, key_check, julianday('now') - julianday(master_at),
           touch_build, touch_account, touch_locked FROM vault_meta WHERE id = 1",
        [],
        |r| {
            Ok(Meta {
                salt: r.get(0)?,
                kdf: (r.get(1)?, r.get(2)?, r.get(3)?),
                wrapped: r.get(4)?,
                check: r.get(5)?,
                master_age_days: r.get(6)?,
                touch_build: r.get(7)?,
                touch_account: r.get(8)?,
                touch_locked: r.get(9)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

fn need_meta(conn: &Connection) -> Result<Meta, String> {
    meta(conn)?.ok_or_else(|| "还没有设置主密码".into())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `new` (no master password yet) · `locked` · `open`
    pub state: &'static str,
    /// Touch ID may be offered now (locked only).
    pub touch: bool,
    /// Why not (locked only): none · failed · expired · updated · first
    pub reason: Option<&'static str>,
}

/// Ok = Touch ID allowed; Err = reason code.
fn touch_rule(m: &Meta) -> Result<(), &'static str> {
    if !secure::bio_available() {
        return Err("none");
    }
    if m.touch_locked {
        return Err("failed");
    }
    if m.master_age_days >= MASTER_DAYS {
        return Err("expired");
    }
    match (&m.touch_account, &m.touch_build) {
        (Some(_), Some(b)) if *b == secure::build_id() => Ok(()),
        (Some(_), _) => Err("updated"),
        _ => Err("first"),
    }
}

pub fn reason_text(code: &str) -> &'static str {
    match code {
        "failed" => "Touch ID 没有通过，请输入主密码",
        "expired" => "已经 3 天没输入主密码了",
        "updated" => "App 更新后，第一次要输入主密码",
        "first" => "这次请输入主密码",
        _ => "这台 Mac 现在用不了 Touch ID",
    }
}

pub fn status(conn: &Connection, s: &mut Session) -> Result<Status, String> {
    let Some(m) = meta(conn)? else {
        s.lock();
        return Ok(Status { state: "new", touch: false, reason: None });
    };
    if s.peek() {
        return Ok(Status { state: "open", touch: false, reason: None });
    }
    let r = touch_rule(&m);
    Ok(Status { state: "locked", touch: r.is_ok(), reason: r.err() })
}

// ---------- master password ----------

fn check_new_password(p: &str) -> Result<(), String> {
    let n = p.chars().count();
    if n < MIN_PASSWORD {
        return Err(format!("主密码至少要 {MIN_PASSWORD} 个字符"));
    }
    if n > 200 {
        return Err("主密码太长了（最多 200 个字符）".into());
    }
    Ok(())
}

fn unwrap_with(m: &Meta, password: &str) -> Result<Key, String> {
    let mk = master_key(password, &m.salt, m.kdf)?;
    unseal(&mk, &m.wrapped, AD_KEY).and_then(|raw| as_key(&raw)).ok_or_else(|| "主密码不对".into())
}

/// (Re)wraps the vault key under `password`; counts as a master unlock (clears a Touch ID failure).
fn write_master(conn: &Connection, vault: &Key, password: &str) -> Result<(), String> {
    let salt = random::<16>();
    let mk = master_key(password, &salt, KDF)?;
    let wrapped = seal(&mk, vault.as_ref(), AD_KEY);
    conn.execute(
        &format!(
            "UPDATE vault_meta SET kdf_salt = ?1, kdf_m = ?2, kdf_t = ?3, kdf_p = ?4, wrapped_key = ?5,
               master_at = {NOW}, touch_locked = 0, updated_at = {NOW} WHERE id = 1"
        ),
        params![salt.as_slice(), KDF.0, KDF.1, KDF.2, wrapped],
    )
    .map_err(err)?;
    Ok(())
}

/// Saves a Touch ID copy for this build (if Touch ID exists), replacing this build's older copy.
/// Best effort: on failure Touch ID simply stays off.
fn save_touch_copy(conn: &Connection, vault: &Key) -> Result<(), String> {
    if !secure::bio_available() {
        return Ok(());
    }
    let build = secure::build_id();
    let account = format!("vault-{}", random::<8>().iter().map(|b| format!("{b:02x}")).collect::<String>());
    if secure::key_save(&account, vault.as_ref()).is_err() {
        return Ok(());
    }
    let m = need_meta(conn)?;
    if let (Some(old), Some(b)) = (&m.touch_account, &m.touch_build) {
        if *b == build {
            secure::key_delete(old);
        }
    }
    conn.execute(
        &format!("UPDATE vault_meta SET touch_build = ?1, touch_account = ?2, updated_at = {NOW} WHERE id = 1"),
        params![build, account],
    )
    .map_err(err)?;
    Ok(())
}

pub fn setup(conn: &Connection, s: &mut Session, password: &str) -> Result<Status, String> {
    if meta(conn)?.is_some() {
        return Err("已经设置过主密码了".into());
    }
    check_new_password(password)?;
    let vault: Key = Zeroizing::new(random::<32>());
    let check = seal(&vault, b"pomelo-vault", AD_CHECK);
    let tx = common::Tx::begin(conn)?;
    tx.execute(
        "INSERT INTO vault_meta (id, kdf_salt, kdf_m, kdf_t, kdf_p, wrapped_key, key_check, master_at) VALUES (1, x'', 0, 0, 0, x'', ?1, '')",
        [check],
    )
    .map_err(err)?;
    write_master(&tx, &vault, password)?;
    tx.commit()?;
    save_touch_copy(conn, &vault)?;
    s.open(vault);
    status(conn, s)
}

pub fn unlock(conn: &Connection, s: &mut Session, password: &str) -> Result<Status, String> {
    let m = need_meta(conn)?;
    let vault = unwrap_with(&m, password)?;
    conn.execute(&format!("UPDATE vault_meta SET master_at = {NOW}, touch_locked = 0, updated_at = {NOW} WHERE id = 1"), [])
        .map_err(err)?;
    if matches!(touch_rule(&need_meta(conn)?), Err("updated" | "first")) {
        save_touch_copy(conn, &vault)?;
    }
    s.open(vault);
    status(conn, s)
}

/// Before the prompt: Touch ID is allowed now?
pub fn touch_allowed(conn: &Connection) -> Result<(), String> {
    touch_rule(&need_meta(conn)?).map_err(|r| reason_text(r).to_string())
}

/// After the prompt. Cancelling leaves things as they were; a failure requires the master password.
pub fn finish_touch(conn: &Connection, s: &mut Session, result: secure::Bio) -> Result<Status, String> {
    use secure::Bio;
    match result {
        Bio::Ok => {
            let m = need_meta(conn)?;
            touch_rule(&m).map_err(|r| reason_text(r).to_string())?;
            let vault = m
                .touch_account
                .as_deref()
                .and_then(|a| secure::key_load(a).ok())
                .and_then(|raw| as_key(&raw))
                .filter(|k| unseal(k, &m.check, AD_CHECK).is_some());
            match vault {
                Some(k) => {
                    s.open(k);
                    status(conn, s)
                }
                None => {
                    conn.execute("UPDATE vault_meta SET touch_build = NULL, touch_account = NULL WHERE id = 1", []).map_err(err)?;
                    Err("Touch ID 的钥匙找不到了，请输入主密码".into())
                }
            }
        }
        Bio::Failed => {
            conn.execute(&format!("UPDATE vault_meta SET touch_locked = 1, updated_at = {NOW} WHERE id = 1"), []).map_err(err)?;
            Err(reason_text("failed").into())
        }
        Bio::Cancelled => status(conn, s),
        Bio::Unavailable => Err(reason_text("none").into()),
    }
}

/// Needs the current master password; the vault key (and so every entry) stays the same.
pub fn change_password(conn: &Connection, s: &mut Session, old: &str, new: &str) -> Result<Status, String> {
    let m = need_meta(conn)?;
    let vault = unwrap_with(&m, old).map_err(|_| "原来的主密码不对".to_string())?;
    check_new_password(new)?;
    if old == new {
        return Err("新主密码和原来的一样".into());
    }
    write_master(conn, &vault, new)?;
    s.open(vault);
    status(conn, s)
}

// ---------- entries ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fields {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Fields {
    fn validated(self) -> Result<Self, String> {
        let f = Fields {
            name: common::require(&self.name, "名称", 100)?,
            link: common::clean(self.link),
            username: common::clean(self.username),
            account: common::clean(self.account),
            // spaces can be part of a password: only an empty one counts as none
            password: self.password.filter(|p| !p.is_empty()),
            note: self.note.map(|n| n.trim_end().to_string()).filter(|n| !n.trim().is_empty()),
        };
        common::check_len(&f.link, "链接", 1000)?;
        common::check_len(&f.username, "用户名", 200)?;
        common::check_len(&f.account, "账号", 200)?;
        common::check_len(&f.password, "密码", 1000)?;
        common::check_len(&f.note, "备注", 5000)?;
        Ok(f)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    #[serde(flatten)]
    pub fields: Fields,
    pub created_at: String,
    pub updated_at: String,
}

/// List row: everything but the password and the note text.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: i64,
    pub name: String,
    pub link: Option<String>,
    pub username: Option<String>,
    pub account: Option<String>,
    pub has_password: bool,
    pub has_note: bool,
    pub updated_at: String,
}

fn decrypt(key: &Key, data: &[u8]) -> Result<Fields, String> {
    let plain = unseal(key, data, AD_ITEM).ok_or("有一条密码解不开，数据可能损坏了")?;
    serde_json::from_slice(&plain).map_err(err)
}

fn encrypt(key: &Key, f: &Fields) -> Result<Vec<u8>, String> {
    let json = Zeroizing::new(serde_json::to_vec(f).map_err(err)?);
    Ok(seal(key, &json, AD_ITEM))
}

fn rows(conn: &Connection, filter: &str, arg: Option<i64>) -> Result<Vec<(i64, Vec<u8>, String, String)>, String> {
    let mut st = conn.prepare(&format!("SELECT id, data, created_at, updated_at FROM vault_items WHERE {filter} ORDER BY id")).map_err(err)?;
    let out = st
        .query_map(rusqlite::params_from_iter(arg), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(out)
}

pub fn list(conn: &Connection, key: &Key) -> Result<Vec<Summary>, String> {
    rows(conn, "1 = 1", None)?
        .into_iter()
        .map(|(id, data, _, updated_at)| {
            let f = decrypt(key, &data)?;
            Ok(Summary {
                id,
                has_password: f.password.is_some(),
                has_note: f.note.is_some(),
                name: f.name,
                link: f.link,
                username: f.username,
                account: f.account,
                updated_at,
            })
        })
        .collect()
}

pub fn get(conn: &Connection, key: &Key, id: i64) -> Result<Entry, String> {
    let (id, data, created_at, updated_at) = rows(conn, "id = ?1", Some(id))?.pop().ok_or("找不到这条密码，可能已经被删除了")?;
    Ok(Entry { id, fields: decrypt(key, &data)?, created_at, updated_at })
}

pub fn create(conn: &Connection, key: &Key, f: Fields) -> Result<Entry, String> {
    let f = f.validated()?;
    conn.execute("INSERT INTO vault_items (data) VALUES (?1)", [encrypt(key, &f)?]).map_err(err)?;
    get(conn, key, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, key: &Key, id: i64, f: Fields) -> Result<Entry, String> {
    let old = get(conn, key, id)?;
    let f = f.validated()?;
    if f == old.fields {
        return Ok(old);
    }
    conn.execute(&format!("UPDATE vault_items SET data = ?1, updated_at = {NOW} WHERE id = ?2"), params![encrypt(key, &f)?, id])
        .map_err(err)?;
    get(conn, key, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), String> {
    if conn.execute("DELETE FROM vault_items WHERE id = ?1", [id]).map_err(err)? == 0 {
        return Err("找不到这条密码，可能已经被删除了".into());
    }
    Ok(())
}

/// The text of one field to copy.
pub fn field(conn: &Connection, key: &Key, id: i64, which: &str) -> Result<String, String> {
    let f = get(conn, key, id)?.fields;
    let v = match which {
        "password" => f.password,
        "username" => f.username,
        "account" => f.account,
        "link" => f.link,
        _ => return Err(format!("没有「{which}」这一项")),
    };
    v.ok_or_else(|| "这一项是空的".into())
}

/// Like Apple's: three groups of six letters, one capital and one digit (`kfwmqa-Pzhtdv-xr4ena`).
pub fn generate() -> String {
    let mut letters: Vec<char> = Vec::with_capacity(18);
    while letters.len() < 18 {
        for b in random::<32>() {
            if b < 234 && letters.len() < 18 {
                letters.push((b'a' + b % 26) as char); // 234 = 9 × 26: no modulo bias
            }
        }
    }
    let pick = |n: usize| loop {
        let b = random::<1>()[0] as usize;
        if b < 256 - 256 % n {
            break b % n;
        }
    };
    let cap = pick(18);
    let digit = loop {
        let d = pick(18);
        if d != cap {
            break d;
        }
    };
    letters[cap] = letters[cap].to_ascii_uppercase();
    letters[digit] = char::from(b'0' + pick(10) as u8);
    letters.chunks(6).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

// ---------- commands (UI only; never an AI tool) ----------

impl AppState {
    pub(crate) fn vault(&self) -> Result<std::sync::MutexGuard<'_, Session>, String> {
        self.vault.lock().map_err(|_| "芝麻开门状态异常，请重启 App".to_string())
    }
    pub(crate) fn vault_lock(&self) {
        if let Ok(mut s) = self.vault.lock() {
            s.lock();
        }
    }
    /// The open vault key (counts as use) or the 已锁上 error.
    fn vault_key(&self) -> Result<Key, String> {
        self.vault()?.key()
    }
}

#[tauri::command]
pub fn vault_status(state: State<AppState>) -> Result<Status, String> {
    state.with_db(|c| status(c, &mut *state.vault()?))
}
#[tauri::command]
pub fn vault_setup(state: State<AppState>, password: String) -> Result<Status, String> {
    let password = Zeroizing::new(password);
    state.with_db(|c| setup(c, &mut *state.vault()?, &password))
}
#[tauri::command]
pub fn vault_unlock(state: State<AppState>, password: String) -> Result<Status, String> {
    let password = Zeroizing::new(password);
    state.with_db(|c| unlock(c, &mut *state.vault()?, &password))
}
#[tauri::command]
pub async fn vault_unlock_touch(state: State<'_, AppState>) -> Result<Status, String> {
    state.with_db(|c| touch_allowed(c))?;
    let bio = tauri::async_runtime::spawn_blocking(|| secure::bio_verify("打开芝麻开门")).await.map_err(err)?;
    state.with_db(|c| finish_touch(c, &mut *state.vault()?, bio))
}
#[tauri::command]
pub fn vault_lock(state: State<AppState>) {
    state.vault_lock();
}
/// Activity elsewhere in the app keeps an open vault open.
#[tauri::command]
pub fn vault_touch(state: State<AppState>) -> Result<bool, String> {
    Ok(state.vault()?.key().is_ok())
}
#[tauri::command]
pub fn vault_change_password(state: State<AppState>, old: String, new: String) -> Result<Status, String> {
    let (old, new) = (Zeroizing::new(old), Zeroizing::new(new));
    state.with_db(|c| change_password(c, &mut *state.vault()?, &old, &new))
}
#[tauri::command]
pub fn vault_list(state: State<AppState>) -> Result<Vec<Summary>, String> {
    let key = state.vault_key()?;
    state.with_db(|c| list(c, &key))
}
#[tauri::command]
pub fn vault_get(state: State<AppState>, id: i64) -> Result<Entry, String> {
    let key = state.vault_key()?;
    state.with_db(|c| get(c, &key, id))
}
#[tauri::command]
pub fn vault_create(state: State<AppState>, fields: Fields) -> Result<Entry, String> {
    let key = state.vault_key()?;
    state.with_undo("添加密码", |c| create(c, &key, fields))
}
#[tauri::command]
pub fn vault_update(state: State<AppState>, id: i64, fields: Fields) -> Result<Entry, String> {
    let key = state.vault_key()?;
    state.with_undo("修改密码", |c| update(c, &key, id, fields))
}
#[tauri::command]
pub fn vault_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    state.vault_key()?;
    state.with_undo("删除密码", |c| delete(c, id))
}
/// Copies one field straight from Rust to the pasteboard (the password never passes through the page).
#[tauri::command]
pub fn vault_copy(state: State<AppState>, id: i64, field: String) -> Result<(), String> {
    let key = state.vault_key()?;
    let text = Zeroizing::new(state.with_db(|c| self::field(c, &key, id, &field))?);
    secure::copy_concealed(&text, Duration::from_secs(CLIPBOARD_SECS));
    Ok(())
}
#[tauri::command]
pub fn vault_generate() -> String {
    generate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::memory_db;
    use crate::secure::fake;
    use crate::secure::Bio;

    const PW: &str = "correct horse";

    fn f(name: &str, password: Option<&str>) -> Fields {
        Fields { name: name.into(), username: Some("me".into()), password: password.map(Into::into), ..Default::default() }
    }

    #[test]
    fn crypto_roundtrip_and_tamper() {
        let k = [7u8; 32];
        let blob = seal(&k, b"hello", AD_ITEM);
        assert_eq!(unseal(&k, &blob, AD_ITEM).unwrap().as_slice(), b"hello");
        assert!(unseal(&k, &blob, AD_KEY).is_none()); // other purpose
        assert!(unseal(&[8u8; 32], &blob, AD_ITEM).is_none());
        let mut bad = blob.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(unseal(&k, &bad, AD_ITEM).is_none());
        assert_ne!(seal(&k, b"hello", AD_ITEM), blob); // fresh nonce
        let g = generate();
        assert_eq!((g.len(), g.matches('-').count()), (20, 2));
        assert_eq!(g.chars().filter(char::is_ascii_uppercase).count(), 1);
        assert_eq!(g.chars().filter(char::is_ascii_digit).count(), 1);
    }

    #[test]
    fn setup_lock_unlock_and_ciphertext_only() {
        let c = memory_db();
        let mut s = Session::default();
        assert_eq!(status(&c, &mut s).unwrap().state, "new");
        assert!(unlock(&c, &mut s, PW).is_err());
        assert!(setup(&c, &mut s, "short").unwrap_err().contains("8"));
        assert_eq!(setup(&c, &mut s, PW).unwrap().state, "open");
        assert!(setup(&c, &mut s, "another one").is_err()); // only once

        let key = s.key().unwrap();
        let e = create(&c, &key, f("  GitHub ", Some(" p@ss word "))).unwrap();
        assert_eq!((e.fields.name.as_str(), e.fields.password.as_deref()), ("GitHub", Some(" p@ss word ")));
        assert!(create(&c, &key, f(" ", None)).is_err());
        // nothing readable in the database
        let blobs: Vec<Vec<u8>> = c.prepare("SELECT data FROM vault_items").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        let hay = String::from_utf8_lossy(&blobs.concat()).to_string();
        assert!(!hay.contains("GitHub") && !hay.contains("p@ss"));

        s.lock();
        assert_eq!(s.key().unwrap_err(), LOCKED);
        assert_eq!(unlock(&c, &mut s, "wrong password").unwrap_err(), "主密码不对");
        assert_eq!(unlock(&c, &mut s, PW).unwrap().state, "open");
        let key = s.key().unwrap();
        let rows = list(&c, &key).unwrap();
        assert_eq!((rows[0].name.as_str(), rows[0].has_password, rows[0].has_note), ("GitHub", true, false));
        let e2 = update(&c, &key, e.id, Fields { note: Some("备用码 123\n".into()), ..e.fields.clone() }).unwrap();
        assert_eq!(e2.fields.note.as_deref(), Some("备用码 123"));
        assert_eq!(field(&c, &key, e.id, "password").unwrap(), " p@ss word ");
        assert!(field(&c, &key, e.id, "account").is_err());
        delete(&c, e.id).unwrap();
        assert!(get(&c, &key, e.id).is_err());
    }

    #[test]
    fn idle_lock_and_status_does_not_count_as_use() {
        let c = memory_db();
        let mut s = Session::default();
        setup(&c, &mut s, PW).unwrap();
        s.idle_for(Duration::from_secs(4 * 60));
        assert_eq!(status(&c, &mut s).unwrap().state, "open");
        s.idle_for(Duration::from_secs(61)); // status above didn't refresh it
        assert_eq!(status(&c, &mut s).unwrap().state, "locked");
        assert!(s.key().is_err());
        unlock(&c, &mut s, PW).unwrap();
        s.idle_for(Duration::from_secs(4 * 60));
        s.key().unwrap(); // use → fresh again
        s.idle_for(Duration::from_secs(4 * 60));
        assert_eq!(status(&c, &mut s).unwrap().state, "open");
    }

    #[test]
    fn touch_id_rules() {
        let c = memory_db();
        let mut s = Session::default();
        setup(&c, &mut s, PW).unwrap();
        let key = s.key().unwrap();
        create(&c, &key, f("A", Some("1"))).unwrap();
        s.lock();
        let st = status(&c, &mut s).unwrap();
        assert_eq!((st.state, st.touch), ("locked", true));
        // Touch ID → the keychain copy opens it
        touch_allowed(&c).unwrap();
        assert_eq!(finish_touch(&c, &mut s, Bio::Ok).unwrap().state, "open");
        assert_eq!(list(&c, &s.key().unwrap()).unwrap().len(), 1);
        // cancelling changes nothing
        s.lock();
        assert_eq!(finish_touch(&c, &mut s, Bio::Cancelled).unwrap(), Status { state: "locked", touch: true, reason: None });
        // a failure → master password first
        assert!(finish_touch(&c, &mut s, Bio::Failed).is_err());
        assert_eq!(status(&c, &mut s).unwrap().reason, Some("failed"));
        assert!(touch_allowed(&c).is_err());
        assert!(finish_touch(&c, &mut s, Bio::Ok).is_err()); // can't sneak past
        unlock(&c, &mut s, PW).unwrap();
        s.lock();
        assert!(status(&c, &mut s).unwrap().touch);
        // 3 days without the master password
        c.execute("UPDATE vault_meta SET master_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-3 days', '-1 minutes')", []).unwrap();
        assert_eq!(status(&c, &mut s).unwrap().reason, Some("expired"));
        c.execute("UPDATE vault_meta SET master_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-2 days', '-23 hours')", []).unwrap();
        assert!(status(&c, &mut s).unwrap().touch);
        // a new build can't use the old copy until the master password is typed, then saves its own
        let old_account: String = c.query_row("SELECT touch_account FROM vault_meta", [], |r| r.get(0)).unwrap();
        fake::BUILD.with_borrow_mut(|b| *b = "build-2".into());
        assert_eq!(status(&c, &mut s).unwrap().reason, Some("updated"));
        unlock(&c, &mut s, PW).unwrap();
        let (account, build): (String, String) = c.query_row("SELECT touch_account, touch_build FROM vault_meta", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_ne!(account, old_account);
        assert_eq!(build, "build-2");
        s.lock();
        assert_eq!(finish_touch(&c, &mut s, Bio::Ok).unwrap().state, "open");
        // a lost keychain copy → master password, then a new copy
        fake::KEYCHAIN.with_borrow_mut(|k| k.clear());
        s.lock();
        assert!(finish_touch(&c, &mut s, Bio::Ok).is_err());
        assert_eq!(status(&c, &mut s).unwrap().reason, Some("first"));
        // no Touch ID on this Mac
        fake::AVAILABLE.set(false);
        assert_eq!(status(&c, &mut s).unwrap(), Status { state: "locked", touch: false, reason: Some("none") });
    }

    #[test]
    fn change_password_keeps_entries() {
        let c = memory_db();
        let mut s = Session::default();
        setup(&c, &mut s, PW).unwrap();
        let e = create(&c, &s.key().unwrap(), f("Mail", Some("x"))).unwrap();
        s.lock();
        assert_eq!(change_password(&c, &mut s, "wrong one", "new password 1").unwrap_err(), "原来的主密码不对");
        assert!(change_password(&c, &mut s, PW, "short").is_err());
        assert!(change_password(&c, &mut s, PW, PW).is_err());
        assert_eq!(change_password(&c, &mut s, PW, "new password 1").unwrap().state, "open");
        s.lock();
        assert!(unlock(&c, &mut s, PW).is_err());
        unlock(&c, &mut s, "new password 1").unwrap();
        assert_eq!(get(&c, &s.key().unwrap(), e.id).unwrap().fields.password.as_deref(), Some("x"));
        // Touch ID copy still fits (same vault key)
        s.lock();
        assert_eq!(finish_touch(&c, &mut s, Bio::Ok).unwrap().state, "open");
    }

    #[test]
    fn copy_goes_to_the_pasteboard_only() {
        let c = memory_db();
        let mut s = Session::default();
        setup(&c, &mut s, PW).unwrap();
        let key = s.key().unwrap();
        let e = create(&c, &key, f("Bank", Some("s3cret"))).unwrap();
        let text = field(&c, &key, e.id, "password").unwrap();
        secure::copy_concealed(&text, Duration::from_secs(CLIPBOARD_SECS));
        assert_eq!(fake::CLIPBOARD.with_borrow(|c| c.clone()), "s3cret");
        assert!(list(&c, &key).unwrap().iter().all(|r| serde_json::to_string(r).unwrap().find("s3cret").is_none()));
    }
}
