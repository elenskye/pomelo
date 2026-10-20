//! 待发布的改动: `git status` (files) + commits on main not yet on origin/main, each file named by what it
//! is on the site (作品《…》中文, Now 2026-07 English, 图片 …; anything else is code, shown by path).
//! Size rules are the site's own (scripts/audit-assets.mjs): under public/, images ≤ 1.5 MiB, other
//! files ≤ 5 MiB. The token (a hash of every change's path, action and bytes, HEAD and the unpushed
//! commits) is what 发布 checks the confirmed list against.

use super::tools::{self, GIT};
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Duration;

pub const MAX_IMAGE: u64 = 1536 * 1024;
pub const MAX_FILE: u64 = 5 * 1024 * 1024;
const IMAGE_EXT: [&str; 7] = ["avif", "gif", "jpeg", "jpg", "png", "svg", "webp"];
const DIFF_LINES: usize = 3000;
const INLINE_IMAGE: u64 = 8 * 1024 * 1024;
const T: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    /// The old path of a rename.
    pub from: Option<String>,
    /// add | modify | delete | rename
    pub action: &'static str,
    /// content | code
    pub kind: &'static str,
    pub label: String,
    pub size: Option<u64>,
    /// Why it may not go online (too big).
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    /// Unix seconds.
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub files: Vec<Change>,
    /// On main, not on origin/main yet (oldest last).
    pub ahead: Vec<Commit>,
    pub token: String,
}

impl Changes {
    pub fn blocked(&self) -> bool {
        self.files.iter().any(|c| c.blocked.is_some())
    }
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.ahead.is_empty()
    }
}

/// (XY, path, renamed-from) from `git status --porcelain=v1 -z`.
pub fn parse_status(z: &str) -> Vec<(String, String, Option<String>)> {
    let mut out = vec![];
    let mut it = z.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = it.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = (entry[..2].to_string(), entry[3..].to_string());
        let from = if xy.contains('R') || xy.contains('C') { it.next().map(str::to_string) } else { None };
        out.push((xy, path, from));
    }
    out
}

fn action(xy: &str) -> &'static str {
    if xy == "??" || xy.contains('A') {
        "add"
    } else if xy.contains('D') {
        "delete"
    } else if xy.contains('R') {
        "rename"
    } else {
        "modify"
    }
}

fn lang(s: &str) -> &'static str {
    if s == "en" {
        "English"
    } else {
        "中文"
    }
}

/// `title:` from a Markdown file's YAML front matter (plain, quoted, or a folded / literal block).
pub fn front_title(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut lines = lines.take_while(|l| l.trim_end() != "---").peekable();
    while let Some(l) = lines.next() {
        let Some(v) = l.strip_prefix("title:") else { continue };
        let v = v.trim();
        let t = if v.starts_with('>') || v.starts_with('|') {
            let mut parts = vec![];
            while let Some(next) = lines.peek().filter(|n| n.starts_with(' ') || n.is_empty()) {
                parts.push(next.trim().to_string());
                lines.next();
            }
            parts.join(if v.starts_with('>') { " " } else { "\n" }).trim().to_string()
        } else if let Some(q) = v.strip_prefix('\'').and_then(|q| q.strip_suffix('\'')) {
            q.replace("''", "'")
        } else if let Some(q) = v.strip_prefix('"').and_then(|q| q.strip_suffix('"')) {
            q.replace("\\\"", "\"")
        } else {
            v.to_string()
        };
        return (!t.is_empty()).then_some(t);
    }
    None
}

/// What a path is on the site: (content | code, label). `title` reads a work's title (now, else as it was).
pub fn label(path: &str, title: impl Fn(&str) -> Option<String>) -> (&'static str, String) {
    let parts: Vec<&str> = path.split('/').collect();
    let stem_lang = |f: &str| f.strip_suffix(".md").and_then(|s| s.rsplit_once('.')).map(|(b, l)| (b.to_string(), l.to_string()));
    match parts.as_slice() {
        ["src", "content", "wander", f] => match stem_lang(f) {
            Some((base, l)) => ("content", format!("作品《{}》{}", title(path).unwrap_or(base), lang(&l))),
            None => ("content", format!("作品 {f}")),
        },
        ["src", "content", "now", f] => match stem_lang(f) {
            Some((m, l)) => ("content", format!("Now {m} {}", lang(&l))),
            None => ("content", format!("Now {f}")),
        },
        ["src", "content", "about", f] => ("content", format!("关于我 {}", lang(f.trim_end_matches(".md")))),
        ["src", "content", "career", f] => ("content", format!("职业页 {}", lang(f.trim_end_matches(".md")))),
        ["src", "data", "now-status.json"] => ("content", "当下状态".into()),
        ["src", "data", "wander-taxonomy.json"] => ("content", "话题与类别".into()),
        ["src", "data", "social-links.json"] => ("content", "外部链接".into()),
        ["src", "data", "about-images.json"] => ("content", "关于我的图片".into()),
        ["src", "data", "assets", "images", f] => ("content", format!("资源库 · 图片 {}", f.trim_end_matches(".json"))),
        ["src", "data", "assets", "documents", f] => ("content", format!("资源库 · 文档 {}", f.trim_end_matches(".json"))),
        ["public", "assets", "images", f] => ("content", format!("图片 {f}")),
        ["public", "assets", "documents", f] => ("content", format!("文档 {f}")),
        ["public", rest @ ..] if is_image(path) => ("content", format!("图片 {}", rest.join("/"))),
        ["public", rest @ ..] => ("content", format!("文件 {}", rest.join("/"))),
        _ => ("code", path.to_string()),
    }
}

pub fn is_image(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| IMAGE_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn mib(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
}

/// The site's size rule for a file going online.
pub fn too_big(path: &str, size: u64) -> Option<String> {
    if !path.starts_with("public/") {
        return None;
    }
    let (limit, what) = if is_image(path) { (MAX_IMAGE, "图片") } else { (MAX_FILE, "文件") };
    (size > limit).then(|| format!("{what}有 {}，网站规定不超过 {}", mib(size), mib(limit)))
}

fn head_text(folder: &Path, path: &str) -> Option<String> {
    tools::git(folder, &["show", &format!("HEAD:{path}")], T).ok()
}

/// Every change with its label and size check, plus unpushed commits.
pub fn list(folder: &Path) -> Result<Changes, String> {
    let z = tools::git(folder, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], T).map_err(|e| format!("读不了网站的改动：{e}"))?;
    let title = |p: &str| std::fs::read_to_string(folder.join(p)).ok().or_else(|| head_text(folder, p)).as_deref().and_then(front_title);
    let mut files: Vec<Change> = parse_status(&z)
        .into_iter()
        .map(|(xy, path, from)| {
            let act = action(&xy);
            let (kind, label) = label(&path, title);
            let size = (act != "delete").then(|| std::fs::metadata(folder.join(&path)).map(|m| m.len()).ok()).flatten();
            let blocked = size.and_then(|s| too_big(&path, s));
            Change { path, from, action: act, kind, label, size, blocked }
        })
        .collect();
    // content first (as the site names it), code after; each by path
    files.sort_by(|a, b| (a.kind != "content", &a.path).cmp(&(b.kind != "content", &b.path)));
    let ahead = ahead(folder)?;
    let token = token(folder, &files, &ahead);
    Ok(Changes { files, ahead, token })
}

pub fn ahead(folder: &Path) -> Result<Vec<Commit>, String> {
    let log = tools::git(folder, &["log", "origin/main..HEAD", "--format=%H%x1f%s%x1f%ct"], T).map_err(|e| format!("读不了还没推送的提交：{e}"))?;
    Ok(parse_log(&log))
}

pub fn parse_log(log: &str) -> Vec<Commit> {
    log.lines()
        .filter_map(|l| {
            let mut f = l.split('\u{1f}');
            Some(Commit { sha: f.next()?.to_string(), subject: f.next()?.to_string(), time: f.next()?.trim().parse().ok()? })
        })
        .collect()
}

fn token(folder: &Path, files: &[Change], ahead: &[Commit]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tools::git(folder, &["rev-parse", "HEAD"], T).unwrap_or_default().hash(&mut h);
    for c in ahead {
        c.sha.hash(&mut h);
    }
    for c in files {
        (&c.path, &c.from, c.action).hash(&mut h);
        if c.action != "delete" {
            std::fs::read(folder.join(&c.path)).unwrap_or_default().hash(&mut h);
        }
    }
    format!("{:016x}", h.finish())
}

// ---------- 差异 ----------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// ctx | add | del | hunk
    pub t: &'static str,
    pub s: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Diff {
    Text { lines: Vec<Line>, truncated: bool },
    /// data: URLs (None = not there before / not there now).
    #[serde(rename_all = "camelCase")]
    Image { before: Option<String>, after: Option<String> },
    Binary { before: Option<u64>, after: Option<u64> },
}

/// Lines of a unified diff after its header (None when git calls it binary).
pub fn parse_diff(text: &str) -> Option<(Vec<Line>, bool)> {
    let mut lines = vec![];
    let mut started = false;
    for l in text.lines() {
        if !started {
            if l.starts_with("Binary files ") || l.starts_with("GIT binary patch") {
                return None;
            }
            if !l.starts_with("@@") {
                continue;
            }
            started = true;
        }
        if lines.len() >= DIFF_LINES {
            return Some((lines, true));
        }
        let (t, s) = match l.as_bytes().first() {
            Some(b'@') => ("hunk", l.splitn(3, "@@").nth(2).unwrap_or("").trim().to_string()),
            Some(b'+') => ("add", l[1..].to_string()),
            Some(b'-') => ("del", l[1..].to_string()),
            Some(b'\\') => continue,
            _ => ("ctx", l.get(1..).unwrap_or("").to_string()),
        };
        lines.push(Line { t, s });
    }
    Some((lines, false))
}

fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("svg") => "image/svg+xml",
        _ => "image/jpeg",
    }
}

fn data_url(path: &str, bytes: Vec<u8>) -> Option<String> {
    (!bytes.is_empty() && bytes.len() as u64 <= INLINE_IMAGE).then(|| format!("data:{};base64,{}", mime(path), crate::picky::office::base64(&bytes)))
}

fn head_bytes(folder: &Path, path: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new(GIT).current_dir(folder).args(["show", &format!("HEAD:{path}")]).env("GIT_OPTIONAL_LOCKS", "0").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// One change's diff: text line by line, images before / after, other binaries by size.
pub fn diff(folder: &Path, path: &str) -> Result<Diff, String> {
    let changes = list(folder)?;
    let c = changes.files.iter().find(|c| c.path == path).ok_or("这个文件已经没有改动了")?;
    let old = c.from.as_deref().unwrap_or(&c.path);
    let in_head = c.action != "add";
    if is_image(path) {
        let before = in_head.then(|| head_bytes(folder, old)).flatten();
        let after = (c.action != "delete").then(|| std::fs::read(folder.join(path)).ok()).flatten();
        if before.as_ref().map_or(0, |b| b.len() as u64) <= INLINE_IMAGE && after.as_ref().map_or(0, |b| b.len() as u64) <= INLINE_IMAGE {
            return Ok(Diff::Image { before: before.and_then(|b| data_url(old, b)), after: after.and_then(|b| data_url(path, b)) });
        }
        return Ok(Diff::Binary { before: before.map(|b| b.len() as u64), after: c.size });
    }
    let text = if in_head {
        let mut args = vec!["diff", "HEAD", "--no-color", "--no-ext-diff", "-M", "--"];
        if old != path {
            args.push(old);
        }
        args.push(path);
        tools::git(folder, &args, T)?
    } else {
        // untracked: against nothing (exit 1 = there are differences)
        let out = tools::run(Path::new(GIT), folder, &["diff", "--no-index", "--no-color", "--no-ext-diff", "--", "/dev/null", path], None, T)?;
        if out.code != Some(0) && out.code != Some(1) {
            return Err(out.tail());
        }
        out.stdout
    };
    match parse_diff(&text) {
        Some((lines, truncated)) => Ok(Diff::Text { lines, truncated }),
        None => Ok(Diff::Binary { before: in_head.then(|| head_bytes(folder, old).map(|b| b.len() as u64)).flatten(), after: c.size }),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::tests::temp_dir;

    pub fn git(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new(GIT).current_dir(dir).args(args).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    pub fn write(dir: &Path, path: &str, body: impl AsRef<[u8]>) {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// A tiny site repo on main with an origin (a bare repo next to it).
    pub fn site_repo(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let (site, remote) = (dir.join("website"), dir.join("remote.git"));
        std::fs::create_dir_all(&site).unwrap();
        git(&site, &["init", "-q", "-b", "main"]);
        write(&site, "src/content/wander/big-fish.zh.md", "---\ntitle: 百言集 | 大鱼\nlanguage: zh\n---\n正文\n");
        write(&site, "src/components/Wall.astro", "<div/>\n");
        write(&site, "public/images/wander/a.jpg", [0xffu8, 0xd8, 0xff, 1, 2, 3]);
        write(&site, ".gitignore", "dist/\n");
        git(&site, &["add", "-A"]);
        git(&site, &["commit", "-q", "-m", "initial commit"]);
        git(dir, &["clone", "-q", "--bare", "website", "remote.git"]);
        git(&site, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&site, &["fetch", "-q", "origin"]);
        git(&site, &["branch", "-q", "--set-upstream-to=origin/main", "main"]);
        (site, remote)
    }

    #[test]
    fn titles_from_front_matter() {
        assert_eq!(front_title("---\ntitle: 百言集 | 大鱼\nlanguage: zh\n---\n").as_deref(), Some("百言集 | 大鱼"));
        assert_eq!(front_title("---\nlanguage: zh\ntitle: 'Excel&AI: It''s'\n---\n").as_deref(), Some("Excel&AI: It's"));
        assert_eq!(front_title("---\ntitle: \"A \\\"b\\\"\"\n---\n").as_deref(), Some("A \"b\""));
        assert_eq!(front_title("---\ntitle: >-\n  How I went\n  from 5.5\nlanguage: en\n---\n").as_deref(), Some("How I went from 5.5"));
        assert_eq!(front_title("---\ntitle: ''\n---\n"), None);
        assert_eq!(front_title("no front matter\ntitle: x"), None);
        assert_eq!(front_title("---\nlanguage: zh\n---\ntitle: in the body\n"), None);
    }

    #[test]
    fn labels_by_path() {
        let t = |_: &str| Some("大鱼".to_string());
        let none = |_: &str| None;
        assert_eq!(label("src/content/wander/big-fish.zh.md", t), ("content", "作品《大鱼》中文".into()));
        assert_eq!(label("src/content/wander/big-fish.en.md", none), ("content", "作品《big-fish》English".into()));
        assert_eq!(label("src/content/now/2026-07.en.md", none), ("content", "Now 2026-07 English".into()));
        assert_eq!(label("src/content/about/zh.md", none), ("content", "关于我 中文".into()));
        assert_eq!(label("src/content/career/en.md", none), ("content", "职业页 English".into()));
        assert_eq!(label("src/data/now-status.json", none), ("content", "当下状态".into()));
        assert_eq!(label("src/data/wander-taxonomy.json", none), ("content", "话题与类别".into()));
        assert_eq!(label("src/data/social-links.json", none), ("content", "外部链接".into()));
        assert_eq!(label("src/data/assets/images/wander-big-fish.json", none), ("content", "资源库 · 图片 wander-big-fish".into()));
        assert_eq!(label("public/assets/documents/cv.pdf", none), ("content", "文档 cv.pdf".into()));
        assert_eq!(label("public/assets/images/big-fish.jpg", none), ("content", "图片 big-fish.jpg".into()));
        assert_eq!(label("public/images/wander/big-fish/1.JPG", none), ("content", "图片 images/wander/big-fish/1.JPG".into()));
        assert_eq!(label("public/love-shuyu/audio/sky.m4a", none), ("content", "文件 love-shuyu/audio/sky.m4a".into()));
        assert_eq!(label("src/components/WanderWall.astro", none), ("code", "src/components/WanderWall.astro".into()));
        assert_eq!(label("package.json", none), ("code", "package.json".into()));
    }

    #[test]
    fn size_rules_are_the_sites() {
        assert_eq!(too_big("public/images/a.webp", MAX_IMAGE), None);
        assert!(too_big("public/images/a.webp", MAX_IMAGE + 1).unwrap().contains("不超过 1.5 MB"));
        assert_eq!(too_big("public/cv/zh/cv.pdf", MAX_IMAGE + 1), None);
        assert!(too_big("public/love-shuyu/audio/a.m4a", MAX_FILE + 1).unwrap().contains("不超过 5.0 MB"));
        assert_eq!(too_big("src/assets/huge.png", MAX_FILE * 3), None);
    }

    #[test]
    fn status_with_renames_spaces_and_untracked() {
        let s = "M  src/a.md\0R  new name.md\0old name.md\0?? public/images/x y.png\0 D gone.json\0";
        assert_eq!(
            parse_status(s),
            vec![
                ("M ".into(), "src/a.md".into(), None),
                ("R ".into(), "new name.md".into(), Some("old name.md".into())),
                ("??".into(), "public/images/x y.png".into(), None),
                (" D".into(), "gone.json".into(), None),
            ]
        );
        assert_eq!([action("??"), action(" M"), action("D "), action("R "), action("A ")], ["add", "modify", "delete", "rename", "add"]);
    }

    #[test]
    fn lists_changes_ahead_commits_and_diffs() {
        let dir = temp_dir("site-changes");
        let (site, _) = site_repo(&dir);
        let empty = list(&site).unwrap();
        assert!(empty.is_empty());
        write(&site, "src/content/wander/big-fish.zh.md", "---\ntitle: 百言集 | 大鱼\nlanguage: zh\n---\n正文改了\n");
        write(&site, "src/content/wander/new-one.en.md", "---\ntitle: New One\n---\nhi\n");
        write(&site, "public/images/wander/big.png", vec![0u8; MAX_IMAGE as usize + 10]);
        std::fs::remove_file(site.join("public/images/wander/a.jpg")).unwrap();
        write(&site, "src/components/Wall.astro", "<div class=\"x\"/>\n");
        let c = list(&site).unwrap();
        let rows: Vec<(&str, &str, &str)> = c.files.iter().map(|f| (f.action, f.kind, f.label.as_str())).collect();
        assert_eq!(
            rows,
            vec![
                ("delete", "content", "图片 images/wander/a.jpg"),
                ("add", "content", "图片 images/wander/big.png"),
                ("modify", "content", "作品《百言集 | 大鱼》中文"),
                ("add", "content", "作品《New One》English"),
                ("modify", "code", "src/components/Wall.astro"),
            ]
        );
        assert!(c.blocked() && c.files[1].blocked.as_ref().unwrap().contains("1.5 MB") && c.files[0].size.is_none());
        assert_ne!(c.token, empty.token);
        assert_eq!(list(&site).unwrap().token, c.token);
        // text diff of a tracked file, an untracked one, image before / after
        let Diff::Text { lines, truncated } = diff(&site, "src/content/wander/big-fish.zh.md").unwrap() else { panic!() };
        assert!(!truncated && lines.contains(&Line { t: "del", s: "正文".into() }) && lines.contains(&Line { t: "add", s: "正文改了".into() }) && lines[0].t == "hunk");
        let Diff::Text { lines, .. } = diff(&site, "src/content/wander/new-one.en.md").unwrap() else { panic!() };
        assert_eq!(lines.iter().filter(|l| l.t == "add").count(), 4);
        let Diff::Image { before, after } = diff(&site, "public/images/wander/a.jpg").unwrap() else { panic!() };
        assert!(before.unwrap().starts_with("data:image/jpeg;base64,") && after.is_none());
        assert!(diff(&site, "nope.md").is_err());
        // a commit not pushed yet
        git(&site, &["add", "-A"]);
        git(&site, &["commit", "-q", "-m", "改了大鱼"]);
        let c = list(&site).unwrap();
        assert!(c.files.is_empty() && c.ahead.len() == 1 && c.ahead[0].subject == "改了大鱼" && c.ahead[0].time > 0);
    }

    #[test]
    fn diff_lines() {
        let d = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@ heading\n a\n-b\n+c\n\\ No newline at end of file\n";
        let (lines, t) = parse_diff(d).unwrap();
        assert!(!t);
        assert_eq!(lines, vec![Line { t: "hunk", s: "heading".into() }, Line { t: "ctx", s: "a".into() }, Line { t: "del", s: "b".into() }, Line { t: "add", s: "c".into() }]);
        assert!(parse_diff("diff --git a/x b/x\nBinary files a/x and b/x differ\n").is_none());
    }
}
