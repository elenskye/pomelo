use super::rules::{GB, MB};
use super::*;
use crate::db::tests::{temp_dir, TempDir};

fn sandbox(name: &str) -> (TempDir, Janitor) {
    let dir = temp_dir(&format!("janitor-{name}"));
    let j = Janitor::new(Mode::Sandbox, Path::new("/Users/nobody"), &dir);
    ensure_sandbox(&j.root).unwrap();
    (dir, j)
}

const SITE: &str = "/Users/me/Documents/GitHub/personal-website";

fn scanned(j: &Janitor) -> Overview {
    let out = scan::scan(&j.config(SITE, vec![]), &j.sys(), &Progress::default()).unwrap();
    let envs = serde_json::to_value(&out.envs).unwrap();
    j.with_db(|c| store::replace(c, &out.summary, &out.nodes, &out.items, &envs)).unwrap();
    overview(j, &Settings::default(), &[]).unwrap()
}

fn one_click(ov: &Overview, title: &str) -> Item {
    ov.categories.iter().flat_map(|g| &g.items).find(|i| i.title == title).cloned().unwrap_or_else(|| panic!("no 一键清理 item {title}: {:?}", ov.categories.iter().flat_map(|g| g.items.iter().map(|i| i.title.clone())).collect::<Vec<_>>()))
}

fn group<'a>(ov: &'a Overview, id: &str) -> Vec<&'a Item> {
    ov.groups.iter().filter(|g| g.id == id).flat_map(|g| &g.items).collect()
}

#[test]
fn the_fake_mac_is_scanned() {
    let (_d, j) = sandbox("scan");
    let ov = scanned(&j);
    // projects: untouched for months → checked; busy → in use; the website's node_modules never by default
    let blog = one_click(&ov, "old-blog · node_modules");
    assert!(blog.checked && blog.in_use.is_none());
    assert_eq!(blog.size, 450 * MB);
    assert!(one_click(&ov, "old-blog · dist").checked);
    let nm = one_click(&ov, "pomelo · node_modules");
    assert!(!nm.checked && nm.in_use.as_deref() == Some("node 在跑"));
    // a hard link inside target counts once and is freed with it
    let target = one_click(&ov, "pomelo · src-tauri/target");
    assert_eq!(target.size, 8 * GB + 4_300 * MB + 300 * MB);
    assert!(target.in_use.is_some());
    let site = one_click(&ov, "personal-website · node_modules");
    assert!(!site.checked && site.note.is_some());
    assert!(!one_click(&ov, "personal-website · .astro").checked);
    let venv = one_click(&ov, "ml-lab · .venv");
    assert!(venv.checked);
    let pyc = one_click(&ov, "ml-lab · __pycache__");
    assert_eq!((pyc.paths.len(), pyc.size, pyc.checked), (2, 55 * MB, true));
    // pnpm: node_modules links into the store, so it frees nothing (no row) and the store only its own files
    assert!(ov.categories.iter().flat_map(|g| &g.items).all(|i| !i.title.starts_with("pnpm-app")));
    assert_eq!(one_click(&ov, "pnpm 的包仓库").size, 1_500 * MB);
    // download caches are checked; Codex, logs, App caches aren't; a running app's cache is in use
    for t in ["npm 的下载缓存", "Gradle 的缓存", "Maven 的本地仓库", "Homebrew 的下载缓存", "pip 的下载缓存"] {
        assert!(one_click(&ov, t).checked, "{t}");
    }
    for t in ["Codex 的运行时", "资源库里的日志", "Spotify 的缓存", "VS Code 的更新下载"] {
        assert!(!one_click(&ov, t).checked, "{t}");
    }
    assert_eq!(one_click(&ov, "Chrome 的缓存").in_use.as_deref(), Some("App 开着"));
    // cargo is running: its caches are busy
    assert_eq!(one_click(&ov, "cargo 下载的包").in_use.as_deref(), Some("cargo 在跑"));
    // Conda: the package an env links to stays; the rest and the tarball go
    let conda = ov.categories.iter().find(|g| g.id == rules::CONDA).unwrap();
    assert_eq!(conda.items[0].size, 3 * GB);
    // Xcode: only the older 设备支持
    let xcode: Vec<String> = ov.categories.iter().find(|g| g.id == rules::XCODE).unwrap().items.iter().map(|i| i.title.clone()).collect();
    assert!(xcode.contains(&"iOS 设备支持 16.4 (20E247)".to_string()) && !xcode.iter().any(|t| t.contains("17.2")), "{xcode:?}");
    // tools' own folders aren't projects; the npx cache and the website's .vercel/output are
    let all: Vec<&Item> = ov.categories.iter().flat_map(|g| &g.items).collect();
    assert!(all.iter().all(|i| !i.paths[0].contains("/.vscode/") && !i.paths[0].contains("/envs/")), "{:?}", all.iter().map(|i| &i.paths[0]).collect::<Vec<_>>());
    assert!(one_click(&ov, "npx 的缓存").checked);
    assert_eq!(one_click(&ov, "personal-website · .vercel/output").size, 30 * MB);
    // nothing in 一键清理 is protected
    assert!(ov.categories.iter().flat_map(|g| &g.items).all(|i| i.protected.is_none() && !i.paths.iter().any(|p| p.contains("/Documents/LLM"))));

    // listed groups
    let big: Vec<&str> = group(&ov, rules::BIG).iter().map(|i| i.title.as_str()).collect();
    for t in ["照片备份.zip", "电影.mp4", "截图合集.mov", "旧版微信.dmg", "qwen3.5-9b-q4.gguf"] {
        assert!(big.contains(&t), "{t} not in {big:?}");
    }
    assert!(!big.contains(&"Docker.dmg") && !big.contains(&"embedding.gguf") && !big.contains(&"旧项目.zip"));
    let model = group(&ov, rules::BIG).into_iter().find(|i| i.title == "qwen3.5-9b-q4.gguf").unwrap();
    assert_eq!(model.protected.as_deref(), Some("本地模型"));
    assert!(group(&ov, rules::BIG).iter().find(|i| i.title == "电影.mp4").unwrap().tag.as_deref().unwrap().starts_with("很久没打开"));
    let mut installers: Vec<&str> = group(&ov, rules::INSTALLERS).iter().map(|i| i.title.as_str()).collect();
    installers.sort();
    assert_eq!(installers, ["Docker.dmg", "旧版微信.dmg", "照片备份.zip", "课件.zip"]);
    let left = group(&ov, rules::LEFTOVERS);
    assert_eq!(left.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["com.removed.oldapp"]);
    assert_eq!(left[0].paths.len(), 5);
    let dups = group(&ov, rules::DUPLICATES);
    assert_eq!(dups.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["讲座录像.mov", "讲座录像 副本.mov"]);
    assert!(dups.iter().all(|i| i.dup == Some(1)));
    // Apple's own caches aren't listed; com.docker.install belongs with Docker (installed)
    assert_eq!(group(&ov, rules::UNKNOWN_CACHE).iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["SomeTool"]);
    assert_eq!(group(&ov, rules::ARCHIVES).len(), 1);

    // the summary and the ring
    let s = ov.summary.as_ref().unwrap();
    assert_eq!(s["disk"]["total"].as_u64(), Some(460 * GB));
    assert_eq!(s["disk"]["system"].as_u64(), Some(30 * GB));
    assert_eq!(s["trash"]["count"].as_u64(), Some(2));
    assert_eq!(s["docker"]["size"].as_u64(), Some(9_700 * MB));
    assert_eq!(s["docker"]["parts"].as_array().map(Vec::len), Some(4));
    let ring = ring_view(&j, "/").unwrap().unwrap();
    let names: Vec<&str> = ring.node.children.iter().map(|c| c.name.as_str()).collect();
    for n in ["App", "个人文件夹", "资源库", "/opt", "/usr/local", "系统和其他"] {
        assert!(names.contains(&n), "{n} not in {names:?}");
    }
    let home = ring_view(&j, "/Users/me").unwrap().unwrap();
    assert_eq!(home.crumbs, [("/".to_string(), "整块磁盘".to_string())]);
    assert!(home.node.children.iter().any(|c| c.name == "Documents"));
    assert!(ov.cleanable > 20 * GB && ov.cleanable < one_click_total(&ov));
    assert_eq!(live_text(12_345_000_000).as_deref(), Some("可清 12.3 GB"));
    assert_eq!(live_text(420 * MB).as_deref(), Some("可清 420 MB"));
}

fn one_click_total(ov: &Overview) -> u64 {
    ov.categories.iter().map(|g| g.size).sum()
}

#[test]
fn the_fake_macs_environments() {
    let (_d, j) = sandbox("envs");
    scanned(&j);
    let envs = j.envs();
    let env = |id: &str| envs.iter().find(|e| e.id == id).unwrap_or_else(|| panic!("no {id}: {:?}", envs.iter().map(|e| &e.id).collect::<Vec<_>>()));
    let row = |id: &str, title: &str| env(id).items.iter().find(|i| i.title == title).cloned().unwrap_or_else(|| panic!("no {title} in {id}: {:?}", env(id).items.iter().map(|i| &i.title).collect::<Vec<_>>()));
    // Java: three JDKs from their release files, the default from java_home, the admin note
    let java = env("java");
    assert_eq!(java.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["JDK 8（Oracle）", "JDK 21（Oracle）", "JDK 25（Oracle）"]);
    assert!(row("java", "JDK 25（Oracle）").default && row("java", "JDK 8（Oracle）").version.as_deref() == Some("1.8.0_461"));
    assert!(java.items.iter().all(|i| i.admin));
    // Conda: base (default) without its envs and pkgs, each env's Python and packages, the cache
    let base = row("conda", "base（miniconda）");
    assert!(base.default && base.version.as_deref() == Some("Python 3.13.5") && base.packages == Some(3));
    let iss = row("conda", "iss_ml");
    assert_eq!((iss.version.as_deref(), iss.packages), (Some("Python 3.12.4"), Some(3)));
    assert_eq!(iss.top[0].0, "sklearn");
    assert_eq!(row("conda", "elen_ai_agent").version.as_deref(), Some("Python 3.11.9"));
    assert!(row("conda", "包缓存（pkgs）").tag.is_some());
    // Python: Homebrew's is the default (bin/python3 points at it); the system's is protected; the venv
    assert!(row("python", "Python 3.13.1").default);
    assert_eq!(row("python", "Python 3.9").protected.as_deref(), Some("系统"));
    let venv = row("python", "ml-lab · .venv");
    assert_eq!((venv.version.as_deref(), venv.packages), (Some("Python 3.12.4"), Some(2)));
    // Node: nvm's default 24.16 with its global packages; Homebrew's older version
    let n = row("node", "Node 24.16.0");
    assert!(n.default && n.packages == Some(2) && n.by.as_deref() == Some("nvm"));
    assert_eq!(n.top.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["typescript", "pnpm"]);
    assert_eq!(row("node", "Node 24.11").tag.as_deref(), Some("旧版本"));
    // Rust: the default toolchain, cargo's own tools, the projects' target
    assert!(row("rust", "stable-aarch64-apple-darwin").default);
    assert_eq!(row("rust", "cargo 的工具和仓库").packages, Some(1));
    assert!(env("rust").items.iter().any(|i| i.title == "pomelo · src-tauri/target"));
    // Homebrew: formulae and casks; the old node is a part (not added twice)
    let brew = row("homebrew", "Homebrew（/opt/homebrew）");
    assert_eq!(env("homebrew").note.as_deref(), Some("2 个包、1 个 App"));
    let old = row("homebrew", "node 24.11");
    assert!(old.part && env("homebrew").size == brew.size);
    // Gradle, Docker, Xcode, editors, .NET, the known and the unknown
    assert!(env("jvm").items.iter().any(|i| i.title == "Gradle 8.5"));
    assert_eq!(env("docker").note.as_deref(), Some("3 个镜像 · 3 个容器 · 2 个数据卷"));
    assert!(row("docker", "python:3.12").part);
    assert!(row("xcode", "命令行工具").default);
    let ext = row("editors", "VS Code 的扩展");
    assert_eq!(ext.packages, Some(3));
    assert!(row("editors", "ms-python.python-2025.9.0").part);
    assert!(row("dotnet", ".NET SDK 9.0.100").default);
    for t in ["Ollama 的模型", "Claude Code 的数据", "MySQL 9.4.0"] {
        assert!(env("known").items.iter().any(|i| i.title == t), "{t}");
    }
    assert!(env("known").items.iter().all(|i| i.title != "MySQL"), "the symlink isn't a second install");
    let mut unknown: Vec<&str> = env("unknown").items.iter().map(|i| i.title.as_str()).collect();
    unknown.sort();
    assert_eq!(unknown, [".mystery-tool", "someapp", "whisper"]);
    // files in dot folders aren't 大文件
    let ov = overview(&j, &Settings::default(), &[]).unwrap();
    assert!(group(&ov, rules::BIG).iter().all(|i| !i.paths[0].contains("/.")));
}

#[test]
fn cleaning_trashing_and_emptying() {
    let (_d, j) = sandbox("clean");
    let ov = scanned(&j);
    let s = Settings::default();
    let ids = |titles: &[&str]| -> Vec<i64> { titles.iter().map(|t| ov.categories.iter().flat_map(|g| &g.items).chain(ov.groups.iter().flat_map(|g| &g.items)).find(|i| i.title == *t).unwrap().id).collect() };
    let out = clean(&j, &ids(&["old-blog · node_modules", "npm 的下载缓存", "资源库里的日志", "pomelo · src-tauri/target"]), &s, SITE).unwrap();
    assert_eq!(out.done, 3);
    assert_eq!(out.failed, [("pomelo · src-tauri/target".to_string(), "node 在跑".to_string())]);
    assert!(!j.real_of("/Users/me/Documents/GitHub/old-blog/node_modules").exists());
    assert!(j.real_of("/Users/me/Documents/GitHub/old-blog/package.json").exists());
    assert!(!j.real_of("/Users/me/.npm/_cacache").exists());
    let logs = j.real_of("/Users/me/Library/Logs");
    assert!(logs.exists() && std::fs::read_dir(&logs).unwrap().next().is_none());
    // conda clean (the fake empties pkgs; the env keeps its linked file)
    let conda = ov.categories.iter().find(|g| g.id == rules::CONDA).unwrap().items[0].id;
    assert_eq!(clean(&j, &[conda], &s, SITE).unwrap().done, 1);
    assert!(j.real_of("/opt/homebrew/Caskroom/miniconda/base/envs/iss_ml/lib/libnumpy.dylib").exists());
    assert!(std::fs::read_dir(j.real_of("/opt/homebrew/Caskroom/miniconda/base/pkgs")).unwrap().next().is_none());
    let after = overview(&j, &s, &[]).unwrap();
    assert!(after.categories.iter().flat_map(|g| &g.items).all(|i| i.title != "old-blog · node_modules"));
    assert!(after.cleanable < ov.cleanable);
    // listed → the (fake) Trash; a protected one only when confirmed twice
    let model = ids(&["qwen3.5-9b-q4.gguf"]);
    let out = trash(&j, &[ids(&["照片备份.zip"]), model.clone()].concat(), false).unwrap();
    assert_eq!((out.done, out.failed.len()), (1, 1));
    assert!(j.real_of("/Users/me/.Trash/照片备份.zip").exists() && !j.real_of("/Users/me/Downloads/照片备份.zip").exists());
    assert_eq!(trash(&j, &model, true).unwrap().done, 1);
    assert!(j.real_of("/Users/me/.Trash/qwen3.5-9b-q4.gguf").exists());
    // a one-click id isn't trashed, a listed one isn't cleaned
    assert_eq!(trash(&j, &ids(&["Codex 的运行时"]), true).unwrap().done, 0);
    assert_eq!(clean(&j, &ids(&["电影.mp4"]), &s, SITE).unwrap().done, 0);
    let summary = overview(&j, &s, &[]).unwrap().summary.unwrap();
    assert_eq!(summary["trash"]["count"].as_u64(), Some(4));
    let out = empty_trash(&j).unwrap();
    assert_eq!(out.done, 4);
    assert!(std::fs::read_dir(j.real_of("/Users/me/.Trash")).unwrap().next().is_none());
    assert_eq!(overview(&j, &s, &[]).unwrap().summary.unwrap()["trash"]["size"].as_u64(), Some(0));
}

#[test]
fn settings_change_the_list() {
    let (_d, j) = sandbox("settings");
    let ov = scanned(&j);
    let s = Settings { off: vec![rules::PKG_CACHE.into()], stale_days: 500, ..Default::default() };
    let ov2 = overview(&j, &s, &[]).unwrap();
    assert!(ov2.categories.iter().all(|g| g.id != rules::PKG_CACHE));
    assert!(!one_click(&ov2, "old-blog · node_modules").checked);
    assert!(ov2.cleanable < ov.cleanable);
    let npm = one_click(&ov, "npm 的下载缓存").id;
    assert_eq!(clean(&j, &[npm], &s, SITE).unwrap().failed[0].1, "这一类没放进一键清理");
    // the 模块坞's total follows the same settings
    assert_eq!(j.cleanable(&s, &[]), ov2.cleanable);
}

#[test]
fn protected_places_stay_out_of_one_click() {
    let (_d, j) = sandbox("protected");
    for p in ["/Users/me/Documents/LLM/tool/package.json", "/Users/me/Documents/Seeker/web/package.json"] {
        let real = j.real_of(p);
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "{}").unwrap();
        let nm = real.parent().unwrap().join("node_modules/x.js");
        std::fs::create_dir_all(nm.parent().unwrap()).unwrap();
        std::fs::File::create(&nm).unwrap().set_len(30 * MB).unwrap();
    }
    let ov = scanned(&j);
    assert!(ov.categories.iter().flat_map(|g| &g.items).all(|i| !i.title.starts_with("tool") && !i.title.starts_with("web")));
    // a forged one in janitor.db is refused when cleaning
    let p = "/Users/me/Documents/LLM/tool/node_modules";
    j.with_db(|c| c.execute("INSERT INTO items (id, listed, kind, title, paths, size, newest, extra) VALUES (999, 0, 'node_modules', 'forged', ?1, 1, 0, '{}')", [serde_json::json!([p]).to_string()]).map_err(crate::modules::common::err)).unwrap();
    let out = clean(&j, &[999], &Settings::default(), SITE).unwrap();
    assert_eq!(out.failed, [("forged".to_string(), "受保护（本地模型）".to_string())]);
    assert!(j.real_of(p).exists());
}

#[test]
fn library_names() {
    assert_eq!(scan::bundle_like("com.foo.bar.plist").as_deref(), Some("com.foo.bar"));
    assert_eq!(scan::bundle_like("group.com.foo.bar").as_deref(), Some("com.foo.bar"));
    assert_eq!(scan::bundle_like("UBF8T346G9.com.microsoft.office").as_deref(), Some("com.microsoft.office"));
    assert_eq!(scan::bundle_like("com.apple.Safari"), None);
    assert_eq!(scan::bundle_like("Google"), None);
    assert_eq!(scan::bundle_like("Code"), None);
    let p = rules::Protected::new("/Users/me", SITE);
    assert_eq!(p.of("/Users/me/Documents/GitHub"), Some("个人网站"));
}

/// The real disk, read only (asked first): `cargo test --release real_janitor_scan -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_janitor_scan() {
    let home = std::env::var("HOME").unwrap();
    // never into Pomelo's real data or the Seeker folder (CLAUDE.md)
    let mut skip: std::collections::HashSet<String> = ["Library/Application Support/Pomelo OS", "Library/Application Support/Pomelo OS Dev", "Documents/Seeker"].iter().map(|p| format!("{home}/{p}")).collect();
    // as the app does without Full Disk Access (no prompts for other apps' data)
    if !Sys::Real.full_disk_access() {
        skip.extend(scan::TCC.iter().map(|p| format!("{home}/{p}")));
    }
    let cfg = Config { root: "/".into(), home: home.clone(), site: format!("{home}/Documents/GitHub/personal-website"), allocated: true, skip, now: now(), threads: 8, rules: vec![] };
    let sys = Sys::Real;
    let t = std::time::Instant::now();
    let out = scan::scan(&cfg, &sys, &Progress::default()).unwrap();
    println!("took {:?}, {} nodes, {} items", t.elapsed(), out.nodes.len(), out.items.len());
    println!("{}", serde_json::to_string_pretty(&out.summary).unwrap());
    let mut by: std::collections::BTreeMap<String, (usize, u64)> = Default::default();
    for i in &out.items {
        let e = by.entry(format!("{}{}", if i.listed { "listed " } else { "" }, i.kind)).or_default();
        e.0 += 1;
        e.1 += i.size;
    }
    for (k, (n, size)) in by {
        println!("{k}: {n} 项, {:.1} GB", size as f64 / 1e9);
    }
    // POMELO_JANITOR_SHOW=1: titles of the dev / app kinds (never personal files: big, duplicates, installers)
    if std::env::var("POMELO_JANITOR_SHOW").is_ok() {
        for i in out.items.iter().filter(|i| ![rules::BIG, rules::DUPLICATES, rules::INSTALLERS].contains(&i.kind.as_str())) {
            println!("  [{}] {} · {:.2} GB · {}{}", i.kind, i.title, i.size as f64 / 1e9, i.paths.len(), i.extra.get("inUse").and_then(|v| v.as_str()).map(|u| format!(" · 在用 {u}")).unwrap_or_default());
        }
        for e in &out.envs {
            println!("ENV {} · {:.1} GB{}", e.name, e.size as f64 / 1e9, e.note.as_ref().map(|n| format!(" · {n}")).unwrap_or_default());
            for i in &e.items {
                println!("    {}{} · {} · {} · {:.2} GB{}{}{}", if i.part { "  ↳ " } else { "" }, i.title, i.version.as_deref().unwrap_or("-"), i.by.as_deref().unwrap_or("-"), i.size as f64 / 1e9, if i.default { " · 默认" } else { "" }, i.packages.map(|p| format!(" · {p} 个")).unwrap_or_default(), i.tag.as_ref().map(|t| format!(" · {t}")).unwrap_or_default());
            }
        }
        let top: Vec<_> = out.nodes.iter().filter(|n| n.parent == Some(1)).map(|n| format!("{} {:.1} GB", n.name, n.size as f64 / 1e9)).collect();
        println!("first ring: {}", top.join("、"));
    }
}

/// The browser mock's data (src/dev/janitorMock.json) from the fake Mac:
/// `POMELO_JANITOR_MOCK=<file> cargo test janitor_mock_json -- --ignored`.
#[test]
#[ignore]
fn janitor_mock_json() {
    let (_d, j) = sandbox("mock");
    let ov = scanned(&j);
    let nodes: Vec<serde_json::Value> = j
        .with_db(|c| {
            let mut s = c.prepare("SELECT id, parent, name, path, kind, size, files, newest, denied FROM nodes ORDER BY id").map_err(crate::modules::common::err)?;
            let rows = s
                .query_map([], |r| Ok(serde_json::json!({ "id": r.get::<_, i64>(0)?, "parent": r.get::<_, Option<i64>>(1)?, "name": r.get::<_, String>(2)?, "path": r.get::<_, String>(3)?, "kind": r.get::<_, String>(4)?, "size": r.get::<_, i64>(5)?, "files": r.get::<_, i64>(6)?, "newest": r.get::<_, i64>(7)?, "denied": r.get::<_, i64>(8)? })))
                .map_err(crate::modules::common::err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(crate::modules::common::err)?;
            Ok(rows)
        })
        .unwrap();
    let out = serde_json::json!({ "overview": ov, "nodes": nodes, "envs": j.envs() });
    std::fs::write(std::env::var("POMELO_JANITOR_MOCK").expect("POMELO_JANITOR_MOCK"), serde_json::to_string(&out).unwrap()).unwrap();
}

// ---- the Agent (阶段 8) ----

mod agent_tests {
    use super::super::agent::{self, Card};
    use serde_json::json;
    use super::*;

    fn run(c: &rusqlite::Connection) -> i64 {
        c.execute_batch("INSERT INTO sessions (title) VALUES ('t'); INSERT INTO runs (session_id, model, model_name, state) VALUES (last_insert_rowid(), 'local:x', 'X', 'completed');").unwrap();
        c.last_insert_rowid()
    }

    fn card(j: &Janitor, c: &rusqlite::Connection, run: i64, tool: &str, args: serde_json::Value) -> Card {
        let card = agent::build_card(j, SITE, tool, &args.to_string()).unwrap();
        let id = agent::save_card(c, run, &card).unwrap();
        agent::card(c, id).unwrap()
    }

    #[test]
    fn paths_patterns_and_commands() {
        let home = "/Users/me";
        let p = rules::Protected::new(home, SITE);
        assert_eq!(agent::virt_of(home, "~/.cache/x/").unwrap(), "/Users/me/.cache/x");
        assert_eq!(agent::virt_of(home, "$HOME/Library").unwrap(), "/Users/me/Library");
        assert!(agent::virt_of(home, "relative/x").is_err() && agent::virt_of(home, "/").is_err());
        assert!(agent::glob("com.foo.*", "com.foo.bar") && agent::glob("*cache*", "my-cache-1") && !agent::glob("com.foo.*", "com.bar.x"));
        assert!(agent::matches("/Users/me/Library/Caches/com.foo.*", "/Users/me/Library/Caches/com.foo.helper"));
        assert!(!agent::matches("/Users/me/Library/Caches/com.foo.*", "/Users/me/Library/Caches/com.foo.helper/inner"));
        // a rule: under home, two levels at least, outside personal folders and protected places
        assert_eq!(agent::check_pattern(home, &p, "~/.cache/whisper").unwrap(), "/Users/me/.cache/whisper");
        for bad in ["~/.cache", "~/Downloads/*.dmg", "~/Documents/LLM/models", "/opt/homebrew/x/y", "~/*/*"] {
            assert!(agent::check_pattern(home, &p, bad).is_err(), "{bad}");
        }
        // commands: sudo, and the protected places their paths touch
        assert!(agent::needs_admin("sudo rm -rf /Library/Java/x") && agent::needs_admin("cd /tmp && sudo du") && !agent::needs_admin("brew cleanup --prune=all"));
        assert_eq!(agent::command_protected(home, &p, "rm -rf ~/Documents/LLM/models/a.gguf").as_deref(), Some("本地模型"));
        assert_eq!(agent::command_protected(home, &p, "security delete-generic-password -s x").as_deref(), Some("钥匙串"));
        assert_eq!(agent::command_protected(home, &p, "rm -rf \"$HOME/Library/Application Support/Pomelo OS\"").as_deref(), None);
        assert_eq!(agent::command_protected(home, &p, "rm -rf ~/Library/Application\\ Support/Pomelo\\ OS"), None, "a path with escaped spaces is cut at the space");
        assert_eq!(agent::command_protected(home, &p, "conda clean --all --yes"), None);
        assert_eq!(agent::command_protected(home, &p, "rm -rf ~/Documents/GitHub/old-blog/node_modules"), None);
    }

    #[test]
    fn looking_and_the_three_cards() {
        let (_d, j) = sandbox("agent");
        scanned(&j);
        let c = crate::youzi::store::tests::memory_db();
        let r = run(&c);
        // looking: a folder (names, sizes, times) and find
        let a = agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"folder","path":"~/Downloads"}"#, 4000);
        assert_eq!(a.state, "ok");
        assert!(a.text.contains("电影.mp4") && a.text.contains("2.5 GB"), "{}", a.text);
        let a = agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"find","path":"~","ext":"dmg"}"#, 4000);
        assert!(a.text.contains("找到 3 个文件") && a.text.contains("旧版微信.dmg") && a.text.contains("com.docker.install"), "{}", a.text);
        // the overview says there's a list; the list itself on request (Mail's places among the listed)
        let a = agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"overview"}"#, 4000).text;
        assert!(a.contains("清理页有一份清单") && !a.contains("包管理器的下载缓存"), "{a}");
        let a = agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"cleanup"}"#, 8000).text;
        assert!(a.contains("## 一键清理") && a.contains("邮件（Mail）的数据") && a.contains("Mail 的邮箱和索引"), "{a}");
        let ov = overview(&j, &Settings::default(), &[]).unwrap();
        let mail = ov.groups.iter().find(|g| g.id == rules::MAIL).expect("Mail's group");
        assert_eq!(mail.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Mail 的邮箱和索引", "Mail 打开过的附件"]);
        assert!(mail.items[0].note.as_deref().unwrap_or("").contains("互联网账户"));
        // my names for the categories
        let first = ov.categories[0].id.clone();
        let mine = Settings { names: [(first.clone(), "我起的名字".to_string())].into(), ..Settings::default() };
        assert_eq!(overview(&j, &mine, &[]).unwrap().categories[0].name, "我起的名字");
        // read-only commands: run right away (the fake Mac doesn't), never a card; the rest refused
        let a = agent::run_readonly_with(&j, r#"{"command":"du -sh ~/Library/Caches"}"#, 4000);
        assert!(a.state == "ok" && a.text.contains("假 Mac"), "{}", a.text);
        assert_eq!(agent::run_readonly_with(&j, r#"{"command":"rm -rf ~/x"}"#, 4000).state, "failed");
        assert!(agent::build_card(&j, SITE, agent::COMMAND, &json!({ "command": "brew list --versions", "what": "看", "deletes": "", "recoverable": "" }).to_string()).unwrap_err().contains("pomelo_janitor_run"));
        assert!(agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"envs"}"#, 4000).text.contains("JDK 25"));
        assert_eq!(agent::look_with(&j, &Settings::default(), &[], SITE, r#"{"view":"folder","path":"~/nope"}"#, 4000).state, "failed");

        // trash: sizes, the protected one marked; picking without the second confirm leaves it
        let t = card(&j, &c, r, agent::TRASH, json!({ "items": [{ "path": "~/.mystery-tool", "why": "认不出来" }, { "path": "~/Documents/LLM/models/embedding.gguf", "why": "试试" }] }));
        assert_eq!((t.items[0].size, t.items[1].protected.as_deref(), t.protected.is_some()), (310 * MB, Some("本地模型"), true));
        let (state, result) = agent::trash_card(&j, &t, &[0, 1], false).unwrap();
        assert_eq!(state, "done");
        assert_eq!((result["items"][0]["done"].as_bool(), result["items"][1]["error"].as_str()), (Some(true), Some("受保护，要再确认一次")));
        assert!(j.real_of("/Users/me/.Trash/.mystery-tool").exists() && j.real_of("/Users/me/Documents/LLM/models/embedding.gguf").exists());
        agent::set_state(&c, t.id, state, Some(&result)).unwrap();
        assert!(agent::trash_card(&j, &agent::card(&c, t.id).unwrap(), &[1], true).is_err(), "a card is done once");
        // what it came to goes back with the reply (pending cards don't)
        let h = agent::in_history(&agent::cards_of(&c, r).unwrap());
        assert!(h.contains("卡片的结果") && h.contains("移了 1 项") && h.contains("受保护，要再确认一次"), "{h}");
        // a path that isn't there, the whole home: no card
        assert!(agent::build_card(&j, SITE, agent::TRASH, &json!({ "items": [{ "path": "~/nope", "why": "x" }] }).to_string()).is_err());
        assert!(agent::build_card(&j, SITE, agent::TRASH, &json!({ "items": [{ "path": "~", "why": "x" }] }).to_string()).is_err());

        // commands: sudo only shown; protected asks twice; one line each
        let sudo = card(&j, &c, r, agent::COMMAND, json!({ "command": "sudo rm -rf /Library/Java/JavaVirtualMachines/jdk-1.8.jdk", "what": "删 JDK 8", "deletes": "JDK 8", "recoverable": "重装" }));
        assert!(sudo.sudo && agent::may_run(&sudo, true).is_err());
        let risky = card(&j, &c, r, agent::COMMAND, json!({ "command": "rm -rf ~/Documents/LLM/models", "what": "x", "deletes": "x", "recoverable": "不能" }));
        assert!(risky.protected.is_some() && agent::may_run(&risky, false).is_err() && agent::may_run(&risky, true).is_ok());
        let fine = card(&j, &c, r, agent::COMMAND, json!({ "command": "conda clean --all --yes", "what": "清 Conda", "deletes": "没在用的包", "recoverable": "重新下载", "frees": "约 3 GB" }));
        assert!(!fine.sudo && fine.protected.is_none() && agent::may_run(&fine, false).is_ok());
        assert!(agent::build_card(&j, SITE, agent::COMMAND, &json!({ "command": "ls\nrm -rf x", "what": "", "deletes": "", "recoverable": "" }).to_string()).is_err());

        // a rule: its matches now; added (checked again), then a scan makes it a 一键清理 group
        let rule = card(&j, &c, r, agent::RULE, json!({ "name": "Whisper 的模型", "path": "~/.cache/whisper", "recognize": "Whisper 下的模型", "rebuild": "用到时重新下载", "checked": true }));
        assert_eq!((rule.matches, rule.match_size), (1, 1_500 * MB));
        assert!(agent::build_card(&j, SITE, agent::RULE, &json!({ "name": "x", "path": "~/Downloads/*", "recognize": "", "rebuild": "" }).to_string()).is_err());
        let p = rules::Protected::new("/Users/me", SITE);
        let id = agent::rule_in(&c, "/Users/me", &p, rule.id).unwrap();
        assert_eq!(agent::card(&c, rule.id).unwrap().state, "done");
        assert!(agent::rule_in(&c, "/Users/me", &p, rule.id).is_err());
        let rs = agent::rules(&c).unwrap();
        assert_eq!((rs.len(), rs[0].id, rs[0].pattern.as_str()), (1, id, "/Users/me/.cache/whisper"));
        let out = scan::scan(&j.config(SITE, rs.clone()), &j.sys(), &Progress::default()).unwrap();
        let envs = serde_json::to_value(&out.envs).unwrap();
        j.with_db(|db| store::replace(db, &out.summary, &out.nodes, &out.items, &envs)).unwrap();
        let ov = overview(&j, &Settings::default(), &rs).unwrap();
        let g = ov.categories.iter().find(|g| g.id == format!("rule:{id}")).expect("the rule's group");
        assert_eq!((g.name.as_str(), g.items[0].title.as_str(), g.items[0].checked, g.size), ("Whisper 的模型", "Whisper 的模型", true, 1_500 * MB));
        // and it cleans like the rest; a deleted rule's items stop counting
        assert_eq!(clean(&j, &[g.items[0].id], &Settings::default(), SITE).unwrap().done, 1);
        assert!(!j.real_of("/Users/me/.cache/whisper").exists());
        agent::rename_rule(&c, id, " Whisper 模型 ").unwrap();
        assert_eq!(agent::rules(&c).unwrap()[0].name, "Whisper 模型");
        assert!(agent::rename_rule(&c, id, "  ").is_err() && agent::rename_rule(&c, 999, "x").is_err());
        agent::delete_rule(&c, id).unwrap();
        assert!(agent::rules(&c).unwrap().is_empty());

        // the cards come with their reply; the model hears what it proposed
        assert_eq!(agent::cards_of(&c, r).unwrap().len(), 5);
        assert!(agent::proposed(&sudo).text.contains("要管理员密码"));
    }

    #[test]
    fn its_conversation_is_hidden_and_new_drops_the_old() {
        let c = crate::youzi::store::tests::memory_db();
        let level = crate::youzi::cloud::Level::of(true, None);
        let s = agent::open_chat(&c, &level).unwrap();
        assert_eq!(agent::open_chat(&c, &level).unwrap(), s);
        assert!(agent::is_chat(&c, s).unwrap());
        let listed: i64 = c.query_row(concat!("SELECT count(*) FROM sessions WHERE id NOT IN ", crate::youzi::library::unlisted!()), [], |r| r.get(0)).unwrap();
        assert_eq!(listed, 0, "not in 柚子's list, search, ⌘K or the 模块坞's count");
        c.execute("INSERT INTO messages (session_id, role, content, state) VALUES (?1, 'user', '旧的', 'complete')", [s]).unwrap();
        let n = agent::new_chat(&c, &level).unwrap();
        assert_ne!(n, s, "a new id: the page switches on it");
        assert!(agent::is_chat(&c, n).unwrap());
        let (sessions, messages): (i64, i64) = c.query_row("SELECT (SELECT count(*) FROM sessions), (SELECT count(*) FROM messages)", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((sessions, messages), (1, 0), "the old conversation is gone");
    }

    #[test]
    fn my_skill_replaces_the_built_in_one() {
        let c = crate::youzi::store::tests::memory_db();
        let (built_in, mine) = agent::skill(&c);
        assert!(!mine && built_in.contains("pomelo_janitor_run"));
        agent::set_skill(&c, Some("  只看不删。 ")).unwrap();
        assert_eq!(agent::skill(&c), ("只看不删。".to_string(), true));
        agent::set_skill(&c, Some(&built_in)).unwrap();
        assert!(!agent::skill(&c).1, "the built-in text back is the built-in one");
        agent::set_skill(&c, Some("x")).unwrap();
        agent::set_skill(&c, None).unwrap();
        assert!(!agent::skill(&c).1);
    }

    #[test]
    fn the_mock_model_proposes_by_what_is_asked() {
        let names = |q: &str| agent::mock_calls(q).into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names("看看能清什么"), [agent::LOOK, agent::LOOK]);
        assert_eq!(names("帮我删掉没用的，再给条 conda 命令，加个规则"), [agent::LOOK, agent::TRASH, agent::COMMAND, agent::RULE]);
        assert!(agent::mock_calls("删掉 JDK 8 sudo").iter().any(|c| c.arguments.contains("sudo")));
        assert_eq!(names("用只读的看看清单"), [agent::LOOK, agent::LOOK, agent::RUN, agent::LOOK]);
    }
}
