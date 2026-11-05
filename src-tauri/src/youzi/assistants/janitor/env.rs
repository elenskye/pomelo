//! 开发环境 (v0.8 阶段 7; requirements「DevJanitor · 环境页」): what's installed for development, found after
//! the walk from the tree (sizes) and the environments' own small files (a JDK's `release`, Conda's
//! `conda-meta/`, a venv's `pyvenv.cfg`, nvm's `alias/default`, rustup's `settings.toml`). Read only; only
//! looking (uninstalling is the Agent's, 阶段 8).
//! - Each environment: rows with version, where, size, who installed it, the default, packages (count and the
//!   biggest). A row inside another one (an old Homebrew version, an old extension, a Docker image) is
//!   `part` and isn't added to the environment's size.
//! - 其他: big folders nobody claims (home's dot folders, ~/.cache, ~/.local/share, ~/.config, /opt, /usr/local
//!   outside Homebrew's), 100 MB or more after taking out what's claimed inside — for the Agent to work out.
//! - Defaults come from the tools (java_home, xcode-select) or their files; real commands only on the real
//!   disk, the fake Mac answers from `.janitor-fake.json`.

use super::rules::{self, Protected};
use super::scan::{Config, Tree};
use super::store::ItemRow;
use super::system::Sys;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvItem {
    pub title: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub size: u64,
    /// Who installed it: Homebrew, nvm, conda, 安装包…
    pub by: Option<String>,
    pub default: bool,
    pub packages: Option<u64>,
    /// The biggest packages / parts.
    pub top: Vec<(String, u64)>,
    pub tag: Option<String>,
    /// Inside another row (not added to the environment's size).
    pub part: bool,
    pub protected: Option<String>,
    /// Deleting it needs an administrator's password.
    pub admin: bool,
    pub newest: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Env {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub note: Option<String>,
    pub items: Vec<EnvItem>,
}

pub struct Ctx<'a> {
    pub cfg: &'a Config,
    pub sys: &'a Sys,
    pub tree: &'a Tree<'a>,
    pub items: &'a [ItemRow],
    pub protected: &'a Protected,
}

fn join(a: &str, b: &str) -> String {
    if a == "/" {
        format!("/{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// "17.2 (21C62)" / "v24.16.0" / "gradle-8.5-bin" → numbers to sort by.
pub fn ver_key(s: &str) -> Vec<u64> {
    s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).take(5).filter_map(|x| x.parse().ok()).collect()
}

impl Ctx<'_> {
    fn home(&self, rel: &str) -> String {
        format!("{}/{rel}", self.cfg.home)
    }

    fn at(&self, p: &str) -> String {
        p.strip_prefix("~/").map_or_else(|| p.to_string(), |r| self.home(r))
    }

    fn is_dir(&self, v: &str) -> bool {
        self.cfg.real_of(v).is_dir()
    }

    fn exists(&self, v: &str) -> bool {
        std::fs::symlink_metadata(self.cfg.real_of(v)).is_ok()
    }

    /// Names in a folder, sorted (hidden files and .DS_Store left out).
    fn ls(&self, v: &str) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(self.cfg.real_of(v)).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect()).unwrap_or_default();
        out.sort();
        out
    }

    /// Folders in a folder, sorted (not symlinks to folders: /usr/local/mysql → mysql-9.4.0…).
    fn dirs(&self, v: &str) -> Vec<String> {
        self.ls(v).into_iter().filter(|n| std::fs::symlink_metadata(self.cfg.real_of(&join(v, n))).is_ok_and(|m| m.is_dir())).collect()
    }

    /// A small text file (a version file, a config).
    fn read(&self, v: &str) -> Option<String> {
        let p = self.cfg.real_of(v);
        let m = std::fs::metadata(&p).ok()?;
        (m.is_file() && m.len() <= 256 * 1024).then(|| std::fs::read_to_string(&p).ok()).flatten()
    }

    fn size(&self, v: &str) -> u64 {
        self.tree.size(v)
    }

    fn top(&self, v: &str, n: usize) -> Vec<(String, u64)> {
        self.tree.list(v).into_iter().filter(|(_, s)| *s > 0).take(n).collect()
    }

    /// Outside home and Homebrew, owned by root (the fake Mac: under /Library).
    fn admin(&self, v: &str) -> bool {
        if rules::within(v, &self.cfg.home) || rules::within(v, "/opt/homebrew") {
            return false;
        }
        if self.cfg.allocated {
            std::fs::symlink_metadata(self.cfg.real_of(v)).is_ok_and(|m| m.uid() == 0)
        } else {
            rules::within(v, "/Library")
        }
    }

    fn item(&self, title: impl Into<String>, version: Option<String>, path: &str, by: &str) -> EnvItem {
        EnvItem {
            title: title.into(),
            version,
            path: Some(path.to_string()),
            size: self.size(path),
            by: (!by.is_empty()).then(|| by.to_string()),
            protected: self.protected.of(path).map(str::to_string),
            admin: self.admin(path),
            newest: self.tree.newest(path),
            tag: self.tree.denied(path).then(|| "没权限".to_string()),
            ..Default::default()
        }
    }

    fn brew_prefixes(&self) -> Vec<String> {
        ["/opt/homebrew", "/usr/local"].iter().filter(|p| self.is_dir(&format!("{p}/Cellar"))).map(|p| p.to_string()).collect()
    }

    /// Homebrew's installs of a formula (`python` matches `python@3.13`): (formula, version, path, an older version).
    fn cellar(&self, formula: &str) -> Vec<(String, String, String, bool)> {
        let mut out = Vec::new();
        for prefix in self.brew_prefixes() {
            let cellar = format!("{prefix}/Cellar");
            for f in self.dirs(&cellar).into_iter().filter(|f| f == formula || f.starts_with(&format!("{formula}@"))) {
                let dir = join(&cellar, &f);
                let mut vers = self.dirs(&dir);
                vers.sort_by_key(|v| ver_key(v));
                let last = vers.len().saturating_sub(1);
                for (i, v) in vers.into_iter().enumerate() {
                    out.push((f.clone(), v.clone(), join(&dir, &v), i < last));
                }
            }
        }
        out
    }

    /// The biggest packages in a Python's site-packages, and how many there are (`*.dist-info`).
    fn site_packages(&self, root: &str) -> (Option<u64>, Vec<(String, u64)>) {
        for lib in self.dirs(&format!("{root}/lib")).into_iter().filter(|l| l.starts_with("python")) {
            let sp = format!("{root}/lib/{lib}/site-packages");
            if self.is_dir(&sp) {
                let names = self.ls(&sp);
                let count = names.iter().filter(|n| n.ends_with(".dist-info") || n.ends_with(".egg-info")).count() as u64;
                let top = self.tree.list(&sp).into_iter().filter(|(n, s)| *s > 0 && !n.ends_with("-info") && n != "__pycache__").take(5).collect();
                return (Some(count), top);
            }
        }
        (None, Vec::new())
    }

    /// A symlink's target as a virtual path.
    fn link(&self, v: &str) -> Option<String> {
        let target = std::fs::read_link(self.cfg.real_of(v)).ok()?;
        let t = target.to_string_lossy();
        let joined = if t.starts_with('/') { t.into_owned() } else { join(v.rsplit_once('/').map_or("/", |(p, _)| if p.is_empty() { "/" } else { p }), &t) };
        let mut parts: Vec<&str> = Vec::new();
        for seg in joined.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                s => parts.push(s),
            }
        }
        Some(format!("/{}", parts.join("/")))
    }
}

#[derive(Default)]
struct Found {
    envs: Vec<Env>,
    claims: Vec<String>,
}

impl Found {
    fn claim(&mut self, p: impl Into<String>) {
        self.claims.push(p.into());
    }

    fn add(&mut self, id: &str, name: &str, note: Option<String>, items: Vec<EnvItem>) {
        if items.is_empty() {
            return;
        }
        self.claims.extend(items.iter().filter_map(|i| i.path.clone()));
        let size = items.iter().filter(|i| !i.part).map(|i| i.size).sum();
        self.envs.push(Env { id: id.into(), name: name.into(), size, note, items });
    }
}

pub fn detect(c: &Ctx) -> Vec<Env> {
    let mut f = Found::default();
    java(c, &mut f);
    conda(c, &mut f);
    python(c, &mut f);
    node(c, &mut f);
    rust(c, &mut f);
    homebrew(c, &mut f);
    jvm_tools(c, &mut f);
    docker(c, &mut f);
    xcode(c, &mut f);
    editors(c, &mut f);
    dotnet(c, &mut f);
    go(c, &mut f);
    ruby(c, &mut f);
    flutter(c, &mut f);
    android(c, &mut f);
    known(c, &mut f);
    unknown(c, &mut f);
    f.envs
}

// ---- Java ----

fn release_field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(|v| v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty())
}

/// "1.8.0_461" → 8, "25.0.1" → 25.
pub fn java_major(v: &str) -> String {
    let mut it = v.split(['.', '_', '-', '+']);
    match it.next() {
        Some("1") => it.next().unwrap_or("1").to_string(),
        Some(m) => m.to_string(),
        None => v.to_string(),
    }
}

fn java(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for dir in ["/Library/Java/JavaVirtualMachines".to_string(), c.home("Library/Java/JavaVirtualMachines")] {
        for name in c.dirs(&dir).into_iter().filter(|n| n.ends_with(".jdk")) {
            let jdk = join(&dir, &name);
            let rel = c.read(&format!("{jdk}/Contents/Home/release")).unwrap_or_default();
            let version = release_field(&rel, "JAVA_VERSION").or_else(|| name.strip_suffix(".jdk").map(|n| n.trim_start_matches(|c: char| !c.is_ascii_digit()).to_string()));
            let vendor = release_field(&rel, "IMPLEMENTOR").map(|v| v.split(' ').next().unwrap_or("").trim_end_matches(".com").to_string());
            let major = version.as_deref().map(java_major).unwrap_or_default();
            let title = match vendor {
                Some(v) if !v.is_empty() => format!("JDK {major}（{v}）"),
                _ => format!("JDK {major}"),
            };
            items.push(c.item(title, version, &jdk, "安装包"));
        }
    }
    for (_, ver, path, old) in c.cellar("openjdk") {
        let mut i = c.item(format!("JDK {}（Homebrew）", java_major(&ver)), Some(ver), &path, "Homebrew");
        if old {
            i.tag = Some("旧版本".into());
        }
        items.push(i);
    }
    let sdk = c.home(".sdkman/candidates/java");
    for name in c.dirs(&sdk).into_iter().filter(|n| n != "current") {
        items.push(c.item(format!("JDK {}（SDKMAN）", java_major(&name)), Some(name.clone()), &join(&sdk, &name), "SDKMAN"));
    }
    if let Some(h) = c.sys.java_home() {
        if let Some(i) = items.iter_mut().filter(|i| i.path.as_deref().is_some_and(|p| rules::within(&h, p))).max_by_key(|i| i.path.as_ref().map_or(0, String::len)) {
            i.default = true;
        }
    }
    f.claim(c.home(".sdkman"));
    f.add("java", "Java", None, items);
}

// ---- Conda ----

fn conda_env(c: &Ctx, title: String, path: &str, by: &str) -> EnvItem {
    let metas: Vec<String> = c.ls(&format!("{path}/conda-meta")).into_iter().filter(|n| n.ends_with(".json")).collect();
    let python = metas.iter().find_map(|n| n.strip_prefix("python-").and_then(|r| r.split('-').next()).filter(|v| v.starts_with(|ch: char| ch.is_ascii_digit())).map(str::to_string));
    let mut i = c.item(title, python.map(|v| format!("Python {v}")), path, by);
    i.packages = Some(metas.len() as u64);
    i.top = c.site_packages(path).1;
    i
}

fn conda(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for root in rules::CONDA_ROOTS.iter().map(|r| c.at(r)) {
        if !c.is_dir(&format!("{root}/conda-meta")) {
            continue;
        }
        let flavor = root.split('/').rev().find(|s| s.contains("conda") || s.contains("forge")).unwrap_or("conda").to_string();
        let by = if root.contains("/Caskroom/") { "Homebrew" } else { "安装包" };
        let (envs, pkgs) = (format!("{root}/envs"), format!("{root}/pkgs"));
        let mut base = conda_env(c, format!("base（{flavor}）"), &root, by);
        base.size = c.size(&root).saturating_sub(c.size(&envs) + c.size(&pkgs));
        base.default = true;
        items.push(base);
        for name in c.dirs(&envs) {
            let p = join(&envs, &name);
            if c.is_dir(&format!("{p}/conda-meta")) {
                items.push(conda_env(c, name, &p, by));
            }
        }
        if c.is_dir(&pkgs) {
            let mut i = c.item("包缓存（pkgs）", None, &pkgs, by);
            i.tag = Some("一键清理能清没在用的".into());
            items.push(i);
        }
        f.claim(root);
    }
    f.claim(c.home(".conda"));
    f.add("conda", "Conda", None, items);
}

// ---- Python ----

fn python(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for (dir, by) in [("/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions", "系统（命令行工具）"), ("/Library/Frameworks/Python.framework/Versions", "安装包")] {
        for v in c.dirs(dir).into_iter().filter(|v| v != "Current") {
            let p = join(dir, &v);
            let mut i = c.item(format!("Python {v}"), Some(v.clone()), &p, by);
            i.packages = c.site_packages(&p).0;
            items.push(i);
        }
    }
    for (_, ver, path, old) in c.cellar("python") {
        let mut i = c.item(format!("Python {ver}"), Some(ver), &path, "Homebrew");
        if old {
            i.tag = Some("旧版本".into());
        }
        items.push(i);
    }
    let pyenv = c.home(".pyenv/versions");
    for v in c.dirs(&pyenv) {
        let p = join(&pyenv, &v);
        let mut i = c.item(format!("Python {v}"), Some(v.clone()), &p, "pyenv");
        (i.packages, i.top) = c.site_packages(&p);
        items.push(i);
    }
    let uv = c.home(".local/share/uv/python");
    for v in c.dirs(&uv) {
        let ver = v.split('-').nth(1).unwrap_or(&v).to_string();
        items.push(c.item(format!("Python {ver}"), Some(ver.clone()), &join(&uv, &v), "uv"));
    }
    // the projects' virtual environments (found by the scan)
    for it in c.items.iter().filter(|i| i.kind == rules::PYTHON && !i.listed) {
        let Some(p) = it.paths.first().filter(|p| [".venv", "venv", "env"].contains(&p.rsplit('/').next().unwrap_or(""))) else { continue };
        let cfg = c.read(&format!("{p}/pyvenv.cfg")).unwrap_or_default();
        let version = cfg.lines().find_map(|l| l.split_once('=').filter(|(k, _)| matches!(k.trim(), "version" | "version_info")).map(|(_, v)| v.trim().split(".final").next().unwrap_or("").to_string())).filter(|v| !v.is_empty());
        let mut i = c.item(it.title.clone(), version.map(|v| format!("Python {v}")), p, "项目");
        (i.packages, i.top) = c.site_packages(p);
        i.tag = Some("一键清理能清".into());
        items.push(i);
    }
    // the default: what Homebrew's python3 points at, else pyenv's global, else the system's
    let linked = c.brew_prefixes().iter().find_map(|p| c.link(&format!("{p}/bin/python3")));
    let pyenv_global = c.read(&c.home(".pyenv/version")).and_then(|s| s.lines().next().map(|l| l.trim().to_string()));
    let pick = items.iter().position(|i| linked.as_deref().is_some_and(|l| i.path.as_deref().is_some_and(|p| rules::within(l, p)))).or_else(|| items.iter().position(|i| i.by.as_deref() == Some("pyenv") && pyenv_global.as_deref() == i.version.as_deref())).or_else(|| items.iter().position(|i| i.by.as_deref() == Some("系统（命令行工具）")));
    if let Some(k) = pick {
        items[k].default = true;
    }
    f.claim(c.home(".pyenv"));
    f.claim(uv);
    f.add("python", "Python", None, items);
}

// ---- Node ----

fn node_globals(c: &Ctx, nm: &str) -> (Option<u64>, Vec<(String, u64)>) {
    if !c.is_dir(nm) {
        return (None, Vec::new());
    }
    let skip = ["npm", "corepack"];
    let mut count = 0;
    for n in c.dirs(nm).into_iter().filter(|n| !skip.contains(&n.as_str())) {
        count += if n.starts_with('@') { c.dirs(&join(nm, &n)).len() as u64 } else { 1 };
    }
    let top = c.tree.list(nm).into_iter().filter(|(n, s)| *s > 0 && !skip.contains(&n.as_str())).take(5).collect();
    ((count > 0).then_some(count), top)
}

fn node(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let nvm = c.home(".nvm/versions/node");
    for v in c.dirs(&nvm).into_iter().filter(|v| v.starts_with('v')) {
        let p = join(&nvm, &v);
        let mut i = c.item(format!("Node {}", &v[1..]), Some(v[1..].to_string()), &p, "nvm");
        (i.packages, i.top) = node_globals(c, &format!("{p}/lib/node_modules"));
        items.push(i);
    }
    let alias = c.read(&c.home(".nvm/alias/default")).map(|s| s.trim().trim_start_matches('v').to_string());
    let brew = c.cellar("node");
    let newest_brew = brew.iter().filter(|b| !b.3).map(|b| b.2.clone()).collect::<Vec<_>>();
    for (_, ver, path, old) in brew {
        let mut i = c.item(format!("Node {ver}"), Some(ver), &path, "Homebrew");
        if old {
            i.tag = Some("旧版本".into());
        } else if newest_brew.contains(&path) {
            let prefix = path.split("/Cellar/").next().unwrap_or("").to_string();
            (i.packages, i.top) = node_globals(c, &format!("{prefix}/lib/node_modules"));
        }
        items.push(i);
    }
    let fnm = c.home(".local/share/fnm/node-versions");
    for v in c.dirs(&fnm) {
        let ver = v.trim_start_matches('v').to_string();
        items.push(c.item(format!("Node {ver}"), Some(ver.clone()), &join(&fnm, &v), "fnm"));
    }
    let volta = c.home(".volta/tools/image/node");
    for v in c.dirs(&volta) {
        items.push(c.item(format!("Node {v}"), Some(v.clone()), &join(&volta, &v), "Volta"));
    }
    // nvm's default ("24.16", "node"), else the only one
    let nvm_items: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.by.as_deref() == Some("nvm")).map(|(k, _)| k).collect();
    let pick = match alias.as_deref() {
        Some("node") | Some("stable") => nvm_items.iter().copied().max_by_key(|k| ver_key(items[*k].version.as_deref().unwrap_or(""))),
        Some(a) if !a.is_empty() && a.starts_with(|ch: char| ch.is_ascii_digit()) => nvm_items.iter().copied().filter(|k| items[*k].version.as_deref().is_some_and(|v| v == a || v.starts_with(&format!("{a}.")))).max_by_key(|k| ver_key(items[*k].version.as_deref().unwrap_or(""))),
        _ => None,
    }
    .or_else(|| (items.iter().filter(|i| i.tag.is_none()).count() == 1).then(|| items.iter().position(|i| i.tag.is_none())).flatten());
    if let Some(k) = pick {
        items[k].default = true;
    }
    for p in [".nvm", ".volta", ".local/share/fnm"] {
        f.claim(c.home(p));
    }
    f.add("node", "Node", None, items);
}

// ---- Rust ----

const RUSTUP_PROXIES: [&str; 14] = ["cargo", "cargo-clippy", "cargo-fmt", "cargo-miri", "clippy-driver", "rls", "rust-analyzer", "rust-gdb", "rust-gdbgui", "rust-lldb", "rustc", "rustdoc", "rustfmt", "rustup"];

fn rust(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let tc = c.home(".rustup/toolchains");
    let default = c.read(&c.home(".rustup/settings.toml")).and_then(|s| s.lines().find_map(|l| l.trim().strip_prefix("default_toolchain").map(|v| v.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string())));
    for name in c.dirs(&tc) {
        let mut i = c.item(name.clone(), name.split('-').next().map(str::to_string), &join(&tc, &name), "rustup");
        i.default = default.as_deref() == Some(name.as_str());
        items.push(i);
    }
    for (_, ver, path, old) in c.cellar("rust") {
        let mut i = c.item(format!("Rust {ver}"), Some(ver), &path, "Homebrew");
        if old {
            i.tag = Some("旧版本".into());
        }
        items.push(i);
    }
    let cargo = c.home(".cargo");
    if c.is_dir(&cargo) {
        let mut i = c.item("cargo 的工具和仓库", None, &cargo, "rustup");
        let tools: Vec<String> = c.ls(&format!("{cargo}/bin")).into_iter().filter(|n| !RUSTUP_PROXIES.contains(&n.as_str())).collect();
        i.packages = (!tools.is_empty()).then_some(tools.len() as u64);
        i.top = c.top(&cargo, 5);
        items.push(i);
    }
    for it in c.items.iter().filter(|i| i.kind == rules::RUST_TARGET && !i.listed) {
        let mut i = c.item(it.title.clone(), None, &it.paths[0], "项目");
        i.tag = Some("一键清理能清".into());
        items.push(i);
    }
    f.claim(c.home(".rustup"));
    f.add("rust", "Rust", None, items);
}

// ---- Homebrew ----

fn homebrew(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let mut note = None;
    for prefix in c.brew_prefixes() {
        let (cellar, casks) = (format!("{prefix}/Cellar"), format!("{prefix}/Caskroom"));
        let formulae = c.dirs(&cellar);
        let cask_list = c.dirs(&casks);
        let mut i = c.item(format!("Homebrew（{prefix}）"), None, &prefix, "");
        let mut top: Vec<(String, u64)> = c.tree.list(&cellar).into_iter().chain(c.tree.list(&casks)).filter(|(_, s)| *s > 0).collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        top.truncate(5);
        i.top = top;
        items.push(i);
        note = Some(format!("{} 个包、{} 个 App", formulae.len(), cask_list.len()));
        for formula in formulae {
            let dir = join(&cellar, &formula);
            let mut vers = c.dirs(&dir);
            vers.sort_by_key(|v| ver_key(v));
            vers.pop();
            for v in vers {
                let mut old = c.item(format!("{formula} {v}"), Some(v.clone()), &join(&dir, &v), "Homebrew");
                old.tag = Some("旧版本".into());
                old.part = true;
                items.push(old);
            }
        }
        f.claim(prefix);
    }
    f.add("homebrew", "Homebrew", note, items);
}

// ---- Gradle, Maven ----

fn jvm_tools(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let dists = c.home(".gradle/wrapper/dists");
    for d in c.dirs(&dists) {
        let ver = d.trim_start_matches("gradle-").trim_end_matches("-bin").trim_end_matches("-all").to_string();
        items.push(c.item(format!("Gradle {ver}"), Some(ver), &join(&dists, &d), "Gradle Wrapper"));
    }
    for (name, title) in [("gradle", "Gradle"), ("maven", "Maven")] {
        for (_, ver, path, old) in c.cellar(name) {
            let mut i = c.item(format!("{title} {ver}"), Some(ver), &path, "Homebrew");
            if old {
                i.tag = Some("旧版本".into());
            }
            items.push(i);
        }
    }
    let caches = c.home(".gradle/caches");
    if c.is_dir(&caches) {
        let mut i = c.item("Gradle 的缓存", None, &caches, "Gradle");
        i.top = c.top(&caches, 5);
        i.tag = Some("一键清理能清".into());
        items.push(i);
    }
    let repo = c.home(".m2/repository");
    if c.is_dir(&repo) {
        let mut i = c.item("Maven 的本地仓库", None, &repo, "Maven");
        i.top = c.top(&repo, 5);
        i.tag = Some("一键清理能清".into());
        items.push(i);
    }
    f.claim(c.home(".gradle"));
    f.claim(c.home(".m2"));
    f.add("jvm", "Gradle、Maven", None, items);
}

// ---- Docker ----

fn docker(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let app = "/Applications/Docker.app";
    let data = c.home("Library/Containers/com.docker.docker");
    if c.is_dir(app) {
        let version = c.sys.app_version(&c.cfg.real_of(app));
        items.push(c.item("Docker Desktop", version, app, "安装包"));
    }
    if c.is_dir(&data) {
        items.push(c.item("Docker 的数据（虚拟磁盘）", None, &data, ""));
    }
    for (p, title) in [(".colima", "Colima"), (".orbstack", "OrbStack")] {
        let p = c.home(p);
        if c.is_dir(&p) {
            items.push(c.item(title, None, &p, ""));
        }
    }
    if items.is_empty() {
        return;
    }
    let details = c.sys.docker_details();
    let note = match &details {
        Some(d) => format!("{} 个镜像 · {} 个容器 · {} 个数据卷", d.images.len(), d.containers, d.volumes),
        None => "Docker 没开，只知道一共多大".to_string(),
    };
    for (name, size) in details.map(|d| d.images).unwrap_or_default() {
        items.push(EnvItem { title: name, size, by: Some("镜像".into()), part: true, ..Default::default() });
    }
    f.claim(c.home(".docker"));
    f.add("docker", "Docker", Some(note), items);
}

// ---- Xcode ----

fn xcode(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for name in c.dirs("/Applications").into_iter().filter(|n| n.starts_with("Xcode") && n.ends_with(".app")) {
        let p = join("/Applications", &name);
        let version = c.sys.app_version(&c.cfg.real_of(&p));
        items.push(c.item(version.as_ref().map_or_else(|| name.trim_end_matches(".app").to_string(), |v| format!("Xcode {v}")), version, &p, "App Store"));
    }
    let clt = "/Library/Developer/CommandLineTools";
    if c.is_dir(clt) {
        items.push(c.item("命令行工具", None, clt, "系统"));
    }
    let devices = c.home("Library/Developer/CoreSimulator/Devices");
    if c.is_dir(&devices) {
        let mut i = c.item("模拟器设备", None, &devices, "Xcode");
        i.packages = Some(c.dirs(&devices).len() as u64);
        items.push(i);
    }
    for dir in ["/Library/Developer/CoreSimulator/Volumes", "/Library/Developer/CoreSimulator/Profiles/Runtimes"] {
        for r in c.dirs(dir) {
            items.push(c.item(format!("模拟器系统 {}", r.trim_end_matches(".simruntime")), None, &join(dir, &r), "Xcode"));
        }
    }
    if let Some(sel) = c.sys.xcode_select() {
        if let Some(i) = items.iter_mut().find(|i| i.path.as_deref().is_some_and(|p| rules::within(&sel, p))) {
            i.default = true;
        }
    }
    f.claim(c.home("Library/Developer"));
    f.add("xcode", "Xcode 和模拟器", None, items);
}

// ---- editors' extensions ----

/// "ms-python.python-2026.1.0-darwin-arm64" → ("ms-python.python", "2026.1.0-darwin-arm64").
pub fn ext_split(n: &str) -> (&str, &str) {
    let b = n.as_bytes();
    for i in 0..b.len().saturating_sub(1) {
        if b[i] == b'-' && b[i + 1].is_ascii_digit() {
            return (&n[..i], &n[i + 1..]);
        }
    }
    (n, "")
}

fn editors(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for (rel, name) in [(".vscode/extensions", "VS Code"), (".vscode-insiders/extensions", "VS Code Insiders"), (".cursor/extensions", "Cursor"), (".windsurf/extensions", "Windsurf"), (".antigravity/extensions", "Antigravity")] {
        let dir = c.home(rel);
        let exts = c.dirs(&dir);
        if exts.is_empty() {
            continue;
        }
        let mut i = c.item(format!("{name} 的扩展"), None, &dir, name);
        i.packages = Some(exts.len() as u64);
        i.top = c.top(&dir, 5);
        items.push(i);
        // the same extension in several versions: the older ones are left over
        let mut by: std::collections::BTreeMap<&str, Vec<&String>> = Default::default();
        for e in &exts {
            by.entry(ext_split(e).0).or_default().push(e);
        }
        for (_, mut vers) in by.into_iter().filter(|(_, v)| v.len() > 1) {
            vers.sort_by_key(|v| ver_key(ext_split(v).1));
            vers.pop();
            for v in vers {
                let mut old = c.item(v.clone(), Some(ext_split(v).1.to_string()), &join(&dir, v), name);
                old.tag = Some("旧版本".into());
                old.part = true;
                items.push(old);
            }
        }
        f.claim(c.home(rel.split('/').next().unwrap_or(rel)));
    }
    f.add("editors", "编辑器的扩展", None, items);
}

// ---- .NET, Go, Ruby, Flutter, Android ----

fn versions_in(c: &Ctx, items: &mut Vec<EnvItem>, dir: &str, title: &str, by: &str) {
    for v in c.dirs(dir) {
        let ver = v.trim_start_matches(|ch: char| !ch.is_ascii_digit()).to_string();
        items.push(c.item(format!("{title} {ver}"), Some(ver.clone()), &join(dir, &v), by));
    }
}

fn brew_versions(c: &Ctx, items: &mut Vec<EnvItem>, formula: &str, title: &str) {
    for (_, ver, path, old) in c.cellar(formula) {
        let mut i = c.item(format!("{title} {ver}"), Some(ver), &path, "Homebrew");
        if old {
            i.tag = Some("旧版本".into());
        }
        items.push(i);
    }
}

fn cache_item(c: &Ctx, items: &mut Vec<EnvItem>, rel: &str, title: &str, by: &str) {
    let p = c.home(rel);
    if c.is_dir(&p) {
        let mut i = c.item(title, None, &p, by);
        i.packages = Some(c.dirs(&p).len() as u64);
        i.top = c.top(&p, 5);
        items.push(i);
    }
}

fn dotnet(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let by = if c.brew_prefixes().iter().any(|p| c.is_dir(&format!("{p}/Caskroom/dotnet-sdk"))) { "Homebrew" } else { "安装包" };
    versions_in(c, &mut items, "/usr/local/share/dotnet/sdk", ".NET SDK", by);
    versions_in(c, &mut items, &c.home(".dotnet/sdk"), ".NET SDK", "dotnet-install");
    brew_versions(c, &mut items, "dotnet", ".NET SDK");
    cache_item(c, &mut items, ".nuget/packages", "NuGet 的包", "dotnet");
    if let Some(i) = items.iter_mut().filter(|i| i.title.starts_with(".NET SDK")).max_by_key(|i| ver_key(i.version.as_deref().unwrap_or(""))) {
        i.default = true;
    }
    for p in ["/usr/local/share/dotnet".to_string(), c.home(".dotnet"), c.home(".nuget")] {
        f.claim(p);
    }
    f.add("dotnet", ".NET", None, items);
}

fn go(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for root in ["/usr/local/go".to_string(), c.home("go/root")] {
        if let Some(v) = c.read(&format!("{root}/VERSION")).and_then(|s| s.lines().next().map(|l| l.trim_start_matches("go").to_string())) {
            items.push(c.item(format!("Go {v}"), Some(v), &root, "安装包"));
        }
    }
    versions_in(c, &mut items, &c.home("sdk"), "Go", "go install");
    brew_versions(c, &mut items, "go", "Go");
    let modcache = c.home("go/pkg/mod");
    if c.is_dir(&modcache) {
        let mut i = c.item("Go 的模块缓存", None, &modcache, "Go");
        i.top = c.top(&modcache, 5);
        items.push(i);
    }
    f.claim(c.home("go"));
    f.claim(c.home("sdk"));
    f.add("go", "Go", None, items);
}

fn ruby(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    versions_in(c, &mut items, &c.home(".rbenv/versions"), "Ruby", "rbenv");
    versions_in(c, &mut items, &c.home(".rvm/rubies"), "Ruby", "RVM");
    brew_versions(c, &mut items, "ruby", "Ruby");
    cache_item(c, &mut items, ".gem", "gem 的包", "Ruby");
    for p in [".rbenv", ".rvm", ".gem"] {
        f.claim(c.home(p));
    }
    f.add("ruby", "Ruby", None, items);
}

fn flutter(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for root in [c.home("flutter"), c.home("development/flutter")] {
        if c.is_dir(&format!("{root}/bin")) && c.exists(&format!("{root}/bin/flutter")) {
            let v = c.read(&format!("{root}/version")).map(|s| s.trim().to_string());
            items.push(c.item(v.as_ref().map_or("Flutter".to_string(), |v| format!("Flutter {v}")), v, &root, "安装包"));
        }
    }
    versions_in(c, &mut items, &c.home("fvm/versions"), "Flutter", "FVM");
    cache_item(c, &mut items, ".pub-cache", "Dart 的包", "Dart");
    for p in ["flutter", "development/flutter", "fvm", ".pub-cache"] {
        f.claim(c.home(p));
    }
    f.add("flutter", "Flutter、Dart", None, items);
}

fn android(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    let sdk = c.home("Library/Android/sdk");
    if c.is_dir(&sdk) {
        let mut i = c.item("Android SDK", None, &sdk, "Android Studio");
        i.packages = Some(c.dirs(&format!("{sdk}/platforms")).len() as u64);
        i.top = c.top(&sdk, 5);
        items.push(i);
    }
    let avd = c.home(".android/avd");
    for a in c.dirs(&avd).into_iter().filter(|a| a.ends_with(".avd")) {
        items.push(c.item(format!("模拟器 {}", a.trim_end_matches(".avd")), None, &join(&avd, &a), "Android Studio"));
    }
    f.claim(c.home(".android"));
    f.claim(c.home("Library/Android"));
    f.add("android", "Android", None, items);
}

// ---- other known ones, and the unknown ----

const KNOWN: [(&str, &str, &str); 24] = [
    ("~/.codex", "Codex 的数据", "Codex"),
    ("~/.claude", "Claude Code 的数据", "Claude Code"),
    ("~/.local/share/claude", "Claude Code 的版本", "Claude Code"),
    ("~/.p2", "Eclipse 的插件（p2）", "Eclipse"),
    ("~/.ollama", "Ollama 的模型", "Ollama"),
    ("~/.lmstudio", "LM Studio 的模型", "LM Studio"),
    ("~/.cache/lm-studio", "LM Studio 的模型", "LM Studio"),
    ("~/.cache/huggingface", "Hugging Face 的缓存", "Hugging Face"),
    ("~/.cache/torch", "PyTorch 的缓存", "PyTorch"),
    ("~/.julia", "Julia", "Julia"),
    ("~/.ghcup", "Haskell（ghcup）", "ghcup"),
    ("~/.composer", "PHP Composer", "Composer"),
    ("~/.bun", "Bun", "Bun"),
    ("~/.deno", "Deno", "Deno"),
    ("~/.local/share/pnpm", "pnpm", "pnpm"),
    ("~/.swiftpm", "Swift 的包", "Swift"),
    ("~/.konan", "Kotlin/Native", "Kotlin"),
    ("~/.platformio", "PlatformIO", "PlatformIO"),
    ("~/.espressif", "ESP-IDF", "Espressif"),
    ("~/Library/Arduino15", "Arduino", "Arduino"),
    ("~/.minikube", "minikube", "minikube"),
    ("/usr/local/texlive", "TeX Live", "安装包"),
    ("/Library/TeX", "MacTeX", "安装包"),
    ("/Library/Frameworks/R.framework", "R", "安装包"),
];

fn known(c: &Ctx, f: &mut Found) {
    let mut items = Vec::new();
    for (p, title, by) in KNOWN {
        let p = c.at(p);
        if c.is_dir(&p) && c.size(&p) >= 10 * rules::MB {
            let mut i = c.item(title, None, &p, by);
            i.top = c.top(&p, 3);
            items.push(i);
        }
    }
    // installs named by version: /usr/local/mysql-9.4.0-macos15-arm64
    for d in c.dirs("/usr/local").into_iter().filter(|d| d.starts_with("mysql")) {
        let p = join("/usr/local", &d);
        let ver = d.split('-').nth(1).map(str::to_string);
        items.push(c.item(ver.as_ref().map_or_else(|| "MySQL".to_string(), |v| format!("MySQL {v}")), ver, &p, "安装包"));
    }
    let jb = c.home("Library/Application Support/JetBrains");
    if c.is_dir(&jb) {
        let mut i = c.item("JetBrains 的 IDE", None, &jb, "JetBrains");
        i.packages = Some(c.dirs(&jb).len() as u64);
        i.top = c.top(&jb, 3);
        items.push(i);
    }
    f.add("known", "其他认得的", None, items);
}

/// /usr/local's own folders (Homebrew's and the system's layout), not candidates for 其他.
const USR_LOCAL: [&str; 12] = ["bin", "lib", "include", "share", "Cellar", "Homebrew", "opt", "etc", "var", "sbin", "Caskroom", "Frameworks"];
pub const UNKNOWN_MIN: u64 = 100 * rules::MB;

fn unknown(c: &Ctx, f: &mut Found) {
    let home = &c.cfg.home;
    let mut claims: Vec<String> = f.claims.clone();
    claims.extend(c.items.iter().flat_map(|i| i.paths.iter().cloned()));
    claims.extend(["/opt/homebrew".to_string(), format!("{home}/.Trash")]);
    claims.sort();
    claims.dedup();
    // only the outermost claims count
    let outer: Vec<&String> = claims.iter().filter(|p| !claims.iter().any(|q| q != *p && rules::within(p, q))).collect();
    let mut cands: Vec<String> = Vec::new();
    // home's dot folders (read_dir again: ls hides them); ~/.cache, ~/.local/share and ~/.config go one deeper
    let dots: Vec<String> = std::fs::read_dir(c.cfg.real_of(home)).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with('.')).collect()).unwrap_or_default();
    for d in dots {
        let p = join(home, &d);
        if !c.is_dir(&p) {
            continue;
        }
        if [".cache", ".local", ".config"].contains(&d.as_str()) {
            let inner = if d == ".local" { format!("{p}/share") } else { p.clone() };
            cands.extend(c.dirs(&inner).into_iter().map(|k| join(&inner, &k)));
            if d == ".local" {
                cands.extend(c.dirs(&p).into_iter().filter(|k| k != "share").map(|k| join(&p, &k)));
            }
        } else {
            cands.push(p);
        }
    }
    cands.extend(c.dirs("/opt").into_iter().map(|k| join("/opt", &k)));
    if !c.is_dir("/usr/local/Cellar") {
        cands.extend(c.dirs("/usr/local").into_iter().filter(|k| !USR_LOCAL.contains(&k.as_str())).map(|k| join("/usr/local", &k)));
    }
    let mut items = Vec::new();
    for p in cands {
        if outer.iter().any(|q| rules::within(&p, q)) {
            continue;
        }
        let inside: u64 = outer.iter().filter(|q| rules::within(q, &p)).map(|q| c.size(q)).sum();
        let size = c.size(&p).saturating_sub(inside);
        if size >= UNKNOWN_MIN {
            let mut i = c.item(p.rsplit('/').next().unwrap_or(&p), None, &p, "");
            i.size = size;
            i.top = c.top(&p, 3);
            items.push(i);
        }
    }
    items.sort_by(|a, b| b.size.cmp(&a.size));
    f.add("unknown", "其他（认不出来的）", None, items);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_versions() {
        assert_eq!(java_major("1.8.0_461"), "8");
        assert_eq!(java_major("25.0.1"), "25");
        assert_eq!(ext_split("ms-python.python-2026.1.0-darwin-arm64"), ("ms-python.python", "2026.1.0-darwin-arm64"));
        assert_eq!(ext_split("esbenp.prettier-vscode-11.0.0"), ("esbenp.prettier-vscode", "11.0.0"));
        assert!(ver_key("24.16.0") > ver_key("24.11.0") && ver_key("v22.23.0") < ver_key("v24.11.0"));
        assert_eq!(release_field("JAVA_VERSION=\"21.0.5\"\nIMPLEMENTOR=\"Oracle Corporation\"\n", "IMPLEMENTOR").as_deref(), Some("Oracle Corporation"));
    }
}
