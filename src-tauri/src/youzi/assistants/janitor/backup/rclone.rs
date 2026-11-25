//! rclone for the backup's clouds (1.1.0 Phase 3; requirements「DevJanitor › 备份」): the Homebrew rclone with
//! a config of its own, `<data>/rclone/rclone.conf`, which rclone keeps encrypted. Its password lives only in the
//! keychain (the 「Pomelo OS Agent」 item, account `rclone`; the sandbox: a fixed one) and reaches rclone in its
//! environment (`RCLONE_CONFIG_PASS`), never on a command line or on disk. The cloud login (PikPak: account,
//! password, the token rclone refreshes) is in that config only; what `config create` prints is dropped.
//! - A sync mirrors the drive into the cloud folder: VAULT and the system's junk left out (`filters`); a rename on
//!   the drive is a move there (size, mtime and name; no checksums: `--track-renames-strategy modtime,leaf`);
//!   deletions go to the cloud's trash (PikPak's own; the fake cloud: `--backup-dir`) and only after every upload
//!   went through (rclone deletes nothing when anything failed); more than `MAX_DELETE` of them stop it until I
//!   say so. Names go up in NFC. Progress: rclone's JSON stats, once a second.
//! - Sandbox (dev, tests, a scratch data dir): the 「PikPak」 is a local folder (`<data>/janitor-cloud/PikPak`),
//!   reached through the same encrypted config; uploads go through rclone's ordinary copy, so the limit and the
//!   progress behave like a real upload.

use super::super::{rules, Janitor, Mode};
use super::diff;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// The remote's name in the config.
pub const REMOTE: &str = "pikpak";
/// The keychain account of the config's password.
pub const KEY: &str = "rclone";
/// More deletions than this in one sync stop it (a drive that suddenly looks empty) until I say so.
pub const MAX_DELETE: u32 = 100;
/// The fake cloud, beside the fake Mac.
pub const FAKE_CLOUD: &str = "janitor-cloud";
const SANDBOX_PASS: &str = "pomelo-sandbox-rclone";
/// The fake PikPak's size.
const FAKE_TOTAL: u64 = 6_000 * rules::GB;
const ERRORS_MAX: usize = 200;

/// The Homebrew rclone (a Mac app doesn't get the shell's PATH).
pub fn binary() -> Option<PathBuf> {
    ["/opt/homebrew/bin/rclone", "/usr/local/bin/rclone"].iter().map(PathBuf::from).find(|p| p.is_file())
}

/// The fake cloud's PikPak (sandbox).
pub fn fake_root(j: &Janitor) -> PathBuf {
    j.backup.data.join(FAKE_CLOUD).join("PikPak")
}

/// The fake PikPak's trash (sandbox).
#[cfg(test)]
pub fn fake_trash(j: &Janitor) -> PathBuf {
    fake_trash_of(&fake_root(j))
}

/// rclone's filters: VAULT (any case, any depth) and the system's junk, as diff.rs leaves them out.
pub fn filters() -> Vec<String> {
    let mut f = vec!["--ignore-case".to_string()];
    for p in ["vault", "vault.*"] {
        f.push(format!("- {p}"));
        f.push(format!("- {p}/**"));
    }
    for j in diff::JUNK {
        f.push(format!("- {j}"));
        f.push(format!("- {j}/**"));
    }
    for p in ["._*", &format!("{}*", diff::PART), &format!("/{}", super::volumes::FAKE_FILE)] {
        f.push(format!("- {p}"));
    }
    f.into_iter().flat_map(|r| if r.starts_with("--") { vec![r] } else { vec!["--filter".to_string(), r] }).collect()
}

/// A cloud folder's name, checked (no `..`, no hidden part, no VAULT, not the root).
pub fn folder_of(f: &str) -> Result<String, String> {
    let f = f.trim().trim_matches('/').to_string();
    if f.is_empty() {
        return Err("要写 PikPak 上的一个文件夹".into());
    }
    if f.chars().count() > 200 || f.split('/').any(|p| p.trim().is_empty() || p == "." || p == ".." || p.starts_with('.')) {
        return Err("文件夹的名字不对".into());
    }
    if f.split('/').any(rules::sealed_name) {
        return Err("这个名字不能用".into());
    }
    Ok(f)
}

pub struct Rclone {
    pub bin: PathBuf,
    conf: PathBuf,
    pass: String,
    mode: Mode,
    fake: PathBuf,
}

/// A fresh password for the config: 40 letters and digits.
fn new_password() -> String {
    let mut b = [0u8; 40];
    let _ = getrandom::fill(&mut b);
    const ABC: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    b.iter().map(|x| ABC[*x as usize % ABC.len()] as char).collect()
}

/// What rclone said last, for a message (its log lines are JSON).
fn last_words(out: &[u8]) -> String {
    let text = String::from_utf8_lossy(out);
    let line = text.lines().rev().find(|l| !l.trim().is_empty() && !l.contains("Config file")).unwrap_or("").trim().to_string();
    let msg = serde_json::from_str::<Value>(&line).ok().and_then(|v| v["msg"].as_str().map(str::to_string)).unwrap_or(line);
    msg.chars().take(300).collect()
}

impl Rclone {
    /// `pass`: the config's password (None: not made yet — a login makes it).
    pub fn new(j: &Janitor, pass: Option<String>) -> Result<Self, String> {
        let bin = binary().ok_or("没装 rclone：在终端里跑 brew install rclone")?;
        let pass = match j.mode {
            Mode::Sandbox => SANDBOX_PASS.to_string(),
            Mode::Real => pass.ok_or("PikPak 的登录不在了（钥匙串里没有），重新登录一次")?,
        };
        Ok(Self { bin, conf: j.backup.data.join("rclone").join("rclone.conf"), pass, mode: j.mode, fake: fake_root(j) })
    }

    /// The password a new login uses (Real: a fresh one to keep in the keychain).
    pub fn fresh_password() -> String {
        new_password()
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new(&self.bin);
        c.arg("--config").arg(&self.conf).arg("--ask-password=false").env("RCLONE_CONFIG_PASS", &self.pass).env_remove("RCLONE_PASSWORD_COMMAND").stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = self.cmd().args(args).output().map_err(|e| format!("rclone 没跑起来：{e}"))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(last_words(&out.stderr))
        }
    }

    /// A new encrypted config (an old one, mine no more, goes).
    fn fresh_config(&self) -> Result<(), String> {
        let dir = self.conf.parent().ok_or("没有上一层")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let _ = std::fs::remove_file(&self.conf);
        let out = Command::new(&self.bin)
            .arg("--config")
            .arg(&self.conf)
            .args(["config", "encryption", "set", "--password-command", "/usr/bin/printenv POMELO_RCLONE_PW"])
            .env("POMELO_RCLONE_PW", &self.pass)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("rclone 没跑起来：{e}"))?;
        let head = std::fs::read_to_string(&self.conf).unwrap_or_default();
        if !out.status.success() || !head.contains("RCLONE_ENCRYPT_V0") {
            let _ = std::fs::remove_file(&self.conf);
            return Err(format!("rclone 的配置没加密成：{}", last_words(&out.stderr)));
        }
        Ok(())
    }

    /// Logs in (a fresh config holding only this remote), then checks it by asking how full it is.
    pub fn login(&self, account: &str, password: &str) -> Result<(u64, u64), String> {
        self.fresh_config()?;
        let r = match self.mode {
            Mode::Real => self.run(&["config", "create", REMOTE, "pikpak", "user", account, "pass", password, "--obscure", "--non-interactive"]),
            Mode::Sandbox => {
                std::fs::create_dir_all(&self.fake).map_err(|e| e.to_string())?;
                self.run(&["config", "create", REMOTE, "local", "--non-interactive"])
            }
        };
        let checked = r.map_err(|e| format!("没登录上：{e}")).and_then(|_| self.about());
        if checked.is_err() {
            self.logout();
        }
        checked
    }

    /// The config goes (the login with it).
    pub fn logout(&self) {
        let _ = std::fs::remove_file(&self.conf);
    }

    /// (used, total) in the cloud.
    pub fn about(&self) -> Result<(u64, u64), String> {
        if self.mode == Mode::Sandbox {
            let used = diff::list(&self.fake, &diff::Skip::default(), &AtomicBool::new(false)).map(|l| l.files.values().map(|(_, m)| m.size).sum()).unwrap_or(0);
            return Ok((used, FAKE_TOTAL));
        }
        let out = self.run(&["about", &format!("{REMOTE}:"), "--json"]).map_err(|e| format!("PikPak 连不上：{e}"))?;
        let v: Value = serde_json::from_slice(&out).map_err(|_| "PikPak 没说用了多少".to_string())?;
        Ok((v["used"].as_u64().unwrap_or(0), v["total"].as_u64().unwrap_or(0)))
    }

    /// The remote path of a cloud folder.
    pub fn dest(&self, folder: &str) -> String {
        match self.mode {
            Mode::Real => format!("{REMOTE}:{folder}"),
            Mode::Sandbox => format!("{REMOTE}:{}", self.fake.join(folder).to_string_lossy()),
        }
    }

    /// Mirrors `source` into the cloud folder; `p` follows along and can stop it.
    pub fn sync(&self, source: &Path, folder: &str, limit_mb: u32, force: bool, day: &str, p: &Progress) -> Done {
        let mut done = Done::default();
        let mut c = self.cmd();
        c.arg("sync").arg(source).arg(self.dest(folder));
        c.args(["--use-json-log", "--stats", "1s", "--stats-log-level", "NOTICE", "--check-first", "--track-renames", "--track-renames-strategy", "modtime,leaf", "--local-unicode-normalization", "--retries", "3"]);
        c.args(filters());
        if limit_mb > 0 {
            c.args(["--bwlimit", &format!("{limit_mb}M")]);
        }
        if !force {
            c.args(["--max-delete", &MAX_DELETE.to_string()]);
        }
        match self.mode {
            Mode::Real => {
                c.arg("--pikpak-use-trash=true");
            }
            Mode::Sandbox => {
                p.trash_is_move.store(true, Ordering::Relaxed);
                let trash = fake_trash_of(&self.fake).join(day);
                c.args(["--disable", "Copy", "--backup-dir", &format!("{REMOTE}:{}", trash.to_string_lossy())]);
            }
        }
        c.stdout(Stdio::null()).stderr(Stdio::piped()).process_group(0);
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                done.fatal = Some(format!("rclone 没跑起来：{e}"));
                return done;
            }
        };
        if let Ok(mut g) = p.pid.lock() {
            *g = Some(child.id());
        }
        if p.stop.load(Ordering::SeqCst) {
            p.kill();
        }
        if let Some(err) = child.stderr.take() {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                p.read_line(&line, &mut done);
            }
        }
        let status = child.wait();
        if let Ok(mut g) = p.pid.lock() {
            *g = None;
        }
        done.stopped = p.stop.load(Ordering::SeqCst);
        done.ok = !done.stopped && status.is_ok_and(|s| s.success());
        if !done.ok && !done.stopped && !source.is_dir() {
            done.stopped = true;
            done.fatal = Some("移动硬盘拔掉了".into());
        }
        done
    }
}

fn fake_trash_of(fake: &Path) -> PathBuf {
    fake.with_file_name("PikPak 回收站")
}

/// A sync as it goes (from rclone's stats), and how to stop it.
#[derive(Debug, Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub total_bytes: AtomicU64,
    pub files: AtomicU64,
    pub total_files: AtomicU64,
    pub deletes: AtomicU64,
    pub renames: AtomicU64,
    /// Bytes a second.
    pub speed: AtomicU64,
    /// Seconds left (0: unknown).
    pub eta: AtomicU64,
    /// Still comparing the two sides (before the uploads).
    pub checking: AtomicBool,
    /// The trash is a folder (`--backup-dir`): rclone counts each deletion as a rename too.
    pub trash_is_move: AtomicBool,
    pub current: Mutex<Vec<String>>,
    pub stop: AtomicBool,
    pid: Mutex<Option<u32>>,
}

/// How a sync ended.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Done {
    pub ok: bool,
    pub stopped: bool,
    /// More deletions than `MAX_DELETE`.
    pub too_many_deletes: bool,
    pub errors: Vec<(String, String)>,
    /// Why it couldn't run.
    pub fatal: Option<String>,
}

impl Progress {
    pub fn new() -> Self {
        let p = Self::default();
        p.checking.store(true, Ordering::Relaxed);
        p
    }

    /// Stops rclone (its whole process group).
    pub fn kill(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(pid) = self.pid.lock().ok().and_then(|g| *g) {
            let _ = Command::new("/bin/kill").args(["-TERM", "--", &format!("-{pid}")]).status();
        }
    }

    /// One line of rclone's JSON log.
    pub fn read_line(&self, line: &str, done: &mut Done) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        let msg = v["msg"].as_str().unwrap_or("");
        let s = &v["stats"];
        if s.is_object() {
            let n = |k: &str| s[k].as_u64().unwrap_or(0);
            self.bytes.store(n("bytes"), Ordering::Relaxed);
            self.total_bytes.store(n("totalBytes"), Ordering::Relaxed);
            self.files.store(n("transfers"), Ordering::Relaxed);
            self.total_files.store(n("totalTransfers"), Ordering::Relaxed);
            self.deletes.store(n("deletes"), Ordering::Relaxed);
            let trash_moves = if self.trash_is_move.load(Ordering::Relaxed) { n("deletes") } else { 0 };
            self.renames.store(n("renames").saturating_sub(trash_moves), Ordering::Relaxed);
            self.speed.store(s["speed"].as_f64().unwrap_or(0.0) as u64, Ordering::Relaxed);
            self.eta.store(s["eta"].as_u64().unwrap_or(0), Ordering::Relaxed);
            let names: Vec<String> = s["transferring"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str().map(str::to_string)).collect();
            // --check-first: once anything is queued, moving or renamed, the comparing is over (its own line is INFO, not logged)
            if n("totalTransfers") > 0 || n("bytes") > 0 || n("renames") > 0 || n("deletes") > 0 || !names.is_empty() {
                self.checking.store(false, Ordering::Relaxed);
            }
            if let Ok(mut g) = self.current.lock() {
                *g = names;
            }
        }
        if !matches!(v["level"].as_str(), Some("error" | "critical")) {
            return;
        }
        if msg.contains("--max-delete") {
            done.too_many_deletes = true;
        }
        // rclone's summing-up lines repeat what was said per file
        let summary = ["Cancelling sync", "not deleting", "Fatal error received", "Failed to sync", "Attempt "].iter().any(|w| msg.starts_with(w));
        if summary || done.errors.len() >= ERRORS_MAX || rules::mentions_sealed(line) {
            return;
        }
        let object = v["object"].as_str().filter(|o| !o.contains(" at ") && !o.starts_with("Local file system")).unwrap_or("").to_string();
        if !done.errors.iter().any(|(o, m)| *o == object && m == msg) {
            done.errors.push((object, msg.chars().take(300).collect()));
        }
    }
}
