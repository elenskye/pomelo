//! The fake Mac (sandbox: the dev build, tests, a build with POMELO_DATA_DIR): a folder shaped like this Mac
//! with made-up projects, caches, downloads and leftovers. Big files are sparse (their length is what the
//! sandbox counts), so the whole thing takes a few dozen MB; the three files duplicates are compared by are
//! real bytes. `.janitor-fake.json` says what the disk, running programs and Docker would say.

use super::rules::{GB, MB};
use super::system::{DockerDetails, DockerPart, Fake, Proc, FAKE_FILE};
use std::fs::{self, File, FileTimes};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Bumped when the fake Mac changes (an older one is made again).
pub const VERSION: u32 = 5;
pub const HOME: &str = "/Users/me";

fn at(root: &Path, virt: &str) -> std::path::PathBuf {
    root.join(virt.trim_start_matches('/'))
}

fn ago(now: SystemTime, days: u64) -> SystemTime {
    now - Duration::from_secs(days * 86400)
}

/// A sparse file of `size`, last changed `days` ago.
fn file(root: &Path, virt: &str, size: u64, days: u64, now: SystemTime) -> Result<(), String> {
    let p = at(root, virt);
    fs::create_dir_all(p.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    let f = File::create(&p).map_err(|e| e.to_string())?;
    f.set_len(size).map_err(|e| e.to_string())?;
    let t = ago(now, days);
    f.set_times(FileTimes::new().set_modified(t).set_accessed(t)).map_err(|e| e.to_string())
}

/// A file with real bytes (`seed` picks the content).
fn bytes(root: &Path, virt: &str, size: usize, seed: u8, days: u64, now: SystemTime) -> Result<(), String> {
    let p = at(root, virt);
    fs::create_dir_all(p.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    let mut f = File::create(&p).map_err(|e| e.to_string())?;
    let chunk: Vec<u8> = (0..1 << 16).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect();
    let mut left = size;
    while left > 0 {
        let n = left.min(chunk.len());
        f.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
        left -= n;
    }
    let t = ago(now, days);
    f.set_times(FileTimes::new().set_modified(t).set_accessed(t)).map_err(|e| e.to_string())
}

/// A small text file (version files, configs).
fn text(root: &Path, virt: &str, body: &str) -> Result<(), String> {
    let p = at(root, virt);
    fs::create_dir_all(p.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    fs::write(&p, body).map_err(|e| e.to_string())
}

/// A relative symlink (the target relative to the link's folder).
fn symlink(root: &Path, virt: &str, to: &str) -> Result<(), String> {
    let p = at(root, virt);
    fs::create_dir_all(p.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    std::os::unix::fs::symlink(to, &p).map_err(|e| e.to_string())
}

fn app(root: &Path, name: &str, id: &str, size: u64, now: SystemTime) -> Result<(), String> {
    let dir = format!("/Applications/{name}.app/Contents");
    let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string></dict></plist>\n");
    let p = at(root, &format!("{dir}/Info.plist"));
    fs::create_dir_all(p.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    fs::write(&p, plist).map_err(|e| e.to_string())?;
    file(root, &format!("{dir}/Frameworks/{name} Framework.framework/{name} Framework"), size, 20, now)
}

fn link(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let t = at(root, to);
    fs::create_dir_all(t.parent().ok_or("no parent")?).map_err(|e| e.to_string())?;
    fs::hard_link(at(root, from), t).map_err(|e| e.to_string())
}

/// Makes the fake Mac in `root` (an empty or missing folder), as of `now`.
pub fn build(root: &Path, now: SystemTime) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let f = |virt: &str, size: u64, days: u64| file(root, &format!("{HOME}/{virt}"), size, days, now);
    let abs = |virt: &str, size: u64, days: u64| file(root, virt, size, days, now);
    // apps
    app(root, "Google Chrome", "com.google.Chrome", 1_100 * MB, now)?;
    app(root, "Visual Studio Code", "com.microsoft.VSCode", 600 * MB, now)?;
    app(root, "Docker", "com.docker.docker", 2 * GB, now)?;
    app(root, "Claude", "com.anthropic.claudefordesktop", 500 * MB, now)?;
    app(root, "Pomelo OS", "com.pomelo.os", 10 * MB, now)?;
    // projects: Pomelo (busy), an old blog (untouched for months), the website, a Python lab, a pnpm app
    f("Documents/GitHub/pomelo/.git/HEAD", 30, 0)?;
    f("Documents/GitHub/pomelo/package.json", 2_000, 0)?;
    f("Documents/GitHub/pomelo/src/main.tsx", 4_000, 0)?;
    f("Documents/GitHub/pomelo/node_modules/vite/dist/index.js", 600 * MB, 2)?;
    f("Documents/GitHub/pomelo/src-tauri/Cargo.toml", 3_000, 1)?;
    f("Documents/GitHub/pomelo/src-tauri/target/debug/deps/libpomelo.rlib", 8 * GB, 0)?;
    f("Documents/GitHub/pomelo/src-tauri/target/debug/incremental/pomelo-1/s.bin", 4_300 * MB, 0)?;
    f("Documents/GitHub/pomelo/src-tauri/target/debug/deps/pomelo_os-abc", 300 * MB, 0)?;
    link(root, &format!("{HOME}/Documents/GitHub/pomelo/src-tauri/target/debug/deps/pomelo_os-abc"), &format!("{HOME}/Documents/GitHub/pomelo/src-tauri/target/debug/pomelo-os"))?;
    f("Documents/GitHub/old-blog/.git/HEAD", 30, 0)?;
    f("Documents/GitHub/old-blog/package.json", 1_000, 120)?;
    f("Documents/GitHub/old-blog/index.html", 8_000, 120)?;
    f("Documents/GitHub/old-blog/node_modules/astro/dist/core.js", 450 * MB, 120)?;
    f("Documents/GitHub/old-blog/dist/index.html", 60 * MB, 120)?;
    f("Documents/GitHub/personal-website/.git/HEAD", 30, 0)?;
    f("Documents/GitHub/personal-website/package.json", 1_000, 3)?;
    f("Documents/GitHub/personal-website/src/content/works/hello.md", 3_000, 3)?;
    f("Documents/GitHub/personal-website/node_modules/astro/dist/core.js", 500 * MB, 40)?;
    f("Documents/GitHub/personal-website/.astro/data-store.json", 25 * MB, 3)?;
    f("Documents/GitHub/ml-lab/.git/HEAD", 30, 0)?;
    f("Documents/GitHub/ml-lab/pyproject.toml", 1_000, 200)?;
    f("Documents/GitHub/ml-lab/train.py", 6_000, 200)?;
    text(root, &format!("{HOME}/Documents/GitHub/ml-lab/.venv/pyvenv.cfg"), "home = /opt/homebrew/bin\ninclude-system-site-packages = false\nversion = 3.12.4\n")?;
    text(root, &format!("{HOME}/Documents/GitHub/ml-lab/.venv/lib/python3.12/site-packages/torch-2.5.1.dist-info/METADATA"), "Name: torch\n")?;
    text(root, &format!("{HOME}/Documents/GitHub/ml-lab/.venv/lib/python3.12/site-packages/numpy-2.1.3.dist-info/METADATA"), "Name: numpy\n")?;
    f("Documents/GitHub/ml-lab/.venv/lib/python3.12/site-packages/torch/libtorch.dylib", 1_200 * MB, 200)?;
    f("Documents/GitHub/ml-lab/lab/__pycache__/model.cpython-312.pyc", 30 * MB, 200)?;
    f("Documents/GitHub/ml-lab/utils/__pycache__/data.cpython-312.pyc", 25 * MB, 200)?;
    f("Documents/GitHub/pnpm-app/package.json", 1_000, 60)?;
    f("Library/pnpm/store/v10/files/aa/react-index", 200 * MB, 60)?;
    link(root, &format!("{HOME}/Library/pnpm/store/v10/files/aa/react-index"), &format!("{HOME}/Documents/GitHub/pnpm-app/node_modules/.pnpm/react@19/node_modules/react/index.js"))?;
    f("Library/pnpm/store/v10/files/bb/old-packages", 1_500 * MB, 90)?;
    // tools' own folders that look like projects (not 一键清理): an editor extension, a conda env's __pycache__
    f(".vscode/extensions/ms-python.python-2026.1/package.json", 2_000, 10)?;
    f(".vscode/extensions/ms-python.python-2026.1/node_modules/vscode-jsonrpc/lib/main.js", 20 * MB, 10)?;
    f(".vscode/extensions/ms-python.python-2026.1/dist/extension.js", 10 * MB, 10)?;
    f(".npm/_npx/6583fba12287d067/node_modules/create-astro/index.js", 50 * MB, 30)?;
    f("Documents/GitHub/personal-website/.vercel/project.json", 200, 3)?;
    f("Documents/GitHub/personal-website/.vercel/output/static/index.html", 30 * MB, 3)?;
    f("Library/Application Support/com.docker.install/Docker.dmg", 50 * MB, 30)?;
    f("Library/Caches/com.apple.Music/SubscriptionPlayCache/a", 20 * MB, 3)?;
    // protected places
    f("Documents/LLM/models/qwen3.5-9b-q4.gguf", 5_500 * MB, 30)?;
    f("Documents/LLM/models/embedding.gguf", 600 * MB, 30)?;
    f("Documents/Seeker/material/证书扫描.pdf", 30 * MB, 80)?;
    f("Documents/Seeker/cv/cn/简历-v3.docx", 80_000, 10)?;
    f("Library/Application Support/Pomelo OS/pomelo.db", 2 * MB, 0)?;
    f("Library/Application Support/Pomelo OS/backups/pomelo-20261101-090000-daily.db", 80 * MB, 3)?;
    f("Library/Keychains/login.keychain-db", MB, 1)?;
    // personal files: big, old, installers, duplicates
    f("Documents/论文草稿.docx", 2 * MB, 5)?;
    f("Downloads/Docker.dmg", 620 * MB, 40)?;
    f("Downloads/旧版微信.dmg", 300 * MB, 400)?;
    f("Downloads/课件.zip", 180 * MB, 90)?;
    f("Downloads/照片备份.zip", 1_600 * MB, 500)?;
    f("Downloads/电影.mp4", 2_500 * MB, 700)?;
    f("Desktop/截图合集.mov", 120 * MB, 500)?;
    bytes(root, &format!("{HOME}/Downloads/讲座录像.mov"), 14 * MB as usize, 7, 30, now)?;
    bytes(root, &format!("{HOME}/Movies/讲座录像 副本.mov"), 14 * MB as usize, 7, 20, now)?;
    bytes(root, &format!("{HOME}/Movies/另一段.mov"), 14 * MB as usize, 9, 20, now)?;
    // caches
    f(".npm/_cacache/content-v2/sha512/aa/bb", 1_400 * MB, 5)?;
    f(".cargo/registry/cache/index.crates.io/serde-1.0.crate", 600 * MB, 10)?;
    f(".cargo/registry/src/index.crates.io/serde-1.0/lib.rs", 900 * MB, 10)?;
    f(".gradle/caches/modules-2/files-2.1/a.jar", 3 * GB, 100)?;
    f(".m2/repository/org/a.jar", 700 * MB, 100)?;
    f(".cache/codex-runtimes/node-24/bin/node", 4_200 * MB, 15)?;
    f("Library/Caches/Homebrew/downloads/miniconda.pkg", 1_100 * MB, 25)?;
    f("Library/Caches/pip/http/a", 300 * MB, 50)?;
    f("Library/Caches/com.google.Chrome/Default/Cache/data", 800 * MB, 0)?;
    f("Library/Caches/Google/Chrome/Default/Code Cache/js", 200 * MB, 0)?;
    f("Library/Caches/com.spotify.client/Data/a", 400 * MB, 2)?;
    f("Library/Caches/com.microsoft.VSCode.ShipIt/update.zip", 250 * MB, 30)?;
    f("Library/Caches/SomeTool/blob", 150 * MB, 12)?;
    f("Library/Caches/com.removed.oldapp/cache.db", 50 * MB, 300)?;
    f("Library/Logs/DiagnosticReports/Pomelo OS-2026-10-01.ips", 300 * MB, 30)?;
    f("Library/Logs/Pomelo/pomelo.log", 200 * MB, 1)?;
    // leftovers of a removed app, and an app's own (not leftovers)
    f("Library/Application Support/com.removed.oldapp/data.sqlite", 300 * MB, 300)?;
    f("Library/Application Support/Code/User/workspaceStorage/a", 400 * MB, 1)?;
    f("Library/Containers/com.removed.oldapp/Data/Library/a", 80 * MB, 300)?;
    f("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw", 9_700 * MB, 1)?;
    f("Library/Preferences/com.removed.oldapp.plist", 2_000, 300)?;
    f("Library/Preferences/com.google.Chrome.plist", 2_000, 1)?;
    f("Library/Saved Application State/com.removed.oldapp.savedState/window_1.data", MB, 300)?;
    f("Library/Mail/V10/MailData/Envelope Index", 1_200 * MB, 0)?;
    f("Library/Containers/com.apple.mail/Data/Library/Mail Downloads/0A1B/报价单.pdf", 240 * MB, 200)?;
    // Xcode
    f("Library/Developer/Xcode/DerivedData/App-abcdef/Build/a", 1_500 * MB, 20)?;
    f("Library/Developer/Xcode/iOS DeviceSupport/16.4 (20E247)/Symbols/a", 2_500 * MB, 300)?;
    f("Library/Developer/Xcode/iOS DeviceSupport/17.2 (21C62)/Symbols/a", 3 * GB, 40)?;
    f("Library/Developer/Xcode/Archives/2025-03-01/App 2025-3-1.xcarchive/Products/a", 400 * MB, 250)?;
    // the Trash
    f(".Trash/旧项目.zip", 600 * MB, 10)?;
    f(".Trash/截图.png", 3 * MB, 10)?;
    // outside home: Java, Conda (an env hard-links its packages), Homebrew, /usr/local
    abs("/Library/Java/JavaVirtualMachines/jdk-25.jdk/Contents/Home/lib/modules", 300 * MB, 60)?;
    abs("/Library/Application Support/Microsoft/MAU2.0/a", 200 * MB, 60)?;
    let conda = "/opt/homebrew/Caskroom/miniconda/base";
    abs(&format!("{conda}/bin/conda"), 2_000, 100)?;
    abs(&format!("{conda}/pkgs/numpy-2.1-py312/lib/libnumpy.dylib"), 400 * MB, 100)?;
    link(root, &format!("{conda}/pkgs/numpy-2.1-py312/lib/libnumpy.dylib"), &format!("{conda}/envs/iss_ml/lib/libnumpy.dylib"))?;
    abs(&format!("{conda}/pkgs/old-torch-2.0/lib/libtorch.dylib"), 2_600 * MB, 300)?;
    abs(&format!("{conda}/pkgs/numpy-2.1-py312.conda"), 400 * MB, 100)?;
    abs(&format!("{conda}/envs/iss_ml/lib/python3.12/site-packages/sklearn/a.so"), 2_600 * MB, 100)?;
    abs(&format!("{conda}/envs/iss_ml/lib/python3.12/site-packages/sklearn/__pycache__/base.cpython-312.pyc"), 10 * MB, 100)?;
    abs("/opt/homebrew/Cellar/node/24.11/bin/node", 100 * MB, 20)?;
    // 开发环境 (阶段 7): JDKs, Conda's envs, Homebrew's Python and a newer node, nvm, rustup, Gradle, editors'
    // extensions, .NET, models, and a few folders nobody claims
    for (jdk, ver, size) in [("jdk-25.jdk", "25.0.1", 0), ("jdk-21.jdk", "21.0.5", 290 * MB), ("jdk-1.8.jdk", "1.8.0_461", 210 * MB)] {
        let home = format!("/Library/Java/JavaVirtualMachines/{jdk}/Contents/Home");
        text(root, &format!("{home}/release"), &format!("JAVA_VERSION=\"{ver}\"\nIMPLEMENTOR=\"Oracle Corporation\"\n"))?;
        if size > 0 {
            abs(&format!("{home}/lib/modules"), size, 200)?;
        }
    }
    for (env, py, pkgs) in [("", "3.13.5", &["conda-24.9.2-py313_0", "pip-24.2-py313_0"][..]), ("envs/iss_ml", "3.12.4", &["numpy-2.1-py312_0", "scikit-learn-1.5.2-py312_0"][..]), ("envs/elen_ai_agent", "3.11.9", &["langchain-0.3.1-py311_0"][..])] {
        let p = if env.is_empty() { conda.to_string() } else { format!("{conda}/{env}") };
        text(root, &format!("{p}/conda-meta/python-{py}-h1a2b3c_0.json"), "{}")?;
        for k in pkgs {
            text(root, &format!("{p}/conda-meta/{k}.json"), "{}")?;
        }
    }
    abs(&format!("{conda}/lib/python3.13/site-packages/conda/core.py"), 230 * MB, 100)?;
    abs(&format!("{conda}/envs/elen_ai_agent/lib/python3.11/site-packages/langchain/a.so"), 330 * MB, 60)?;
    abs("/opt/homebrew/Cellar/node/24.16/bin/node", 103 * MB, 5)?;
    abs("/opt/homebrew/Cellar/python@3.13/3.13.1/bin/python3", 104 * MB, 30)?;
    symlink(root, "/opt/homebrew/bin/python3", "../Cellar/python@3.13/3.13.1/bin/python3")?;
    abs("/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/lib/python3.9/os.py", 55 * MB, 300)?;
    abs("/Library/Developer/CommandLineTools/usr/bin/git", 32 * MB, 300)?;
    for (v, size) in [("v22.23.0", 100), ("v24.11.0", 101), ("v24.16.0", 102)] {
        f(&format!(".nvm/versions/node/{v}/bin/node"), size * MB, 20)?;
    }
    f(".nvm/versions/node/v24.16.0/lib/node_modules/pnpm/dist/pnpm.cjs", 21 * MB, 10)?;
    f(".nvm/versions/node/v24.16.0/lib/node_modules/typescript/lib/tsc.js", 25 * MB, 10)?;
    f(".nvm/versions/node/v24.16.0/lib/node_modules/npm/index.js", 11 * MB, 10)?;
    text(root, &format!("{HOME}/.nvm/alias/default"), "24.16\n")?;
    f(".rustup/toolchains/stable-aarch64-apple-darwin/lib/librustc_driver.dylib", 1_300 * MB, 15)?;
    text(root, &format!("{HOME}/.rustup/settings.toml"), "version = \"12\"\ndefault_toolchain = \"stable-aarch64-apple-darwin\"\n")?;
    for b in ["cargo", "rustc", "rustup"] {
        f(&format!(".cargo/bin/{b}"), 30_000, 15)?;
    }
    f(".cargo/bin/cargo-watch", 12 * MB, 15)?;
    f(".gradle/wrapper/dists/gradle-8.5-bin/abc123/gradle-8.5/lib/gradle-core.jar", 150 * MB, 100)?;
    f(".vscode/extensions/ms-python.python-2025.9.0/dist/extension.js", 26 * MB, 200)?;
    f(".vscode/extensions/esbenp.prettier-vscode-11.0.0/dist/extension.js", 15 * MB, 60)?;
    abs("/usr/local/share/dotnet/sdk/9.0.100/dotnet.dll", 200 * MB, 40)?;
    abs("/usr/local/share/dotnet/sdk/8.0.400/dotnet.dll", 180 * MB, 200)?;
    f(".nuget/packages/newtonsoft.json/13.0.3/lib.dll", 5 * MB, 100)?;
    f(".ollama/models/blobs/sha256-qwen3", 4_100 * MB, 50)?;
    f(".mystery-tool/data/blob.bin", 310 * MB, 90)?;
    f(".cache/whisper/large-v3.pt", 1_500 * MB, 120)?;
    abs("/opt/someapp/lib/libsomeapp.dylib", 200 * MB, 300)?;
    abs("/usr/local/mysql-9.4.0-macos15-arm64/bin/mysqld", 160 * MB, 200)?;
    symlink(root, "/usr/local/mysql", "mysql-9.4.0-macos15-arm64")?;
    f(".claude/projects/pomelo/session.jsonl", 120 * MB, 0)?;
    abs("/usr/local/lib/libsomething.dylib", 50 * MB, 200)?;
    let gb = |n: f64| (n * GB as f64) as u64;
    let fake = Fake {
        disk_total: 460 * GB,
        system: 30 * GB,
        procs: vec![Proc { name: "node".into(), cwd: Some(format!("{HOME}/Documents/GitHub/pomelo")) }, Proc { name: "cargo".into(), cwd: Some(format!("{HOME}/Documents/GitHub/pomelo/src-tauri")) }],
        apps: vec!["com.google.Chrome".into()],
        docker: Some(vec![
            DockerPart { name: "镜像".into(), size: gb(6.1), reclaimable: gb(3.9) },
            DockerPart { name: "容器".into(), size: gb(0.4), reclaimable: gb(0.4) },
            DockerPart { name: "数据卷".into(), size: gb(2.2), reclaimable: gb(1.1) },
            DockerPart { name: "构建缓存".into(), size: gb(1.0), reclaimable: gb(1.0) },
        ]),
        full_disk_access: true,
        java_home: Some("/Library/Java/JavaVirtualMachines/jdk-25.jdk/Contents/Home".into()),
        xcode_select: Some("/Library/Developer/CommandLineTools".into()),
        docker_details: Some(DockerDetails { images: vec![("python:3.12".into(), GB), ("postgres:16".into(), 450 * MB), ("node:24-alpine".into(), 180 * MB)], containers: 3, volumes: 2 }),
    };
    let mut v = serde_json::to_value(&fake).map_err(|e| e.to_string())?;
    v["version"] = serde_json::json!(VERSION);
    fs::write(root.join(FAKE_FILE), v.to_string()).map_err(|e| e.to_string())
}

/// The fake Mac in `root`, made (again) when missing or older.
pub fn ensure(root: &Path) -> Result<(), String> {
    let current = fs::read_to_string(root.join(FAKE_FILE)).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|v| v["version"].as_u64()) == Some(VERSION as u64);
    if current {
        return Ok(());
    }
    if root.exists() {
        fs::remove_dir_all(root).map_err(|e| e.to_string())?;
    }
    build(root, SystemTime::now())
}
