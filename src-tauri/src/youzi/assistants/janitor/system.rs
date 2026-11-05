//! What DevJanitor asks the Mac (read-only, system tools only — no new dependencies): disk totals (`df`),
//! memory (`sysctl`, `vm_stat`, `ps` + libproc's footprint), programs that make a folder 「在用」 (`ps`, then
//! `lsof` for their working folders), installed apps' bundle ids (`plutil` on Info.plist), last opened
//! (`mdls`, Spotlight's kMDItemLastUsedDate), Docker (`docker system df`), Full Disk Access (TCC.db readable).
//! The sandbox (`Sys::Fake`) answers from the fake Mac's `.janitor-fake.json` instead; memory is the real
//! one either way (reading it changes nothing), except in tests.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    pub name: String,
    /// Its working folder (virtual), when it's a dev tool.
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerPart {
    pub name: String,
    pub size: u64,
    pub reclaimable: u64,
}

/// What the fake Mac says about itself (fake.rs writes it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Fake {
    pub disk_total: u64,
    /// Used outside what the scan reaches (系统和其他).
    pub system: u64,
    pub procs: Vec<Proc>,
    /// Running apps' bundle ids.
    pub apps: Vec<String>,
    pub docker: Option<Vec<DockerPart>>,
    pub full_disk_access: bool,
    /// What /usr/libexec/java_home and xcode-select -p say.
    pub java_home: Option<String>,
    pub xcode_select: Option<String>,
    pub docker_details: Option<DockerDetails>,
}

/// Docker's images (name and size), containers and volumes (only while Docker runs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DockerDetails {
    pub images: Vec<(String, u64)>,
    pub containers: u64,
    pub volumes: u64,
}

pub const FAKE_FILE: &str = ".janitor-fake.json";

pub enum Sys {
    Real,
    /// What the fake Mac says.
    Fake(Fake),
}

/// Programs whose working folder makes a project 「在用」 (a dev server, a build), and caches theirs.
const DEV_TOOLS: [&str; 22] = ["node", "cargo", "rustc", "bun", "deno", "python", "java", "gradle", "mvn", "pnpm", "npm", "yarn", "vite", "conda", "brew", "pip", "uv", "swift", "xcodebuild", "go", "dotnet", "tauri"];

pub fn dev_tool(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    DEV_TOOLS.iter().any(|t| n == *t || n.starts_with(&format!("{t}-")) || (*t == "python" && n.starts_with("python")) || (*t == "pip" && n.starts_with("pip")))
}

/// Runs a system tool and returns its output (None: missing, failed or too slow).
pub fn run(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = std::io::Read::read_to_end(&mut out, &mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader.join().ok()?;
                return status.success().then(|| String::from_utf8_lossy(&bytes).into_owned());
            }
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

const SHORT: Duration = Duration::from_secs(8);

impl Sys {
    pub fn fake(root: &Path) -> Self {
        let f = std::fs::read_to_string(root.join(FAKE_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Sys::Fake(f)
    }

    /// (total, free) of the disk, in bytes.
    pub fn disk(&self, scanned: u64) -> Option<(u64, u64)> {
        match self {
            Sys::Fake(f) => Some((f.disk_total, f.disk_total.saturating_sub(scanned + f.system))),
            Sys::Real => {
                let out = run("/bin/df", &["-k", "-P", "/System/Volumes/Data"], SHORT).or_else(|| run("/bin/df", &["-k", "-P", "/"], SHORT))?;
                let cols: Vec<u64> = out.lines().nth(1)?.split_whitespace().skip(1).take(3).filter_map(|v| v.parse().ok()).collect();
                (cols.len() == 3).then(|| (cols[0] * 1024, cols[2] * 1024))
            }
        }
    }

    pub fn full_disk_access(&self) -> bool {
        match self {
            Sys::Fake(f) => f.full_disk_access,
            Sys::Real => std::fs::File::open("/Library/Application Support/com.apple.TCC/TCC.db").is_ok(),
        }
    }

    /// Dev tools running, with their working folders, and running apps' bundle ids.
    pub fn running(&self) -> (Vec<Proc>, HashSet<String>) {
        match self {
            Sys::Fake(f) => (f.procs.clone(), f.apps.iter().cloned().collect()),
            Sys::Real => {
                let Some(ps) = run("/bin/ps", &["-axo", "pid=,comm="], SHORT) else { return (Vec::new(), HashSet::new()) };
                let mut tools: Vec<(String, String)> = Vec::new();
                let mut bundles: HashSet<String> = HashSet::new();
                for line in ps.lines() {
                    let line = line.trim_start();
                    let Some((pid, comm)) = line.split_once(' ') else { continue };
                    let comm = comm.trim();
                    if let Some(i) = comm.find(".app/Contents/") {
                        bundles.insert(comm[..i + 4].to_string());
                    }
                    let base = comm.rsplit('/').next().unwrap_or(comm);
                    if dev_tool(base) {
                        tools.push((pid.to_string(), base.to_string()));
                    }
                }
                let mut cwd: HashMap<String, String> = HashMap::new();
                if !tools.is_empty() {
                    let pids = tools.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(",");
                    if let Some(out) = run("/usr/sbin/lsof", &["-a", "-d", "cwd", "-Fn", "-p", &pids], SHORT) {
                        let mut pid = String::new();
                        for l in out.lines() {
                            if let Some(p) = l.strip_prefix('p') {
                                pid = p.to_string();
                            } else if let Some(n) = l.strip_prefix('n') {
                                cwd.insert(pid.clone(), n.to_string());
                            }
                        }
                    }
                }
                let procs = tools.into_iter().map(|(pid, name)| Proc { cwd: cwd.get(&pid).cloned(), name }).collect();
                let ids = bundles.iter().filter_map(|b| bundle_id(Path::new(b))).collect();
                (procs, ids)
            }
        }
    }

    /// When each file was last opened (Spotlight), by real path; files it doesn't know are left out.
    pub fn last_opened(&self, files: &[PathBuf]) -> HashMap<PathBuf, i64> {
        let mut out = HashMap::new();
        if matches!(self, Sys::Fake(..)) {
            return out;
        }
        for chunk in files.chunks(200) {
            let mut args: Vec<&str> = vec!["-raw", "-name", "kMDItemLastUsedDate"];
            let strs: Vec<String> = chunk.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            args.extend(strs.iter().map(String::as_str));
            let Some(raw) = run("/usr/bin/mdls", &args, Duration::from_secs(30)) else { continue };
            for (p, v) in chunk.iter().zip(raw.split('\0')) {
                if let Some(t) = parse_mdls_date(v.trim()) {
                    out.insert(p.clone(), t);
                }
            }
        }
        out
    }

    pub fn docker(&self) -> Option<Vec<DockerPart>> {
        match self {
            Sys::Fake(f) => f.docker.clone(),
            Sys::Real => {
                let bin = ["/usr/local/bin/docker", "/opt/homebrew/bin/docker", "/Applications/Docker.app/Contents/Resources/bin/docker"].into_iter().find(|b| Path::new(b).exists())?;
                let out = run(bin, &["system", "df", "--format", "{{json .}}"], SHORT)?;
                let parts: Vec<DockerPart> = out
                    .lines()
                    .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                    .map(|v| DockerPart {
                        name: docker_name(v["Type"].as_str().unwrap_or("")),
                        size: parse_human(v["Size"].as_str().unwrap_or("")),
                        reclaimable: parse_human(v["Reclaimable"].as_str().unwrap_or("").split(' ').next().unwrap_or("")),
                    })
                    .collect();
                (!parts.is_empty()).then_some(parts)
            }
        }
    }

    /// The default JDK's home (virtual).
    pub fn java_home(&self) -> Option<String> {
        match self {
            Sys::Fake(f) => f.java_home.clone(),
            Sys::Real => run("/usr/libexec/java_home", &[], SHORT).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }

    /// The developer folder in use (Xcode's or the command line tools').
    pub fn xcode_select(&self) -> Option<String> {
        match self {
            Sys::Fake(f) => f.xcode_select.clone(),
            Sys::Real => run("/usr/bin/xcode-select", &["-p"], SHORT).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }

    /// An app's version (CFBundleShortVersionString).
    pub fn app_version(&self, app: &Path) -> Option<String> {
        let p = app.join("Contents/Info.plist");
        let p = p.to_string_lossy();
        run("/usr/bin/plutil", &["-extract", "CFBundleShortVersionString", "raw", "-o", "-", &p], SHORT).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    pub fn docker_details(&self) -> Option<DockerDetails> {
        match self {
            Sys::Fake(f) => f.docker_details.clone(),
            Sys::Real => {
                let bin = ["/usr/local/bin/docker", "/opt/homebrew/bin/docker", "/Applications/Docker.app/Contents/Resources/bin/docker"].into_iter().find(|b| Path::new(b).exists())?;
                let images = run(bin, &["images", "--format", "{{.Repository}}:{{.Tag}}\t{{.Size}}"], SHORT)?;
                let images = images.lines().filter_map(|l| l.split_once('\t')).map(|(n, s)| (n.to_string(), parse_human(s))).collect();
                let count = |args: &[&str]| run(bin, args, SHORT).map_or(0, |o| o.lines().filter(|l| !l.trim().is_empty()).count() as u64);
                Some(DockerDetails { images, containers: count(&["ps", "-aq"]), volumes: count(&["volume", "ls", "-q"]) })
            }
        }
    }

    /// Moves a file or folder (real path) to the Trash; the fake Mac has its own.
    pub fn trash(&self, real: &Path, home_real: &Path) -> Result<(), String> {
        match self {
            Sys::Fake(..) => {
                let bin = home_real.join(".Trash");
                std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
                let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mut to = bin.join(&name);
                let mut n = 2;
                while to.exists() {
                    to = bin.join(format!("{name} {n}"));
                    n += 1;
                }
                std::fs::rename(real, &to).map_err(|e| e.to_string())
            }
            Sys::Real => crate::youzi::assistants::pub2md::move_to_trash(real),
        }
    }

    /// Runs a fixed cleaning command (`conda clean`). The fake Mac doesn't run it: `fake` is done instead.
    pub fn clean_command(&self, program: &Path, args: &[&str], fake: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        match self {
            Sys::Fake(..) => fake(),
            Sys::Real => {
                let p = program.to_string_lossy();
                run(&p, args, Duration::from_secs(600)).map(|_| ()).ok_or_else(|| format!("{} 没有跑成", program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
            }
        }
    }
}

fn docker_name(t: &str) -> String {
    match t {
        "Images" => "镜像",
        "Containers" => "容器",
        "Local Volumes" => "数据卷",
        "Build Cache" => "构建缓存",
        other => other,
    }
    .to_string()
}

/// "1.2GB", "800MB", "512kB", "0B" → bytes.
pub fn parse_human(s: &str) -> u64 {
    let s = s.trim();
    let i = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let n: f64 = s[..i].parse().unwrap_or(0.0);
    let unit = s[i..].trim().to_ascii_uppercase();
    let m = match unit.as_str() {
        "KB" | "K" => 1e3,
        "MB" | "M" => 1e6,
        "GB" | "G" => 1e9,
        "TB" | "T" => 1e12,
        _ => 1.0,
    };
    (n * m) as u64
}

/// "2024-03-01 10:00:00 +0000" → unix seconds.
pub fn parse_mdls_date(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(' ')?;
    let time = rest.split(' ').next()?;
    let ok = date.len() == 10 && date.as_bytes()[4] == b'-' && date.as_bytes()[7] == b'-' && date.bytes().enumerate().all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !ok {
        return None;
    }
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, m, sec) = (t.next()??, t.next()??, t.next()??);
    Some(crate::modules::common::day_number(date) * 86400 + h * 3600 + m * 60 + sec)
}

/// An app's bundle id (Info.plist, binary or XML).
pub fn bundle_id(app: &Path) -> Option<String> {
    let plist = app.join("Contents/Info.plist");
    let p = plist.to_string_lossy();
    let out = run("/usr/bin/plutil", &["-extract", "CFBundleIdentifier", "raw", "-o", "-", &p], SHORT)?;
    let id = out.trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// Reveals a path in Finder.
pub fn reveal(real: &Path) -> Result<(), String> {
    let target = if real.exists() { real.to_path_buf() } else { real.parent().map(Path::to_path_buf).unwrap_or_default() };
    Command::new("/usr/bin/open").arg("-R").arg(&target).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err("在访达中打不开".into()) })
}

// ---- memory (only shown; nothing is freed) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    /// normal | warn | critical
    pub pressure: String,
    pub top: Vec<MemUser>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemUser {
    pub name: String,
    pub bytes: u64,
}

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
}

/// libproc's phys_footprint (what Activity Monitor shows), for this user's processes.
fn footprint(pid: i32) -> Option<u64> {
    // rusage_info_v0: uuid (16 bytes), then u64s; ri_phys_footprint is the 8th u64 after the uuid
    let mut buf = [0u64; 16];
    let ok = unsafe { proc_pid_rusage(pid, 0, buf.as_mut_ptr()) } == 0;
    ok.then(|| buf[2 + 7])
}

/// The program a process belongs to: the app it's inside of, else its file name.
pub fn program_name(comm: &str) -> String {
    if let Some(i) = comm.find(".app/") {
        let app = &comm[..i];
        return app.rsplit('/').next().unwrap_or(app).to_string();
    }
    comm.rsplit('/').next().unwrap_or(comm).to_string()
}

pub fn memory() -> Memory {
    if cfg!(test) {
        return Memory { total: 18 * (1 << 30), used: 11 * (1 << 30), pressure: "normal".into(), top: vec![MemUser { name: "Google Chrome".into(), bytes: 3 << 30 }] };
    }
    let total = run("/usr/sbin/sysctl", &["-n", "hw.memsize"], SHORT).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let level: u32 = run("/usr/sbin/sysctl", &["-n", "kern.memorystatus_vm_pressure_level"], SHORT).and_then(|s| s.trim().parse().ok()).unwrap_or(1);
    let pressure = match level {
        4 => "critical",
        2 => "warn",
        _ => "normal",
    };
    let used = run("/usr/bin/vm_stat", &[], SHORT).map(|s| vm_used(&s)).unwrap_or(0);
    let mut by: HashMap<String, u64> = HashMap::new();
    if let Some(ps) = run("/bin/ps", &["-axo", "pid=,rss=,comm="], SHORT) {
        for line in ps.lines() {
            let mut it = line.split_whitespace();
            let (Some(pid), Some(rss)) = (it.next().and_then(|p| p.parse::<i32>().ok()), it.next().and_then(|r| r.parse::<u64>().ok())) else { continue };
            let comm: String = it.collect::<Vec<_>>().join(" ");
            let bytes = footprint(pid).unwrap_or(rss * 1024);
            *by.entry(program_name(&comm)).or_default() += bytes;
        }
    }
    let mut top: Vec<MemUser> = by.into_iter().map(|(name, bytes)| MemUser { name, bytes }).collect();
    top.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    top.truncate(8);
    Memory { total, used, pressure: pressure.into(), top }
}

/// Used memory as Activity Monitor roughly counts it: active + wired + compressed pages.
pub fn vm_used(vm_stat: &str) -> u64 {
    let page: u64 = vm_stat.split("page size of ").nth(1).and_then(|s| s.split(' ').next()).and_then(|s| s.parse().ok()).unwrap_or(16384);
    let pages = |key: &str| -> u64 { vm_stat.lines().find(|l| l.starts_with(key)).and_then(|l| l.rsplit(':').next()).and_then(|v| v.trim().trim_end_matches('.').parse().ok()).unwrap_or(0) };
    (pages("Pages active") + pages("Pages wired down") + pages("Pages occupied by compressor")) * page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tool_output() {
        assert_eq!(parse_human("1.5GB"), 1_500_000_000);
        assert_eq!(parse_human("512kB"), 512_000);
        assert_eq!(parse_human("0B"), 0);
        assert_eq!(parse_mdls_date("1970-01-02 00:00:10 +0000"), Some(86410));
        assert_eq!(parse_mdls_date("(null)"), None);
        let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                               10.\nPages active:                            100.\nPages wired down:                         20.\nPages occupied by compressor:             5.\n";
        assert_eq!(vm_used(vm), 125 * 16384);
        assert_eq!(program_name("/Applications/Google Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"), "Google Chrome");
        assert_eq!(program_name("/opt/homebrew/bin/node"), "node");
        assert!(dev_tool("node") && dev_tool("python3.12") && dev_tool("cargo-clippy") && !dev_tool("Finder"));
        assert!(run("/bin/sleep", &["5"], Duration::from_millis(100)).is_none());
        assert_eq!(run("/bin/echo", &["hi"], SHORT).as_deref(), Some("hi\n"));
    }
}
