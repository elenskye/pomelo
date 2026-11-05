//! Read-only commands DevJanitor's Agent runs itself (`agent::RUN`; requirements「DevJanitor · Agent 页」): a
//! fixed list of programs and, for the ones that can also change things, only the subcommands and flags that
//! look. One plain command: no pipes, redirects, `;` `&`, `$`, backticks, parentheses, braces, `!` or `\`
//! (zsh runs it as written, so nothing else can sneak in). Anything else is a 跑命令 card for me to click.
//! Real Mac: my login shell from home (60 s, then stopped; output capped); the fake Mac doesn't run it.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUT: usize = 60_000;
const BANNED: &[char] = &['|', ';', '&', '>', '<', '`', '$', '(', ')', '{', '}', '!', '\\', '\n', '\r'];
/// Where a program given by path may live.
const BIN_DIRS: &[&str] = &["/usr/bin/", "/bin/", "/usr/sbin/", "/sbin/", "/opt/homebrew/bin/", "/usr/local/bin/"];

/// The command's words (quotes keep a word together), or why it isn't one plain command.
pub fn words(cmd: &str) -> Result<Vec<String>, String> {
    if let Some(c) = cmd.chars().find(|c| BANNED.contains(c)) {
        return Err(format!("有「{}」：只读命令只能是一条普通的命令（不接管道、重定向、变量）", if c == '\n' { "换行".to_string() } else { c.to_string() }));
    }
    let (mut out, mut cur, mut quote, mut any) = (Vec::new(), String::new(), None::<char>, false);
    for ch in cmd.trim().chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => (quote, any) = (Some(ch), true),
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err("引号没配对".into());
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        return Err("命令是空的".into());
    }
    Ok(out)
}

/// The program's name: a bare name, or a path in the system's and Homebrew's bin folders.
fn program(first: &str) -> Result<&str, String> {
    if first.contains('=') {
        return Err("不能在命令前面设环境变量".into());
    }
    if first == "/usr/libexec/java_home" {
        return Ok("java_home");
    }
    if first.contains('/') {
        return match BIN_DIRS.iter().find_map(|d| first.strip_prefix(d)).filter(|n| !n.contains('/')) {
            Some(n) => Ok(n),
            None => Err(format!("{first} 不在系统或 Homebrew 的程序文件夹里")),
        };
    }
    Ok(first)
}

const VERSION: &[&str] = &["-v", "-V", "--version", "-version", "version"];

/// Why `prog args` isn't on the read-only list (None: it is).
fn refuse(prog: &str, a: &[&str]) -> Option<String> {
    let first = a.first().copied().unwrap_or("");
    let second = a.get(1).copied().unwrap_or("");
    let has = |f: &[&str]| a.iter().any(|w| f.contains(w) || f.iter().any(|x| x.ends_with('=') && w.starts_with(x)));
    let only = |f: &[&str]| !a.is_empty() && a.iter().all(|w| f.contains(w));
    let sub = |f: &[&str]| f.contains(&first);
    let ok = match prog {
        "du" | "ls" | "stat" | "df" | "which" | "whereis" | "sw_vers" | "uname" | "whoami" | "id" | "ps" | "vm_stat" | "mdfind" | "mdls" | "lsof" | "uptime" | "system_profiler" | "nproc" | "getconf" => true,
        "top" => has(&["-l"]),
        "find" => !has(&["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf", "-fls"]),
        "sysctl" => !has(&["-w"]) && !a.iter().any(|w| w.contains('=')),
        "defaults" => sub(&["read", "read-type", "domains", "find"]),
        "launchctl" => sub(&["list", "print", "print-disabled"]),
        "diskutil" => sub(&["list", "info"]) || (first == "apfs" && ["list", "listSnapshots", "listVolumeGroups"].contains(&second)),
        "tmutil" => sub(&["listlocalsnapshots", "listlocalsnapshotdates", "destinationinfo", "listbackups", "latestbackup", "machinedirectory", "isexcluded"]),
        "pkgutil" => sub(&["--pkgs", "--pkg-info", "--files", "--pkg-groups", "--file-info", "--pkgs-plist", "--pkg-info-plist"]) && !has(&["--forget", "--expand", "--flatten", "--learn", "--bom"]),
        "java" => has(&["-version", "--version"]),
        "java_home" => !has(&["--exec", "-exec"]),
        "xcode-select" => only(&["-p", "--print-path", "-v", "--version"]),
        "xcrun" => (first == "simctl" && second == "list") || only(&["--show-sdk-path", "--show-sdk-version", "--show-sdk-platform-path"]),
        "xcodebuild" => only(&["-version", "-showsdks"]),
        "node" | "python" | "python3" | "ruby" | "rustc" | "cargo" | "deno" | "bun" | "swift" | "clang" | "gcc" | "mvn" | "gradle" | "kotlin" | "php" | "perl" | "dart" | "R" => only(VERSION),
        "rustup" => sub(&["show", "--version", "-V"]) || (sub(&["toolchain", "target", "component"]) && second == "list"),
        "go" => sub(&["version"]) || (first == "env" && !has(&["-w", "-u"])),
        "dotnet" => only(&["--list-sdks", "--list-runtimes", "--info", "--version"]),
        "gem" => sub(&["list", "env", "--version", "-v"]),
        "pip" | "pip3" => sub(&["list", "show", "freeze", "--version", "-V"]) || (first == "cache" && ["dir", "info", "list"].contains(&second)),
        "npm" => sub(&["ls", "list", "ll", "la", "root", "prefix", "outdated", "-v", "--version"]) || (first == "config" && ["get", "list", "ls"].contains(&second)),
        "pnpm" => sub(&["ls", "list", "root", "outdated", "why", "-v", "--version"]) || (first == "store" && ["path", "status"].contains(&second)),
        "yarn" => sub(&["--version", "-v"]) || (first == "cache" && second == "dir"),
        "conda" | "mamba" | "micromamba" => {
            sub(&["info", "list", "--version", "-V"]) || (first == "env" && second == "list") || (first == "config" && has(&["--show", "--show-sources", "--get"])) || (first == "clean" && has(&["-d", "--dry-run"]))
        }
        "brew" => {
            sub(&["list", "ls", "info", "outdated", "leaves", "deps", "uses", "config", "desc", "doctor", "missing", "commands", "--cache", "--cellar", "--prefix", "--repository", "--version", "-v"]) || (sub(&["cleanup", "autoremove"]) && has(&["-n", "--dry-run"]))
        }
        "docker" => matches!((first, second), ("system", "df") | ("image" | "container" | "volume" | "network", "ls") | ("builder" | "buildx", "du")) || sub(&["images", "ps", "info", "version"]),
        "pyenv" => sub(&["versions", "version", "root", "prefix", "which"]),
        "nvm" => sub(&["ls", "list", "current", "version", "which", "--version"]),
        "ollama" => sub(&["list", "ls", "ps", "--version", "-v"]),
        _ => return Some(format!("{prog} 不在只读清单里")),
    };
    (!ok).then(|| format!("{prog} 的这种用法可能会改东西，不在只读清单里"))
}

/// The command if it's on the read-only list (its words), or why not.
pub fn check(cmd: &str) -> Result<Vec<String>, String> {
    let w = words(cmd)?;
    let prog = program(&w[0])?;
    let args: Vec<&str> = w[1..].iter().map(String::as_str).collect();
    match refuse(prog, &args) {
        Some(why) => Err(why),
        None => Ok(w),
    }
}

/// Runs a checked command in my login shell from `home`: its output (stdout and stderr, capped) and how it ended.
pub fn run(cmd: &str, home: &Path) -> String {
    let child = Command::new("/bin/zsh")
        .args(["-lic", cmd])
        .current_dir(home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return format!("没跑起来：{e}"),
    };
    let out = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut readers = Vec::new();
    for mut pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        let out = out.clone();
        readers.push(std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut o) = out.lock() {
                    if o.len() < MAX_OUT * 4 {
                        o.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }));
    }
    let start = Instant::now();
    let ended = loop {
        match child.try_wait() {
            Ok(Some(s)) => break match s.code() {
                Some(0) => String::new(),
                Some(c) => format!("（退出码 {c}）"),
                None => "（被停下了）".into(),
            },
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(40)),
            _ => {
                let _ = Command::new("/bin/kill").args(["-TERM", "--", &format!("-{}", child.id())]).status();
                let _ = child.kill();
                let _ = child.wait();
                break format!("（{} 秒没跑完，停下了；下面是已经输出的）", TIMEOUT.as_secs());
            }
        }
    };
    for r in readers {
        let _ = r.join();
    }
    let bytes = out.lock().map(|o| o.clone()).unwrap_or_default();
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if text.chars().count() > MAX_OUT {
        text = text.chars().take(MAX_OUT).collect::<String>() + "\n…（后面太长，没放进来）";
    }
    let text = if text.trim().is_empty() { "（没有输出）".to_string() } else { text };
    if ended.is_empty() { text } else { format!("{ended}\n{text}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_and_quotes() {
        assert_eq!(words("du -sh ~/Library/Caches").unwrap(), ["du", "-sh", "~/Library/Caches"]);
        assert_eq!(words("ls -la \"~/Library/Application Support\"").unwrap(), ["ls", "-la", "~/Library/Application Support"]);
        assert_eq!(words("mdls -name kMDItemLastUsedDate '/Applications/Foo Bar.app'").unwrap()[3], "/Applications/Foo Bar.app");
        assert!(words("ls \"open").is_err());
        for bad in ["du -sh ~ | sort", "ls; rm -rf ~", "ls && rm x", "ls > out", "echo $HOME", "ls `rm x`", "ls $(rm x)", "ls *(e:rm:)", "ls {a,b}", "ls !!", "ls a\\ b", "ls\nrm x"] {
            assert!(words(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_the_listed_looks() {
        for ok in [
            "du -sh ~/Library/Caches",
            "/usr/bin/du -d 1 -h ~",
            "ls -la ~/.conda",
            "find ~/Downloads -name '*.dmg' -size +100M",
            "brew list --versions",
            "brew cleanup -n",
            "brew --cache",
            "conda env list",
            "conda clean --all --dry-run",
            "conda config --show",
            "pip3 list",
            "pip cache dir",
            "npm ls -g --depth=0",
            "npm config get cache",
            "pnpm store path",
            "docker system df",
            "docker images",
            "java -version",
            "/usr/libexec/java_home -V",
            "xcrun simctl list devices",
            "rustup toolchain list",
            "go env GOPATH",
            "dotnet --list-sdks",
            "nvm ls",
            "sysctl -n hw.memsize",
            "defaults read com.apple.dock",
            "top -l 1 -n 10",
            "diskutil apfs list",
            "tmutil listlocalsnapshots /",
            "mdls -name kMDItemLastUsedDate /Applications/Safari.app",
        ] {
            assert!(check(ok).is_ok(), "{ok}: {:?}", check(ok));
        }
        for no in [
            "rm -rf ~/Library/Caches",
            "find ~ -name x -delete",
            "find ~ -exec rm {} +",
            "brew cleanup",
            "brew uninstall wget",
            "brew home",
            "conda clean --all --yes",
            "conda env remove -n x",
            "pip uninstall x",
            "npm cache clean --force",
            "npm version patch",
            "docker system prune",
            "sysctl -w kern.x=1",
            "sysctl kern.x=1",
            "defaults write com.apple.dock x",
            "launchctl unload x",
            "go env -w GOPATH=x",
            "/usr/libexec/java_home --exec java",
            "top",
            "~/bin/du -sh ~",
            "/tmp/du -sh ~",
            "FOO=1 du -sh ~",
            "sudo du -sh /",
            "tccutil reset All",
            "python3 -c print",
            "xcrun simctl delete unavailable",
            "diskutil eraseDisk x",
            "pkgutil --forget com.x",
        ] {
            assert!(check(no).is_err(), "{no}");
        }
    }

    #[test]
    fn runs_and_reports_how_it_ended() {
        let home = std::env::temp_dir();
        assert!(run("echo hi", &home).contains("hi"));
        assert!(run("false", &home).starts_with("（退出码 1）"));
    }
}
