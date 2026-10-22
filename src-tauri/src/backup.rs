//! Snapshot backups: `backups/<store>-YYYYMMDD-HHMMSS-<label>[-n].db`, written with `VACUUM INTO`
//! (consistent single-file copy, WAL-safe). The file name is the only metadata (store + time + label).
//! Stores: `pomelo` (pomelo.db, life data) and `picky` (picky.db, 小拾); each is backed up, pruned and
//! restored on its own. Labels: `daily` (auto, first launch/focus of the day; pruned), `manual`,
//! `before-vN` (migration), `before-import`, `before-picky-import`, `before-restore`. `manual` is never
//! pruned; the safety copies (升级前 = any `before-vN`, 导入前, 恢复前) keep the newest KEEP_SAFETY of each
//! kind per store, pruned with the daily ones.
//! Restore copies a snapshot into the live connection via the SQLite backup API (atomic), then migrates.

use crate::{db, AppState};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

pub const DAILY: &str = "daily";
pub const MANUAL: &str = "manual";
pub const BEFORE_RESTORE: &str = "before-restore";
/// Safety copies kept of each kind (升级前, 导入前, 恢复前), per store.
pub const KEEP_SAFETY: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Pomelo,
    Picky,
}

impl Store {
    pub const ALL: [Store; 2] = [Store::Pomelo, Store::Picky];

    pub fn prefix(self) -> &'static str {
        match self {
            Store::Pomelo => "pomelo",
            Store::Picky => "picky",
        }
    }

    /// Retention for `daily`: newest N are kept, plus the newest one of every month (forever).
    pub fn keep_daily(self) -> usize {
        match self {
            Store::Pomelo => 30,
            Store::Picky => 7,
        }
    }

    pub fn migrations(self) -> &'static [db::Migration] {
        match self {
            Store::Pomelo => db::MIGRATIONS,
            Store::Picky => crate::picky::store::MIGRATIONS,
        }
    }
}

/// Writes `pomelo-YYYYMMDD-HHMMSS-<label>.db` into `dir`. Never overwrites.
pub fn snapshot(conn: &Connection, dir: &Path, label: &str) -> Result<PathBuf, String> {
    snapshot_of(Store::Pomelo, conn, dir, label)
}

pub fn snapshot_of(store: Store, conn: &Connection, dir: &Path, label: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("无法创建备份目录：{e}"))?;
    let stamp: String = conn
        .query_row("SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let p = store.prefix();
    let mut path = dir.join(format!("{p}-{stamp}-{label}.db"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("{p}-{stamp}-{label}-{n}.db"));
    }
    let target = path.to_str().ok_or("备份路径包含无法识别的字符")?;
    conn.execute("VACUUM INTO ?1", [target])
        .map_err(|e| format!("备份失败：{e}"))?;
    Ok(path)
}

// ---------- listing ----------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Name {
    store: Store,
    /// `YYYYMMDD-HHMMSS` (local time)
    stamp: String,
    label: String,
    /// Collision counter (1 = none).
    n: u32,
}

impl Name {
    fn sort_key(&self) -> (&str, u32) {
        (&self.stamp, self.n)
    }
}

fn parse_name(file: &str) -> Option<Name> {
    let (store, rest) = Store::ALL.iter().find_map(|s| Some((*s, file.strip_prefix(s.prefix())?.strip_prefix('-')?)))?;
    let rest = rest.strip_suffix(".db")?;
    let (stamp, label) = (rest.get(..15)?, rest.get(15..)?.strip_prefix('-')?);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !(digits(&stamp[..8]) && stamp.as_bytes()[8] == b'-' && digits(&stamp[9..])) || label.is_empty() {
        return None;
    }
    // labels never end in `-<digits>`, so a trailing one is the collision counter
    let (label, n) = match label.rsplit_once('-') {
        Some((l, n)) if digits(n) && !l.is_empty() => (l, n.parse().ok()?),
        _ => (label, 1),
    };
    Some(Name { store, stamp: stamp.into(), label: label.into(), n })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file_name: String,
    /// Which database: `pomelo` (life data) or `picky` (小拾).
    pub store: Store,
    /// Raw label (`daily`, `manual`, `before-v7`, ...); UI maps to Chinese.
    pub label: String,
    /// `YYYY-MM-DD HH:MM:SS`, local time.
    pub created_at: String,
    pub size: u64,
}

fn scan(dir: &Path) -> Result<Vec<(Name, PathBuf)>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("读不了备份目录：{e}")),
    };
    let mut out: Vec<(Name, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| Some((parse_name(e.file_name().to_str()?)?, e.path())))
        .collect();
    out.sort_by(|a, b| b.0.sort_key().cmp(&a.0.sort_key())); // newest first
    Ok(out)
}

fn info(name: &Name, path: &Path) -> BackupInfo {
    let s = &name.stamp;
    BackupInfo {
        file_name: path.file_name().and_then(|f| f.to_str()).unwrap_or_default().into(),
        store: name.store,
        label: name.label.clone(),
        created_at: format!("{}-{}-{} {}:{}:{}", &s[..4], &s[4..6], &s[6..8], &s[9..11], &s[11..13], &s[13..15]),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    }
}

/// Both stores, newest first.
pub fn list(dir: &Path) -> Result<Vec<BackupInfo>, String> {
    Ok(scan(dir)?.iter().map(|(n, p)| info(n, p)).collect())
}

fn info_of(path: &Path) -> Result<BackupInfo, String> {
    let file = path.file_name().and_then(|f| f.to_str()).unwrap_or_default();
    let name = parse_name(file).ok_or_else(|| format!("备份文件名不对：{file}"))?;
    Ok(info(&name, path))
}

// ---------- daily + retention ----------

/// Deletes `store`'s `daily` snapshots beyond the newest `keep`, except the newest one of each month.
/// Returns the deleted paths.
pub fn prune_daily(dir: &Path, store: Store, keep: usize) -> Result<Vec<PathBuf>, String> {
    let daily: Vec<(Name, PathBuf)> = scan(dir)?.into_iter().filter(|(n, _)| n.store == store && n.label == DAILY).collect();
    let mut months = HashSet::new();
    let mut deleted = Vec::new();
    for (i, (name, path)) in daily.iter().enumerate() {
        let newest_of_month = months.insert(name.stamp[..6].to_string()); // newest-first order
        if i < keep || newest_of_month {
            continue;
        }
        std::fs::remove_file(path).map_err(|e| format!("清理旧备份失败：{e}"))?;
        deleted.push(path.clone());
    }
    Ok(deleted)
}

/// The kind of an automatic safety copy (None: daily, manual, unknown labels).
fn safety_kind(label: &str) -> Option<&'static str> {
    match label {
        "before-import" | "before-picky-import" => Some("import"),
        BEFORE_RESTORE => Some("restore"),
        _ => label.strip_prefix("before-v").filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).map(|_| "upgrade"),
    }
}

/// Deletes `store`'s safety copies beyond the newest `keep` of each kind. Returns the deleted paths.
pub fn prune_safety(dir: &Path, store: Store, keep: usize) -> Result<Vec<PathBuf>, String> {
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut deleted = Vec::new();
    for (name, path) in scan(dir)?.into_iter().filter(|(n, _)| n.store == store) {
        let Some(kind) = safety_kind(&name.label) else { continue };
        let n = seen.entry(kind).or_default();
        *n += 1;
        if *n > keep {
            std::fs::remove_file(&path).map_err(|e| format!("清理旧备份失败：{e}"))?;
            deleted.push(path);
        }
    }
    Ok(deleted)
}

/// Takes today's `daily` snapshot of `pomelo.db` unless one exists, then prunes.
pub fn daily(conn: &Connection, dir: &Path) -> Result<Option<PathBuf>, String> {
    daily_of(Store::Pomelo, conn, dir)
}

/// Takes today's `daily` snapshot of `store` unless one exists, then prunes. Returns the new snapshot.
pub fn daily_of(store: Store, conn: &Connection, dir: &Path) -> Result<Option<PathBuf>, String> {
    let today: String = conn
        .query_row("SELECT strftime('%Y%m%d', 'now', 'localtime')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if scan(dir)?.iter().any(|(n, _)| n.store == store && n.label == DAILY && n.stamp.starts_with(&today)) {
        return Ok(None);
    }
    let path = snapshot_of(store, conn, dir, DAILY)?;
    prune_daily(dir, store, store.keep_daily())?;
    prune_safety(dir, store, KEEP_SAFETY)?;
    Ok(Some(path))
}

// ---------- restore ----------

/// Resolves a file name from `list` to its store and a path inside `dir` (no traversal, must exist).
fn resolve(dir: &Path, file_name: &str) -> Result<(Store, PathBuf), String> {
    let name = (!file_name.contains(['/', '\\'])).then(|| parse_name(file_name)).flatten();
    let Some(name) = name else {
        return Err("这不是 Pomelo 的备份文件".into());
    };
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("找不到这份备份，可能已经被删除了".into());
    }
    Ok((name.store, path))
}

/// Opens a snapshot read-only and checks it is an intact database of `store` this app can open.
fn open_snapshot(store: Store, path: &Path) -> Result<Connection, String> {
    let bad = |e: String| format!("这份备份不能用：{e}");
    let src = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| bad(e.to_string()))?;
    let check: String = src.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| bad(e.to_string()))?;
    if check != "ok" {
        return Err(bad("文件已损坏".into()));
    }
    let v = db::user_version(&src).map_err(bad)?;
    let latest = store.migrations().last().map_or(0, |m| m.version);
    if v < 1 {
        return Err(bad("里面没有数据".into()));
    }
    if v > latest {
        return Err(bad(format!("它的数据库版本（{v}）比这个 App 新，请用新版 App 恢复")));
    }
    Ok(src)
}

/// Replaces the whole content of `dst` with `src` (one step = one atomic write transaction).
fn copy_into(src: &Connection, dst: &mut Connection) -> Result<(), String> {
    let b = rusqlite::backup::Backup::new(src, dst).map_err(|e| format!("恢复失败：{e}"))?;
    match b.step(-1).map_err(|e| format!("恢复失败：{e}"))? {
        rusqlite::backup::StepResult::Done => Ok(()),
        _ => Err("恢复失败：数据库正忙，请稍后再试".into()),
    }
}

/// Which store a backup file belongs to (for picking the connection before `restore`).
pub fn store_of(dir: &Path, file_name: &str) -> Result<Store, String> {
    resolve(dir, file_name).map(|(s, _)| s)
}

/// Snapshots current data (`before-restore`), then replaces it with `file_name` and migrates it
/// to the current schema. If migrating fails, the current data is put back.
/// `conn` must be the connection of the file's store. Returns the safety snapshot.
pub fn restore(conn: &mut Connection, dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let (store, path) = resolve(dir, file_name)?;
    let src = open_snapshot(store, &path)?;
    let safety = snapshot_of(store, conn, dir, BEFORE_RESTORE)?;
    copy_into(&src, conn)?;
    if let Err(e) = db::migrate_as(conn, store.migrations(), None, store) {
        let back = open_snapshot(store, &safety).and_then(|s| copy_into(&s, conn));
        return Err(match back {
            Ok(()) => format!("这份备份升级到新版本时出错，已还原成恢复前的数据：{e}"),
            Err(e2) => format!("恢复失败：{e}；还原也失败了：{e2}。恢复前的数据在 {}", safety.display()),
        });
    }
    Ok(safety)
}

// ---------- commands ----------

#[tauri::command]
pub fn backups_list(state: State<AppState>) -> Result<Vec<BackupInfo>, String> {
    list(&state.paths.backups)
}

/// Backs up both databases (小拾 only when it opened). Returns the life-data snapshot.
#[tauri::command]
pub fn backup_now(state: State<AppState>) -> Result<BackupInfo, String> {
    let dir = state.paths.backups.clone();
    let main = info_of(&state.with_db(|c| snapshot(c, &dir, MANUAL))?)?;
    if state.picky.is_open() {
        state.picky.with_db(|c| snapshot_of(Store::Picky, c, &dir, MANUAL))?;
    }
    Ok(main)
}

/// Called by the UI on window focus; no-op if today's daily snapshots exist. Returns the
/// life-data snapshot if one was taken.
#[tauri::command]
pub fn backup_daily(state: State<AppState>) -> Result<Option<BackupInfo>, String> {
    let dir = state.paths.backups.clone();
    let main = state.with_db(|c| daily(c, &dir))?;
    if state.picky.is_open() {
        state.picky.with_db(|c| daily_of(Store::Picky, c, &dir))?;
    }
    main.map(|p| info_of(&p)).transpose()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// Snapshot of the data that was replaced.
    pub safety: BackupInfo,
}

#[tauri::command]
pub fn backup_restore(state: State<AppState>, file_name: String) -> Result<RestoreResult, String> {
    let dir = state.paths.backups.clone();
    let safety = match store_of(&dir, &file_name)? {
        Store::Pomelo => {
            let s = state.with_db(|c| restore(c, &dir, &file_name))?;
            state.vault_lock(); // the restored vault may have another master password
            s
        }
        Store::Picky => state.picky.with_db(|c| restore(c, &dir, &file_name))?,
    };
    state.clear_history();
    Ok(RestoreResult { safety: info_of(&safety)? })
}

#[tauri::command]
pub fn reveal_backups_dir(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    std::fs::create_dir_all(&state.paths.backups).map_err(|e| format!("无法创建备份目录：{e}"))?;
    app.opener()
        .open_path(state.paths.backups.display().to_string(), None::<&str>)
        .map_err(|e| format!("无法打开备份目录：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    fn file_db(dir: &Path) -> Connection {
        let mut c = db::open(&dir.join("pomelo.db")).unwrap();
        db::migrate(&mut c, db::MIGRATIONS, &dir.join("backups")).unwrap();
        c
    }

    fn titles(c: &Connection) -> Vec<String> {
        let mut st = c.prepare("SELECT title FROM books ORDER BY id").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn names_parse() {
        let n = parse_name("pomelo-20260929-140305-before-v7.db").unwrap();
        assert_eq!((n.stamp.as_str(), n.label.as_str(), n.n), ("20260929-140305", "before-v7", 1));
        let n = parse_name("pomelo-20260929-140305-daily-3.db").unwrap();
        assert_eq!((n.label.as_str(), n.n), ("daily", 3));
        let n = parse_name("pomelo-20260929-140305-before-v7-2.db").unwrap();
        assert_eq!((n.label.as_str(), n.n), ("before-v7", 2));
        for bad in ["pomelo-2026-daily.db", "pomelo-20260929-140305.db", "x-20260929-140305-daily.db", "pomelo-20260929-140305-daily.db-wal", "pomelo-2026092a-140305-daily.db"] {
            assert!(parse_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn picky_store_is_named_listed_pruned_and_restored_on_its_own() {
        let n = parse_name("picky-20261010-090000-daily-2.db").unwrap();
        assert_eq!((n.store, n.label.as_str(), n.n), (Store::Picky, "daily", 2));
        assert!(parse_name("pickyx-20261010-090000-daily.db").is_none());

        let dir = temp_dir("backup-two-stores");
        for d in 1..=10 {
            std::fs::write(dir.join(format!("pomelo-202610{d:02}-080000-daily.db")), b"x").unwrap();
            std::fs::write(dir.join(format!("picky-202610{d:02}-080000-daily.db")), b"x").unwrap();
        }
        prune_daily(&dir, Store::Picky, Store::Picky.keep_daily()).unwrap();
        let l = list(&dir).unwrap();
        assert_eq!(l.iter().filter(|b| b.store == Store::Picky).count(), 7);
        assert_eq!(l.iter().filter(|b| b.store == Store::Pomelo).count(), 10); // untouched

        // restore a picky snapshot into a picky connection; pomelo.db is not involved
        let b = dir.join("backups");
        let mut c = crate::picky::store::open(&dir.join("picky.db"), &b).unwrap();
        c.execute("INSERT INTO sessions (title) VALUES ('旧')", []).unwrap();
        let snap = snapshot_of(Store::Picky, &c, &b, MANUAL).unwrap();
        c.execute("DELETE FROM sessions", []).unwrap();
        let name = snap.file_name().unwrap().to_str().unwrap();
        assert_eq!(store_of(&b, name).unwrap(), Store::Picky);
        let safety = restore(&mut c, &b, name).unwrap();
        let t: String = c.query_row("SELECT title FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(t, "旧");
        assert!(safety.file_name().unwrap().to_str().unwrap().starts_with("picky-"));
        assert!(daily_of(Store::Picky, &c, &b).unwrap().is_some());
        assert!(daily_of(Store::Picky, &c, &b).unwrap().is_none());
    }

    #[test]
    fn list_newest_first_ignores_strangers() {
        let dir = temp_dir("backup-list");
        for f in ["pomelo-20260101-090000-daily.db", "pomelo-20260102-090000-manual.db", "pomelo-20260102-090000-manual-2.db", "notes.txt", "pomelo.db"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let l = list(&dir).unwrap();
        assert_eq!(
            l.iter().map(|b| b.file_name.as_str()).collect::<Vec<_>>(),
            ["pomelo-20260102-090000-manual-2.db", "pomelo-20260102-090000-manual.db", "pomelo-20260101-090000-daily.db"]
        );
        assert_eq!((l[2].created_at.as_str(), l[2].label.as_str(), l[2].size), ("2026-01-01 09:00:00", "daily", 1));
        assert!(list(&dir.join("missing")).unwrap().is_empty());
    }

    #[test]
    fn prune_keeps_recent_and_monthly() {
        let dir = temp_dir("backup-prune");
        // 90 consecutive days Jul 1 – Sep 28, plus other labels that must survive
        let mut days = vec![];
        for (m, n) in [(7, 31), (8, 31), (9, 28)] {
            for d in 1..=n {
                days.push(format!("2026{m:02}{d:02}"));
            }
        }
        for d in &days {
            std::fs::write(dir.join(format!("pomelo-{d}-080000-daily.db")), b"x").unwrap();
        }
        std::fs::write(dir.join("pomelo-20260701-090000-manual.db"), b"x").unwrap();
        std::fs::write(dir.join("pomelo-20260701-090000-before-v7.db"), b"x").unwrap();
        let deleted = prune_daily(&dir, Store::Pomelo, 30).unwrap();
        let left: Vec<String> = list(&dir).unwrap().into_iter().filter(|b| b.label == DAILY).map(|b| b.created_at[..10].to_string()).collect();
        // newest 30 = Aug 30 – Sep 28 (already holds Aug's last day); July keeps only Jul 31
        assert_eq!(left.len(), 31);
        assert_eq!((left[0].as_str(), left[29].as_str(), left[30].as_str()), ("2026-09-28", "2026-08-30", "2026-07-31"));
        assert_eq!(deleted.len(), 90 - 31);
        assert_eq!(list(&dir).unwrap().len(), 33);
        assert!(prune_daily(&dir, Store::Pomelo, 30).unwrap().is_empty()); // idempotent
    }

    #[test]
    fn safety_copies_keep_the_newest_three_of_each_kind() {
        let dir = temp_dir("backup-safety");
        let mut files = vec![];
        for d in 1..=6 {
            files.push(format!("pomelo-202610{d:02}-080000-before-v{}.db", 20 + d));
            files.push(format!("pomelo-202610{d:02}-090000-before-import.db"));
            files.push(format!("picky-202610{d:02}-080000-before-v{d}.db"));
        }
        files.extend(["pomelo-20261007-080000-before-restore.db", "pomelo-20261008-080000-before-restore.db", "pomelo-20261008-080000-before-restore-2.db", "pomelo-20261009-080000-before-restore.db"].map(String::from));
        files.extend(["picky-20261001-080000-before-picky-import.db", "picky-20261002-080000-before-import.db"].map(String::from));
        files.extend(["pomelo-20260901-080000-manual.db", "pomelo-20260901-080000-daily.db", "pomelo-20260901-080000-before-vx.db"].map(String::from));
        for f in &files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let deleted = prune_safety(&dir, Store::Pomelo, KEEP_SAFETY).unwrap();
        let left: Vec<String> = list(&dir).unwrap().into_iter().filter(|b| b.store == Store::Pomelo).map(|b| b.file_name).collect();
        for kept in ["pomelo-20261006-080000-before-v26.db", "pomelo-20261004-080000-before-v24.db", "pomelo-20261004-090000-before-import.db", "pomelo-20261009-080000-before-restore.db", "pomelo-20261008-080000-before-restore-2.db", "pomelo-20261008-080000-before-restore.db"] {
            assert!(left.iter().any(|f| f == kept), "{kept}");
        }
        assert!(!left.iter().any(|f| f == "pomelo-20261003-080000-before-v23.db" || f == "pomelo-20261007-080000-before-restore.db"));
        // manual, daily and labels that aren't safety copies stay; 3 + 3 + 3 + 3 = 12 left
        assert_eq!((deleted.len(), left.len()), (3 + 3 + 1, 12));
        // picky on its own: its upgrades and its imports (both kinds of import label) are one kind each
        assert_eq!(list(&dir).unwrap().iter().filter(|b| b.store == Store::Picky).count(), 8);
        prune_safety(&dir, Store::Picky, KEEP_SAFETY).unwrap();
        assert_eq!(list(&dir).unwrap().iter().filter(|b| b.store == Store::Picky).count(), 5);
        assert!(prune_safety(&dir, Store::Pomelo, KEEP_SAFETY).unwrap().is_empty()); // idempotent
    }

    #[test]
    fn daily_once_per_day() {
        let dir = temp_dir("backup-daily");
        let c = file_db(&dir);
        let b = dir.join("backups");
        assert!(daily(&c, &b).unwrap().is_some());
        assert!(daily(&c, &b).unwrap().is_none());
        snapshot(&c, &b, MANUAL).unwrap();
        assert!(daily(&c, &b).unwrap().is_none());
        assert_eq!(list(&b).unwrap().len(), 2);
    }

    #[test]
    fn restore_roundtrip_keeps_wal_and_safety_copy() {
        let dir = temp_dir("backup-restore");
        let b = dir.join("backups");
        let mut c = file_db(&dir);
        c.execute("INSERT INTO books (title) VALUES ('A'), ('B')", []).unwrap();
        let snap = snapshot(&c, &b, MANUAL).unwrap();
        c.execute("DELETE FROM books WHERE title = 'A'", []).unwrap();
        c.execute("INSERT INTO books (title) VALUES ('C')", []).unwrap();

        let name = snap.file_name().unwrap().to_str().unwrap();
        let safety = restore(&mut c, &b, name).unwrap();
        assert_eq!(titles(&c), ["A", "B"]);
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        c.execute("INSERT INTO books (title) VALUES ('D')", []).unwrap(); // still writable
        assert_eq!(parse_name(safety.file_name().unwrap().to_str().unwrap()).unwrap().label, BEFORE_RESTORE);

        // undo the restore from the safety copy
        restore(&mut c, &b, safety.file_name().unwrap().to_str().unwrap()).unwrap();
        assert_eq!(titles(&c), ["B", "C"]);

        // a fresh connection sees the same data (it hit the file, not just this connection)
        drop(c);
        let c2 = db::open(&dir.join("pomelo.db")).unwrap();
        assert_eq!(titles(&c2), ["B", "C"]);
    }

    #[test]
    fn restore_old_version_migrates() {
        let dir = temp_dir("backup-restore-old");
        let b = dir.join("backups");
        // a v2-era snapshot (books without kinds)
        let old = temp_dir("backup-restore-old-src");
        let mut o = db::open(&old.join("pomelo.db")).unwrap();
        db::migrate(&mut o, &db::MIGRATIONS[..2], &old).unwrap();
        o.execute("INSERT INTO books (title) VALUES ('旧书')", []).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let old_snap = snapshot(&o, &b, "before-v3").unwrap();

        let mut c = file_db(&dir);
        c.execute("INSERT INTO books (title) VALUES ('新书')", []).unwrap();
        restore(&mut c, &b, old_snap.file_name().unwrap().to_str().unwrap()).unwrap();
        assert_eq!(db::user_version(&c).unwrap(), db::tests::latest_version());
        assert_eq!(titles(&c), ["旧书"]);
        let kind: String = c.query_row("SELECT k.name FROM books b JOIN kinds k ON k.id = b.kind_id", [], |r| r.get(0)).unwrap();
        assert_eq!(kind, "虚构");
        // only the explicit safety copy is added (no extra before-vN snapshot of the restored data)
        let labels: Vec<String> = list(&b).unwrap().into_iter().map(|x| x.label).collect();
        assert_eq!(labels.iter().filter(|l| *l == BEFORE_RESTORE).count(), 1);
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn restore_rejects_bad_input() {
        let dir = temp_dir("backup-restore-bad");
        let b = dir.join("backups");
        let mut c = file_db(&dir);
        c.execute("INSERT INTO books (title) VALUES ('keep')", []).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("pomelo-20260101-000000-manual.db"), b"not a database").unwrap();
        for name in ["../pomelo.db", "pomelo-20260101-000000-manual.db", "pomelo-20990101-000000-manual.db", "x.db"] {
            assert!(restore(&mut c, &b, name).is_err(), "{name}");
        }
        // newer schema is refused
        let newer = snapshot(&c, &b, MANUAL).unwrap();
        let w = Connection::open(&newer).unwrap();
        w.pragma_update(None, "user_version", db::tests::latest_version() + 1).unwrap();
        drop(w);
        let e = restore(&mut c, &b, newer.file_name().unwrap().to_str().unwrap()).unwrap_err();
        assert!(e.contains("新"), "{e}");
        assert_eq!(titles(&c), ["keep"]);
        // nothing was restored, so no safety copies either
        assert!(list(&b).unwrap().iter().all(|x| x.label != BEFORE_RESTORE));
    }
}
