//! External programs for 呦呦漫游: git (/usr/bin/git), gh (Homebrew; only to read Vercel's result), Node
//! (the major version in the site's `.nvmrc`, looked up in nvm). Every run has a timeout, never asks for
//! input (no terminal: `GIT_TERMINAL_PROMPT=0`, stdin closed), and gets a fixed PATH (a GUI app has none).
//! Long runs (npm) go in their own process group so stopping kills npm and what it started.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const GIT: &str = "/usr/bin/git";
const BASE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin";
/// Lines kept from a long run's output (the tail is what a failure shows).
pub const LOG_LINES: usize = 400;

pub fn gh() -> Option<PathBuf> {
    ["/opt/homebrew/bin/gh", "/usr/local/bin/gh"].iter().map(PathBuf::from).find(|p| p.is_file())
}

/// PATH for npm: the Node bin folder first (npm's `#!/usr/bin/env node` must find that node).
pub fn path_with(bin: Option<&Path>) -> String {
    match bin {
        Some(b) => format!("{}:{BASE_PATH}", b.display()),
        None => BASE_PATH.into(),
    }
}

#[derive(Debug)]
pub struct Out {
    pub ok: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    /// The last few lines of stderr (else stdout): what an error message quotes.
    pub fn tail(&self) -> String {
        let s = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        let lines: Vec<&str> = s.trim().lines().collect();
        lines[lines.len().saturating_sub(6)..].join("\n")
    }
}

fn base(program: &Path, dir: &Path, path: &str) -> Command {
    let mut c = Command::new(program);
    c.current_dir(dir)
        .env("PATH", path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    if let Some(home) = std::env::var_os("HOME") {
        c.env("HOME", home);
    }
    c
}

/// Runs to the end (or `timeout`, then kills it): output as text, `stdin` fed and closed.
pub fn run(program: &Path, dir: &Path, args: &[&str], stdin: Option<&str>, timeout: Duration) -> Result<Out, String> {
    let mut cmd = base(program, dir, BASE_PATH);
    cmd.args(args).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("运行 {} 失败：{e}", program.display()))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out_t = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_t = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let status = wait(&mut child, timeout, None);
    let (stdout, stderr) = (out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default());
    match status {
        Some(s) => Ok(Out { ok: s.success(), code: s.code(), stdout, stderr }),
        None => Err(format!("{} 超过 {} 秒没有结束，已停止", name(program), timeout.as_secs())),
    }
}

fn name(program: &Path) -> String {
    program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Waits for the child; past `timeout` kills it (and its group when given) and returns None.
fn wait(child: &mut Child, timeout: Duration, group: Option<u32>) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                match group {
                    Some(g) => kill_group(g),
                    None => {
                        let _ = child.kill();
                    }
                }
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// git in the site folder; an error carries git's own last lines.
pub fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let out = run(Path::new(GIT), dir, args, None, timeout)?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(out.tail())
    }
}

/// SIGTERM to the whole group, SIGKILL after 3 s if anything is left.
pub fn kill_group(pgid: u32) {
    let target = format!("-{pgid}");
    let _ = Command::new("/bin/kill").args(["-TERM", "--", &target]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        let _ = Command::new("/bin/kill").args(["-KILL", "--", &target]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

/// Output lines of a long run, newest last (at most LOG_LINES).
#[derive(Default, Clone)]
pub struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    pub fn push(&self, line: String) {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g.push(line);
        let over = g.len().saturating_sub(LOG_LINES);
        g.drain(..over);
    }
    pub fn tail(&self, n: usize) -> Vec<String> {
        let g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        g[g.len().saturating_sub(n)..].to_vec()
    }
}

/// Starts a long run in its own process group, its stdout + stderr lines going to `log`.
pub fn spawn_logged(program: &Path, dir: &Path, args: &[&str], node_bin: Option<&Path>, log: &Log) -> Result<Child, String> {
    let mut cmd = base(program, dir, &path_with(node_bin));
    // ASTRO_DEV_BACKGROUND (any value): Astro 7 otherwise detaches `astro dev` when it thinks an AI agent runs it.
    cmd.args(args).env("FORCE_COLOR", "0").env("NO_COLOR", "1").env("ASTRO_DEV_BACKGROUND", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("运行 {} 失败：{e}", program.display()))?;
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        let log = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                log.push(strip_ansi(&line));
            }
        });
    }
    Ok(child)
}

/// Runs a long command to its end (killing its group past `timeout`); true = exit 0. `on_start` gets the
/// group id (so a quit can stop it).
pub fn run_logged(program: &Path, dir: &Path, args: &[&str], node_bin: Option<&Path>, log: &Log, timeout: Duration, on_start: impl FnOnce(u32)) -> Result<bool, String> {
    let mut child = spawn_logged(program, dir, args, node_bin, log)?;
    on_start(child.id());
    let group = child.id();
    match wait(&mut child, timeout, Some(group)) {
        Some(s) => Ok(s.success()),
        None => Err(format!("超过 {} 分钟没有结束，已停止", timeout.as_secs() / 60)),
    }
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

// ---------- Node ----------

fn version_of(dir_name: &str) -> Option<(u32, u32, u32)> {
    let v = dir_name.strip_prefix('v')?;
    let mut it = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// The bin folder of the newest nvm Node whose major version is `major`.
pub fn nvm_node(home: &Path, major: u32) -> Option<PathBuf> {
    let root = home.join(".nvm/versions/node");
    let mut found: Vec<((u32, u32, u32), PathBuf)> = std::fs::read_dir(&root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let v = version_of(&e.file_name().to_string_lossy())?;
            let bin = e.path().join("bin");
            (v.0 == major && bin.join("node").is_file() && bin.join("npm").exists()).then_some((v, bin))
        })
        .collect();
    found.sort();
    found.pop().map(|(_, b)| b)
}

/// The major version the site asks for (`.nvmrc`: "22", "v22", "22.3.0"…).
pub fn wanted_major(site: &Path) -> Option<u32> {
    let s = std::fs::read_to_string(site.join(".nvmrc")).ok()?;
    s.trim().trim_start_matches('v').split('.').next()?.parse().ok()
}

/// Node for the site: nvm's newest of the `.nvmrc` major, else a Homebrew node of that major.
pub fn node_bin(site: &Path, home: &Path) -> Result<PathBuf, String> {
    let Some(major) = wanted_major(site) else {
        return Err("网站文件夹里没有 .nvmrc，不知道要用哪个版本的 Node".into());
    };
    if let Some(b) = nvm_node(home, major) {
        return Ok(b);
    }
    for b in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let node = Path::new(b).join("node");
        if node.is_file() {
            if let Ok(o) = run(&node, site, &["--version"], None, Duration::from_secs(10)) {
                if version_of(o.stdout.trim()).map(|v| v.0) == Some(major) {
                    return Ok(PathBuf::from(b));
                }
            }
        }
    }
    Err(format!("找不到 Node {major}（网站的 .nvmrc 要求）：用 nvm 装一个就好"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    #[test]
    fn picks_the_newest_node_of_the_major() {
        let home = temp_dir("site-nvm");
        for v in ["v22.1.0", "v22.23.2", "v22.9.0", "v24.16.0"] {
            let bin = home.join(".nvm/versions/node").join(v).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("node"), "").unwrap();
            std::fs::write(bin.join("npm"), "").unwrap();
        }
        assert_eq!(nvm_node(&home, 22).unwrap(), home.join(".nvm/versions/node/v22.23.2/bin"));
        assert!(nvm_node(&home, 20).is_none());
        let site = home.join("site");
        std::fs::create_dir_all(&site).unwrap();
        assert!(node_bin(&site, &home).unwrap_err().contains(".nvmrc"));
        std::fs::write(site.join(".nvmrc"), "v22\n").unwrap();
        assert_eq!(wanted_major(&site), Some(22));
        assert_eq!(node_bin(&site, &home).unwrap(), home.join(".nvm/versions/node/v22.23.2/bin"));
    }

    #[test]
    fn runs_with_timeout_and_tail() {
        let dir = std::env::temp_dir();
        let o = run(Path::new("/bin/sh"), &dir, &["-c", "echo a; echo b >&2; exit 3"], None, Duration::from_secs(5)).unwrap();
        assert!(!o.ok && o.code == Some(3) && o.stdout == "a\n" && o.tail() == "b");
        let o = run(Path::new("/bin/cat"), &dir, &[], Some("原样\n#保留"), Duration::from_secs(5)).unwrap();
        assert_eq!(o.stdout, "原样\n#保留");
        assert!(run(Path::new("/bin/sleep"), &dir, &["5"], None, Duration::from_millis(200)).unwrap_err().contains("没有结束"));
    }

    #[test]
    fn logged_runs_keep_the_tail_and_stop_the_group() {
        let dir = std::env::temp_dir();
        let log = Log::default();
        let ok = run_logged(Path::new("/bin/sh"), &dir, &["-c", "for i in $(seq 1 500); do echo line$i; done; printf '\\033[31mred\\033[0m\\n' >&2"], None, &log, Duration::from_secs(10), |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let tail = log.tail(1000);
        assert!(ok && tail.len() == LOG_LINES && tail.contains(&"line500".to_string()) && tail.contains(&"red".to_string()));
        let started = Instant::now();
        let r = run_logged(Path::new("/bin/sh"), &dir, &["-c", "sleep 30 & sleep 30"], None, &Log::default(), Duration::from_millis(300), |_| {});
        assert!(r.unwrap_err().contains("没有结束") && started.elapsed() < Duration::from_secs(5));
    }
}
