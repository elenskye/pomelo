//! What the program's own scan recognizes (requirements「DevJanitor」): the 一键清理 categories, the listed
//! groups and the protected places. Paths are "virtual" (absolute as on the Mac, `/Users/me/...`); the scan
//! maps them onto its root (`/`, or the fake Mac's folder in the sandbox).
//! - 一键清理 only holds what rebuilds and loses nothing: project dirs (a name next to its project's marker
//!   file), fixed places under home (package caches, Gradle / Maven, Xcode, Codex, logs), the known App caches
//!   (`APP_CACHES`, plus updaters' `*.ShipIt`), Conda's `pkgs` (by `conda clean`). Never inside a protected
//!   place (the website's deps and build outputs aren't protected).
//! - Default-checked: download caches (`Category::download`) and the deps / builds of a project untouched for
//!   `stale_days` (its newest file outside those dirs and `.git`); never the website's node_modules.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    /// How it comes back, roughly how long, whether it needs the network.
    pub rebuild: &'static str,
    pub takes: &'static str,
    pub network: bool,
    /// A download cache: checked by default.
    pub download: bool,
}

const fn cat(id: &'static str, name: &'static str, rebuild: &'static str, takes: &'static str, network: bool, download: bool) -> Category {
    Category { id, name, rebuild, takes, network, download }
}

pub const NODE_MODULES: &str = "node_modules";
pub const RUST_TARGET: &str = "rust_target";
pub const PYTHON: &str = "python";
pub const WEB_BUILD: &str = "web_build";
pub const PKG_CACHE: &str = "pkg_cache";
pub const CONDA: &str = "conda";
pub const JVM: &str = "jvm";
pub const XCODE: &str = "xcode";
pub const APP_CACHE: &str = "app_cache";
pub const CODEX: &str = "codex";
pub const LOGS: &str = "logs";

pub const CATEGORIES: [Category; 11] = [
    cat(NODE_MODULES, "项目里的 node_modules", "在项目里 pnpm install（或 npm install）", "一两分钟", true, false),
    cat(RUST_TARGET, "Rust 的 target", "下次编译时重建", "几分钟（Pomelo 约一分半）", false, false),
    cat(PYTHON, "Python 虚拟环境和 __pycache__", "按项目的依赖清单重新装；__pycache__ 运行时自己生成", "几分钟", true, false),
    cat(WEB_BUILD, "前端的构建产物", "下次构建时重建（npm run build）", "几十秒", false, false),
    cat(PKG_CACHE, "包管理器的下载缓存", "用到时重新下载", "看网速", true, true),
    cat(CONDA, "Conda 的包缓存", "conda clean 清掉没在用的包；以后装包时重新下载", "看网速", true, true),
    cat(JVM, "Gradle、Maven 的缓存", "下次构建时重新下载", "看网速", true, true),
    cat(XCODE, "Xcode 的编译缓存和旧文件", "下次编译时重建；连上设备时重新准备", "几分钟", true, false),
    cat(APP_CACHE, "各个 App 的缓存", "App 用到时自己再生成", "马上", false, false),
    cat(CODEX, "Codex 的运行时", "Codex 用到时重新下载", "几分钟", true, false),
    cat(LOGS, "日志", "不用重建", "", false, false),
];

pub fn category(id: &str) -> Option<&'static Category> {
    CATEGORIES.iter().find(|c| c.id == id)
}

/// The listed groups (only listed; picked ones go to the Trash).
pub const BIG: &str = "big";
pub const INSTALLERS: &str = "installers";
pub const LEFTOVERS: &str = "leftovers";
pub const DUPLICATES: &str = "duplicates";
pub const UNKNOWN_CACHE: &str = "unknown_cache";
pub const ARCHIVES: &str = "archives";
pub const MAIL: &str = "mail";

pub const GROUPS: [(&str, &str); 7] = [
    (BIG, "大文件和很久没打开的文件"),
    (INSTALLERS, "下载里的安装包和压缩包"),
    (LEFTOVERS, "删掉的 App 留下的文件"),
    (DUPLICATES, "重复的文件"),
    (UNKNOWN_CACHE, "不认得的 App 缓存"),
    (ARCHIVES, "Xcode 的 Archives"),
    (MAIL, "邮件（Mail）的数据"),
];

/// Mail's places (under home; Full Disk Access): title, what deleting means. Listed, never 一键清理.
pub const MAIL_PLACES: [(&str, &str, &str); 3] = [
    ("Library/Mail", "Mail 的邮箱和索引", "Mail 存在这台 Mac 上的邮件。不用 Mail 的话，先在「系统设置 → 互联网账户」里把每个账户的「邮件」关掉，不然打开 Mail 会重新下载；「我的 Mac 上」的邮箱只存在这里，删了就没了"),
    ("Library/Containers/com.apple.mail/Data/Library/Mail Downloads", "Mail 打开过的附件", "打开附件时存的副本，邮件里还有原件"),
    ("Library/Containers/com.apple.mail/Data/Library/Caches", "Mail 的缓存", "Mail 用到时自己再生成"),
];

/// Decimal, as Finder counts.
pub const GB: u64 = 1_000_000_000;
pub const MB: u64 = 1_000_000;
/// 大文件; 很久没打开 (and older than a year); 重复 (at least this big).
pub const BIG_FILE: u64 = GB;
pub const OLD_FILE: u64 = 100 * MB;
pub const OLD_DAYS: i64 = 365;
pub const DUP_FILE: u64 = 10 * MB;
pub const INSTALLER_FILE: u64 = MB;
pub const INSTALLER_EXT: [&str; 13] = ["dmg", "pkg", "mpkg", "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "iso", "xip"];

/// Project dirs: (name, category, what the parent must hold — or the dir itself, for venvs).
pub enum Needs {
    Parent(&'static [&'static str]),
    Inside(&'static str),
    /// The parent folder is called this.
    Under(&'static str),
    Any,
}

pub const PROJECT_DIRS: [(&str, &str, Needs); 13] = [
    ("node_modules", NODE_MODULES, Needs::Parent(&["package.json"])),
    ("target", RUST_TARGET, Needs::Parent(&["Cargo.toml"])),
    (".venv", PYTHON, Needs::Inside("pyvenv.cfg")),
    ("venv", PYTHON, Needs::Inside("pyvenv.cfg")),
    ("env", PYTHON, Needs::Inside("pyvenv.cfg")),
    ("__pycache__", PYTHON, Needs::Any),
    ("dist", WEB_BUILD, Needs::Parent(&["package.json"])),
    (".next", WEB_BUILD, Needs::Parent(&["package.json"])),
    (".astro", WEB_BUILD, Needs::Parent(&["package.json"])),
    (".turbo", WEB_BUILD, Needs::Parent(&["package.json"])),
    (".nuxt", WEB_BUILD, Needs::Parent(&["package.json"])),
    (".svelte-kit", WEB_BUILD, Needs::Parent(&["package.json"])),
    ("output", WEB_BUILD, Needs::Under(".vercel")),
];

/// Where projects live: under home, outside ~/Library and dot folders (~/.vscode/extensions, ~/.nvm,
/// ~/.npm are tools' own; their node_modules and dist aren't a project's to rebuild).
pub fn in_projects(home: &str, virt: &str) -> bool {
    let Some(rel) = virt.strip_prefix(home).and_then(|r| r.strip_prefix('/')) else { return false };
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() > 1 && parts[0] != "Library" && parts[..parts.len() - 1].iter().all(|p| !p.starts_with('.'))
}

/// The marker files the walker notes in each folder (bit = index).
pub const MARKERS: [&str; 9] = ["package.json", "Cargo.toml", "pyvenv.cfg", "pyproject.toml", "setup.py", "requirements.txt", ".git", "CACHEDIR.TAG", "environment.yml"];

pub fn marker_bit(name: &str) -> u16 {
    MARKERS.iter().position(|m| *m == name).map_or(0, |i| 1 << i)
}

pub fn markers_have(bits: u16, names: &[&str]) -> bool {
    names.iter().any(|n| bits & marker_bit(n) != 0)
}

/// A folder that looks like a project's root (for grouping `__pycache__`).
pub fn project_root(bits: u16) -> bool {
    markers_have(bits, &["package.json", "Cargo.toml", "pyproject.toml", "setup.py", "requirements.txt", ".git", "environment.yml"])
}

/// Fixed places under home: (path under home, category, title, how it's cleaned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Delete the folder.
    Remove,
    /// Delete what's in it (the folder stays).
    Empty,
}

pub const HOME_PLACES: [(&str, &str, &str, How); 25] = [
    (".npm/_cacache", PKG_CACHE, "npm 的下载缓存", How::Remove),
    (".npm/_npx", PKG_CACHE, "npx 的缓存", How::Remove),
    ("Library/pnpm/store", PKG_CACHE, "pnpm 的包仓库", How::Remove),
    (".pnpm-store", PKG_CACHE, "pnpm 的包仓库", How::Remove),
    ("Library/Caches/pnpm", PKG_CACHE, "pnpm 的缓存", How::Remove),
    (".cargo/registry/cache", PKG_CACHE, "cargo 下载的包", How::Remove),
    (".cargo/registry/src", PKG_CACHE, "cargo 解压的源码", How::Remove),
    (".cargo/git", PKG_CACHE, "cargo 的 git 依赖", How::Remove),
    ("Library/Caches/pip", PKG_CACHE, "pip 的下载缓存", How::Remove),
    ("Library/Caches/Homebrew", PKG_CACHE, "Homebrew 的下载缓存", How::Remove),
    ("Library/Caches/Yarn", PKG_CACHE, "Yarn 的缓存", How::Remove),
    (".bun/install/cache", PKG_CACHE, "Bun 的缓存", How::Remove),
    (".cache/uv", PKG_CACHE, "uv 的缓存", How::Remove),
    ("Library/Caches/pypoetry", PKG_CACHE, "Poetry 的缓存", How::Remove),
    ("Library/Caches/go-build", PKG_CACHE, "Go 的编译缓存", How::Remove),
    ("Library/Caches/ms-playwright", PKG_CACHE, "Playwright 的浏览器", How::Remove),
    ("Library/Caches/node-gyp", PKG_CACHE, "node-gyp 的头文件", How::Remove),
    ("Library/Caches/electron", PKG_CACHE, "Electron 的下载", How::Remove),
    (".gradle/caches", JVM, "Gradle 的缓存", How::Remove),
    (".gradle/wrapper/dists", JVM, "Gradle 下载的版本", How::Remove),
    (".m2/repository", JVM, "Maven 的本地仓库", How::Remove),
    ("Library/Developer/Xcode/DerivedData", XCODE, "Xcode 的 DerivedData", How::Remove),
    ("Library/Developer/CoreSimulator/Caches", XCODE, "模拟器的缓存", How::Remove),
    (".cache/codex-runtimes", CODEX, "Codex 的运行时", How::Remove),
    ("Library/Logs", LOGS, "资源库里的日志", How::Empty),
];

/// Xcode's 设备支持: every version but the newest of each.
pub const DEVICE_SUPPORT: [&str; 3] = ["Library/Developer/Xcode/iOS DeviceSupport", "Library/Developer/Xcode/watchOS DeviceSupport", "Library/Developer/Xcode/tvOS DeviceSupport"];
pub const ARCHIVES_DIR: &str = "Library/Developer/Xcode/Archives";
pub const CACHES_DIR: &str = "Library/Caches";

/// Where Conda may live (absolute, or under home with `~/`); its `pkgs` is the cache.
pub const CONDA_ROOTS: [&str; 10] = [
    "/opt/homebrew/Caskroom/miniconda/base",
    "/opt/homebrew/Caskroom/miniforge/base",
    "/opt/homebrew/Caskroom/anaconda/base",
    "/usr/local/Caskroom/miniconda/base",
    "/opt/miniconda3",
    "/opt/anaconda3",
    "~/miniconda3",
    "~/anaconda3",
    "~/miniforge3",
    "~/mambaforge",
];

/// Known App caches in ~/Library/Caches (deleting them only costs a reload): the folder name, the app
/// whose running makes it 「在用」, the name shown.
pub const APP_CACHES: [(&str, &str, &str); 28] = [
    ("com.elenskye.pomelo", "com.elenskye.pomelo", "Pomelo OS"),
    ("com.google.Chrome", "com.google.Chrome", "Chrome"),
    ("Google", "com.google.Chrome", "Google"),
    ("com.microsoft.edgemac", "com.microsoft.edgemac", "Edge"),
    ("Microsoft Edge", "com.microsoft.edgemac", "Microsoft Edge"),
    ("com.apple.Safari", "com.apple.Safari", "Safari"),
    ("org.mozilla.firefox", "org.mozilla.firefox", "Firefox"),
    ("Firefox", "org.mozilla.firefox", "Firefox 配置"),
    ("com.brave.Browser", "com.brave.Browser", "Brave"),
    ("BraveSoftware", "com.brave.Browser", "BraveSoftware"),
    ("company.thebrowser.Browser", "company.thebrowser.Browser", "Arc"),
    ("com.operasoftware.Opera", "com.operasoftware.Opera", "Opera"),
    ("com.microsoft.VSCode", "com.microsoft.VSCode", "VS Code"),
    ("com.todesktop.230313mzl4w4u92", "com.todesktop.230313mzl4w4u92", "Cursor"),
    ("com.tinyspeck.slackmacgap", "com.tinyspeck.slackmacgap", "Slack"),
    ("com.hnc.Discord", "com.hnc.Discord", "Discord"),
    ("com.figma.Desktop", "com.figma.Desktop", "Figma"),
    ("com.spotify.client", "com.spotify.client", "Spotify"),
    ("com.apple.dt.Xcode", "com.apple.dt.Xcode", "Xcode"),
    ("com.github.GitHubClient", "com.github.GitHubClient", "GitHub Desktop"),
    ("com.postmanlabs.mac", "com.postmanlabs.mac", "Postman"),
    ("us.zoom.xos", "us.zoom.xos", "Zoom"),
    ("com.anthropic.claudefordesktop", "com.anthropic.claudefordesktop", "Claude"),
    ("com.openai.chat", "com.openai.chat", "ChatGPT"),
    ("JetBrains", "", "JetBrains"),
    ("typescript", "", "TypeScript"),
    ("Cypress", "", "Cypress"),
    ("electron-builder", "", "electron-builder"),
];

/// A known App cache: the app it belongs to ("" = none to check).
pub fn app_cache(name: &str) -> Option<&'static str> {
    if name.ends_with(".ShipIt") {
        return Some("");
    }
    APP_CACHES.iter().find(|(n, ..)| *n == name).map(|(_, app, _)| *app)
}

/// How a known App cache is called on the page: 「Chrome 的缓存」, 「com.microsoft.VSCode 的更新下载」.
pub fn app_cache_title(name: &str) -> String {
    if let Some(app) = name.strip_suffix(".ShipIt") {
        let shown = APP_CACHES.iter().find(|(n, ..)| *n == app).map_or(app, |(.., t)| *t);
        return format!("{shown} 的更新下载");
    }
    APP_CACHES.iter().find(|(n, ..)| *n == name).map_or_else(|| name.to_string(), |(.., t)| format!("{t} 的缓存"))
}

/// macOS's own caches (not listed as 不认得的 App 缓存).
pub fn apple_cache(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("com.apple.") || ["GeoServices", "PassKit", "Animoji", "CloudKit", "familycircled", "Metadata", "SpeechModelCache", "FamilyCircle", "Siri"].contains(&name)
}

/// Bundles the program never looks into for single files (an app's parts, a Photos library…).
pub const BUNDLES: [&str; 12] = ["app", "photoslibrary", "musiclibrary", "tvlibrary", "photolibrary", "fcpbundle", "logicx", "band", "framework", "bundle", "xcarchive", "imovielibrary"];

pub fn is_bundle(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| BUNDLES.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The website's folders that rebuild (not protected).
pub const BUILD_DIRS: [&str; 6] = ["node_modules", "dist", ".astro", ".turbo", ".next", ".vercel"];

/// Where Pomelo's data lives, under home.
pub const POMELO_DATA: [&str; 2] = ["Library/Application Support/Pomelo OS", "Library/Application Support/Pomelo OS Dev"];

/// System folders (outside home).
const SYSTEM: [&str; 9] = ["/System", "/bin", "/sbin", "/usr", "/private", "/cores", "/Library/Apple", "/Library/Keychains", "/Library/Developer/CommandLineTools"];

/// Protected places, as the requirements list them. Shared by the one-click list (never enters), the
/// listed groups and the Agent's cards (marked, confirmed twice).
#[derive(Debug, Clone)]
pub struct Protected {
    places: Vec<(String, &'static str)>,
    site: String,
}

pub fn within(path: &str, dir: &str) -> bool {
    path == dir || (path.starts_with(dir) && path.as_bytes().get(dir.len()) == Some(&b'/'))
}

impl Protected {
    /// `home` and `site` are virtual paths.
    pub fn new(home: &str, site: &str) -> Self {
        let mut places: Vec<(String, &'static str)> = POMELO_DATA.iter().map(|p| (format!("{home}/{p}"), "Pomelo 的数据")).collect();
        places.push((format!("{home}/Library/Keychains"), "钥匙串"));
        places.push((format!("{home}/Documents/LLM"), "本地模型"));
        places.push((format!("{home}/Documents/Seeker"), "Seeker 资料"));
        places.extend(SYSTEM.iter().map(|p| (p.to_string(), "系统")));
        Self { places, site: site.trim_end_matches('/').to_string() }
    }

    /// Why a path is protected (None: it isn't). `/usr/local` is not the system's.
    /// A folder holding a protected place is protected too (deleting it would take that along).
    pub fn of(&self, path: &str) -> Option<&'static str> {
        if path == "/" {
            return Some("系统");
        }
        if within(path, &self.site) {
            return if path[self.site.len()..].split('/').any(|c| BUILD_DIRS.contains(&c)) { None } else { Some("个人网站") };
        }
        if within(path, "/usr/local") {
            return None;
        }
        if let Some((_, why)) = self.places.iter().find(|(p, _)| within(path, p) || within(p, path)) {
            return Some(why);
        }
        within(&self.site, path).then_some("个人网站")
    }

    pub fn site(&self) -> &str {
        &self.site
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_places() {
        let p = Protected::new("/Users/me", "/Users/me/Documents/GitHub/personal-website");
        assert_eq!(p.of("/Users/me/Documents/LLM/models/a.gguf"), Some("本地模型"));
        assert_eq!(p.of("/Users/me/Documents/Seeker"), Some("Seeker 资料"));
        assert_eq!(p.of("/Users/me/Library/Application Support/Pomelo OS/backups"), Some("Pomelo 的数据"));
        assert_eq!(p.of("/Users/me/Library/Keychains/login.keychain-db"), Some("钥匙串"));
        assert_eq!(p.of("/System/Library"), Some("系统"));
        assert_eq!(p.of("/usr/local/bin"), None);
        assert_eq!(p.of("/usr/bin"), Some("系统"));
        // the website's sources are protected, its deps and builds aren't
        assert_eq!(p.of("/Users/me/Documents/GitHub/personal-website/src/content"), Some("个人网站"));
        assert_eq!(p.of("/Users/me/Documents/GitHub/personal-website/node_modules"), None);
        assert_eq!(p.of("/Users/me/Documents/GitHub/personal-website/.astro"), None);
        // a folder holding a protected place is protected too (deleting it would take that along)
        assert_eq!(p.of("/Users/me/Documents"), Some("本地模型"));
        assert_eq!(p.of("/Users/me/Documents/LLMx"), None);
        assert_eq!(p.of("/Users/me/Downloads/a.dmg"), None);
    }

    #[test]
    fn known_caches_and_bundles() {
        assert_eq!(app_cache("com.google.Chrome"), Some("com.google.Chrome"));
        assert_eq!(app_cache("com.microsoft.VSCode.ShipIt"), Some(""));
        assert_eq!(app_cache("com.unknown.thing"), None);
        assert!(is_bundle("Google Chrome.app") && is_bundle("Photos Library.photoslibrary") && !is_bundle("movie.mp4"));
        assert!(markers_have(marker_bit("Cargo.toml") | marker_bit(".git"), &["Cargo.toml"]) && !project_root(marker_bit("pyvenv.cfg")));
    }
}
