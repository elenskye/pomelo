//! 发布 (one at a time, in a background thread; the page polls the job):
//! ① 检查: the site's own `npm run qa` (what Vercel runs before going online; ≤ 15 min). Failing stops
//!    here with the output's tail; nothing is committed.
//! ② 提交并推送: fetch again — behind origin/main, or the files no longer what was confirmed (token) →
//!    stop. Then `git add -A` (content and code alike: the user's choice), `git commit -F -`
//!    (`--cleanup=whitespace`: the text exactly as written, `#` lines kept), `git push origin main`.
//!    Never forced, history never rewritten. A failed push leaves the commit; the next 发布 pushes it.
//! ③ 上线: Vercel's commit status read with gh every 8 s (≤ 15 min). The sandbox fakes it.
//! 最近发布 = the last commits of origin/main, Vercel's results from one `gh api graphql`.

use super::changes::{self, Commit};
use super::tools::{self, Log};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QA_LIMIT: Duration = Duration::from_secs(15 * 60);
const DEPLOY_LIMIT: Duration = Duration::from_secs(15 * 60);
const DEPLOY_EVERY: Duration = Duration::from_secs(8);
const NET: Duration = Duration::from_secs(90);
pub const HISTORY: usize = 10;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    /// check | push | deploy
    pub step: &'static str,
    /// running | done | failed
    pub state: &'static str,
    /// When this step began (unix ms).
    pub since: u64,
    /// The commit text's first line.
    pub subject: String,
    pub sha: Option<String>,
    pub error: Option<String>,
    /// The check's last output lines (all of the tail when it failed, the last line while running).
    pub log: Vec<String>,
    /// Vercel's page for this deployment.
    pub deploy_url: Option<String>,
    /// Vercel never said (no gh, or no status in 15 minutes).
    pub unknown: bool,
}

/// What a run needs, fixed when 发布 is confirmed.
#[derive(Debug, Clone)]
pub struct Plan {
    pub folder: PathBuf,
    /// The sandbox: refuses anything but a local remote; the deployment is faked.
    pub sandbox: bool,
    /// The check: npm + ["run", "qa"] (tests use a shell line).
    pub check: (PathBuf, Vec<String>),
    pub node_bin: Option<PathBuf>,
    pub message: String,
    pub token: String,
    /// GitHub owner / name (for Vercel's status).
    pub repo: Option<(String, String)>,
    pub gh: Option<PathBuf>,
    /// The sandbox's pretend deployment time.
    pub fake_deploy: Duration,
}

#[derive(Default)]
pub struct Publisher {
    job: Mutex<Option<Job>>,
    log: Mutex<Log>,
    group: Mutex<Option<u32>>,
    next: AtomicU64,
}

impl Publisher {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Job>> {
        self.job.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn view(&self) -> Option<Job> {
        let mut j = self.lock().clone()?;
        if j.state == "running" && j.step == "check" {
            j.log = self.log.lock().unwrap_or_else(|e| e.into_inner()).tail(1);
        }
        Some(j)
    }

    pub fn running(&self) -> bool {
        self.lock().as_ref().is_some_and(|j| j.state == "running")
    }

    /// The check's last output lines.
    #[cfg(test)]
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).tail(n)
    }

    /// Forgets a finished job (its card's 关闭).
    pub fn dismiss(&self) {
        let mut g = self.lock();
        if g.as_ref().is_some_and(|j| j.state != "running") {
            *g = None;
        }
    }

    fn update(&self, id: u64, f: impl FnOnce(&mut Job)) {
        if let Some(j) = self.lock().as_mut().filter(|j| j.id == id) {
            f(j);
        }
    }

    fn step(&self, id: u64, step: &'static str) {
        self.update(id, |j| (j.step, j.since) = (step, now_ms()));
    }

    fn fail(&self, id: u64, error: String, log: Vec<String>) {
        self.update(id, |j| (j.state, j.error, j.log) = ("failed", Some(error), log));
    }

    /// Stops a running check (Pomelo is quitting).
    pub fn kill(&self) {
        if let Some(g) = self.group.lock().unwrap_or_else(|e| e.into_inner()).take() {
            tools::kill_group(g);
        }
    }

    /// Takes the job slot and returns its id (None while one runs).
    pub fn begin(&self, plan: &Plan) -> Option<u64> {
        let mut g = self.lock();
        if g.as_ref().is_some_and(|j| j.state == "running") {
            return None;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let subject = plan.message.trim().lines().next().unwrap_or("").to_string();
        *g = Some(Job { id, step: "check", state: "running", since: now_ms(), subject, sha: None, error: None, log: vec![], deploy_url: None, unknown: false });
        *self.log.lock().unwrap_or_else(|e| e.into_inner()) = Log::default();
        Some(id)
    }

    /// The whole run (blocking): check → commit and push → deployment.
    pub fn execute(&self, id: u64, plan: Plan) {
        let log = self.log.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let args: Vec<&str> = plan.check.1.iter().map(String::as_str).collect();
        let checked = tools::run_logged(&plan.check.0, &plan.folder, &args, plan.node_bin.as_deref(), &log, QA_LIMIT, |g| {
            *self.group.lock().unwrap_or_else(|e| e.into_inner()) = Some(g);
        });
        self.group.lock().unwrap_or_else(|e| e.into_inner()).take();
        std::thread::sleep(Duration::from_millis(50)); // the readers finish the last lines
        match checked {
            Ok(true) => {}
            Ok(false) => return self.fail(id, "网站的检查没有通过，什么都没提交。".into(), log.tail(60)),
            Err(e) => return self.fail(id, format!("网站的检查{e}，什么都没提交。"), log.tail(60)),
        }
        self.step(id, "push");
        let sha = match commit_and_push(&plan) {
            Ok(sha) => sha,
            Err(e) => return self.fail(id, e, vec![]),
        };
        self.update(id, |j| j.sha = Some(sha.clone()));
        self.step(id, "deploy");
        if plan.sandbox {
            std::thread::sleep(plan.fake_deploy);
            return self.update(id, |j| j.state = "done");
        }
        let (Some(gh), Some((owner, name))) = (plan.gh.as_deref(), plan.repo.as_ref()) else {
            return self.update(id, |j| (j.state, j.unknown) = ("done", true));
        };
        let begun = Instant::now();
        loop {
            if let Some((state, url)) = vercel_status(gh, &plan.folder, owner, name, &sha) {
                self.update(id, |j| j.deploy_url = url.clone().or(j.deploy_url.take()));
                match state.as_str() {
                    "success" => return self.update(id, |j| j.state = "done"),
                    "failure" | "error" => return self.fail(id, "Vercel 没有上线这次的改动，线上还是原来的样子。".into(), vec![]),
                    _ => {}
                }
            }
            if begun.elapsed() > DEPLOY_LIMIT {
                return self.update(id, |j| (j.state, j.unknown) = ("done", true));
            }
            std::thread::sleep(DEPLOY_EVERY);
        }
    }
}

pub fn spawn(p: Arc<Publisher>, id: u64, plan: Plan) {
    std::thread::spawn(move || p.execute(id, plan));
}

fn git(folder: &Path, args: &[&str]) -> Result<String, String> {
    tools::git(folder, args, NET)
}

/// Behind origin/main by how many commits (after fetching).
pub fn behind(folder: &Path) -> Result<(u32, u32), String> {
    let s = git(folder, &["rev-list", "--left-right", "--count", "HEAD...origin/main"])?;
    let mut it = s.split_whitespace().map(|n| n.parse::<u32>().unwrap_or(0));
    Ok((it.next().unwrap_or(0), it.next().unwrap_or(0)))
}

pub fn fetch(folder: &Path) -> Result<(), String> {
    git(folder, &["fetch", "--quiet", "origin", "main"]).map(|_| ()).map_err(|e| format!("连不上 GitHub：{e}"))
}

fn commit_and_push(plan: &Plan) -> Result<String, String> {
    let f = &plan.folder;
    if plan.sandbox && !super::is_local_remote(&super::origin_url(f).unwrap_or_default()) {
        return Err("开发版只推到本机的假远端，这个文件夹的远端是真的，没有发布。".into());
    }
    fetch(f)?;
    if behind(f)?.1 > 0 {
        return Err("检查的时候 GitHub 上有了新的提交，没有发布。再点一次「发布…」。".into());
    }
    let now = changes::list(f)?;
    if now.token != plan.token {
        return Err("检查的时候文件又改了，没有发布。再点一次「发布…」。".into());
    }
    if !now.files.is_empty() {
        git(f, &["add", "-A"]).map_err(|e| format!("提交失败：{e}"))?;
        let out = tools::run(Path::new(tools::GIT), f, &["commit", "--quiet", "--cleanup=whitespace", "-F", "-"], Some(plan.message.trim()), NET)?;
        if !out.ok {
            let _ = git(f, &["reset", "--quiet"]);
            return Err(format!("提交失败：{}", out.tail()));
        }
    }
    git(f, &["push", "--quiet", "origin", "main"]).map_err(|e| format!("推送失败（已经在本地提交了，下次发布会一起推）：{e}"))?;
    Ok(git(f, &["rev-parse", "HEAD"])?.trim().to_string())
}

/// (state, deployment page) of the newest status whose context starts with "Vercel".
pub fn vercel_in(statuses: &Value) -> Option<(String, Option<String>)> {
    statuses.as_array()?.iter().find(|s| s["context"].as_str().is_some_and(|c| c.starts_with("Vercel"))).map(|s| {
        let state = s["state"].as_str().unwrap_or("pending").to_ascii_lowercase();
        let url = s["target_url"].as_str().or(s["targetUrl"].as_str()).map(str::to_string);
        (state, url)
    })
}

fn gh_json(gh: &Path, folder: &Path, args: &[&str]) -> Option<Value> {
    let out = tools::run(gh, folder, args, None, Duration::from_secs(20)).ok()?;
    out.ok.then(|| serde_json::from_str(&out.stdout).ok()).flatten()
}

fn vercel_status(gh: &Path, folder: &Path, owner: &str, name: &str, sha: &str) -> Option<(String, Option<String>)> {
    let v = gh_json(gh, folder, &["api", &format!("repos/{owner}/{name}/commits/{sha}/status")])?;
    vercel_in(&v["statuses"])
}

/// https://vercel.com/<team>/<project> from a deployment page.
pub fn vercel_project(deploy_url: &str) -> Option<String> {
    let parts: Vec<&str> = deploy_url.split('/').collect();
    (parts.len() >= 5 && parts[2] == "vercel.com" && !parts[3].is_empty() && !parts[4].is_empty()).then(|| parts[..5].join("/"))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    pub sha: String,
    pub subject: String,
    pub time: i64,
    /// success | pending | failure (None = unknown)
    pub state: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub items: Vec<Published>,
    pub vercel_url: Option<String>,
}

const GRAPHQL: &str = "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){defaultBranchRef{target{... on Commit{history(first:30){nodes{oid status{contexts{context state targetUrl}}}}}}}}}";

/// Vercel's result per commit of the default branch (one call).
pub fn statuses(v: &Value) -> Vec<(String, (String, Option<String>))> {
    let nodes = &v["data"]["repository"]["defaultBranchRef"]["target"]["history"]["nodes"];
    nodes.as_array().into_iter().flatten().filter_map(|n| Some((n["oid"].as_str()?.to_string(), vercel_in(&n["status"]["contexts"])?))).collect()
}

/// The last publishes (origin/main's newest commits) and Vercel's result for each.
pub fn history(folder: &Path, sandbox: bool, gh: Option<&Path>, repo: Option<&(String, String)>) -> Result<History, String> {
    let log = tools::git(folder, &["log", "origin/main", "-n", &HISTORY.to_string(), "--format=%H%x1f%s%x1f%ct"], Duration::from_secs(10)).unwrap_or_default();
    let commits: Vec<Commit> = changes::parse_log(&log);
    let found = match (sandbox, gh, repo) {
        (false, Some(gh), Some((owner, name))) => gh_json(gh, folder, &["api", "graphql", "-f", &format!("query={GRAPHQL}"), "-f", &format!("owner={owner}"), "-f", &format!("name={name}")]).map(|v| statuses(&v)).unwrap_or_default(),
        _ => vec![],
    };
    let vercel_url = found.iter().find_map(|(_, (_, u))| u.as_deref().and_then(vercel_project));
    let items = commits
        .into_iter()
        .map(|c| {
            let hit = found.iter().find(|(oid, _)| *oid == c.sha).map(|(_, s)| s.clone());
            let (state, url) = match hit {
                Some((s, u)) => (Some(s), u),
                None if sandbox => (Some("success".into()), None),
                None => (None, None),
            };
            Published { sha: c.sha, subject: c.subject, time: c.time, state, url }
        })
        .collect();
    Ok(History { items, vercel_url })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::temp_dir;
    use crate::site::changes::tests::{git as tgit, site_repo, write};
    use serde_json::json;

    fn plan(site: &Path, check: &str, message: &str) -> Plan {
        let token = changes::list(site).unwrap().token;
        Plan {
            folder: site.into(),
            sandbox: true,
            check: ("/bin/sh".into(), vec!["-c".into(), check.into()]),
            node_bin: None,
            message: message.into(),
            token,
            repo: None,
            gh: None,
            fake_deploy: Duration::ZERO,
        }
    }

    fn run(p: &Publisher, plan: Plan) -> Job {
        let id = p.begin(&plan).unwrap();
        p.execute(id, plan);
        p.view().unwrap()
    }

    fn remote_head(remote: &Path) -> String {
        tgit(remote, &["log", "-1", "--format=%B", "main"])
    }

    #[test]
    fn a_failed_check_commits_nothing() {
        let dir = temp_dir("site-publish-fail");
        let (site, remote) = site_repo(&dir);
        write(&site, "src/content/wander/big-fish.zh.md", "---\ntitle: 大鱼\n---\n新的\n");
        let j = run(&Publisher::default(), plan(&site, "echo building; echo 'InvalidContentEntryDataError' >&2; exit 1", "x"));
        assert_eq!((j.step, j.state), ("check", "failed"));
        assert!(j.error.unwrap().contains("什么都没提交") && j.log.contains(&"InvalidContentEntryDataError".to_string()));
        assert_eq!(changes::list(&site).unwrap().files.len(), 1);
        assert_eq!(remote_head(&remote).trim(), "initial commit");
    }

    #[test]
    fn publishes_everything_with_the_text_as_written() {
        let dir = temp_dir("site-publish-ok");
        let (site, remote) = site_repo(&dir);
        write(&site, "src/content/wander/big-fish.zh.md", "---\ntitle: 大鱼\n---\n新的\n");
        write(&site, "src/components/Wall.astro", "<div class=\"y\"/>\n");
        let message = "content: 大鱼\n\n#2 第二段原样保留  \n";
        let p = Publisher::default();
        let j = run(&p, plan(&site, "test -f package.json || echo checked", message));
        assert_eq!((j.step, j.state, j.subject.as_str(), j.error), ("deploy", "done", "content: 大鱼", None));
        assert_eq!(remote_head(&remote).trim_end(), "content: 大鱼\n\n#2 第二段原样保留");
        assert_eq!(j.sha.unwrap(), tgit(&remote, &["rev-parse", "main"]).trim());
        assert!(changes::list(&site).unwrap().is_empty());
        // only unpushed commits: pushed without a new commit
        write(&site, "src/components/Wall.astro", "<div class=\"z\"/>\n");
        tgit(&site, &["commit", "-qam", "终端里提交的"]);
        let j = run(&p, plan(&site, "true", ""));
        assert_eq!(j.state, "done");
        assert_eq!(remote_head(&remote).trim(), "终端里提交的");
        // history: newest first, faked as online in the sandbox
        let h = history(&site, true, None, None).unwrap();
        assert_eq!(h.items.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["终端里提交的", "content: 大鱼", "initial commit"]);
        assert!(h.items.iter().all(|i| i.state.as_deref() == Some("success")) && h.vercel_url.is_none());
    }

    #[test]
    fn stops_when_files_or_github_moved_on() {
        let dir = temp_dir("site-publish-moved");
        let (site, remote) = site_repo(&dir);
        write(&site, "src/content/wander/big-fish.zh.md", "---\ntitle: 大鱼\n---\n新的\n");
        // edited during the check
        let j = run(&Publisher::default(), plan(&site, "echo more >> src/content/wander/big-fish.zh.md", "x"));
        assert!(j.state == "failed" && j.step == "push" && j.error.unwrap().contains("文件又改了"));
        // someone pushed meanwhile
        let other = dir.join("other");
        tgit(&dir, &["clone", "-q", remote.to_str().unwrap(), "other"]);
        write(&other, "README.md", "x\n");
        tgit(&other, &["add", "-A"]);
        tgit(&other, &["commit", "-qm", "别处推的"]);
        tgit(&other, &["push", "-q", "origin", "main"]);
        let j = run(&Publisher::default(), plan(&site, "true", "x"));
        assert!(j.state == "failed" && j.error.unwrap().contains("新的提交"));
        assert_eq!(remote_head(&remote).trim(), "别处推的");
        // a sandbox never pushes to a real remote
        tgit(&site, &["remote", "set-url", "origin", "https://github.com/elenskye/elenskye-website.git"]);
        let j = run(&Publisher::default(), plan(&site, "true", "x"));
        assert!(j.error.unwrap().contains("假远端"));
    }

    #[test]
    fn one_job_at_a_time() {
        let dir = temp_dir("site-publish-one");
        let (site, _) = site_repo(&dir);
        let p = Publisher::default();
        let pl = plan(&site, "true", "x");
        let id = p.begin(&pl).unwrap();
        assert!(p.begin(&pl).is_none() && p.running());
        p.dismiss();
        assert!(p.view().is_some());
        p.execute(id, pl);
        p.dismiss();
        assert!(p.view().is_none() && p.begin(&plan(&site, "true", "x")).is_some());
    }

    #[test]
    fn reads_vercel_results() {
        let st = json!([{ "context": "Vercel", "state": "success", "target_url": "https://vercel.com/team/site/AbC" }, { "context": "ci", "state": "failure" }]);
        assert_eq!(vercel_in(&st), Some(("success".into(), Some("https://vercel.com/team/site/AbC".into()))));
        assert_eq!(vercel_in(&json!([{ "context": "ci", "state": "success" }])), None);
        assert_eq!(vercel_project("https://vercel.com/team/site/AbC").as_deref(), Some("https://vercel.com/team/site"));
        assert_eq!(vercel_project("https://example.com/a/b/c"), None);
        let g = json!({ "data": { "repository": { "defaultBranchRef": { "target": { "history": { "nodes": [
            { "oid": "a1", "status": { "contexts": [{ "context": "Vercel", "state": "PENDING", "targetUrl": "https://vercel.com/team/site/1" }] } },
            { "oid": "b2", "status": null },
            { "oid": "c3", "status": { "contexts": [{ "context": "Vercel", "state": "FAILURE", "targetUrl": null }] } }
        ] } } } } } });
        assert_eq!(statuses(&g), vec![("a1".into(), ("pending".into(), Some("https://vercel.com/team/site/1".into()))), ("c3".into(), ("failure".into(), None))]);
    }
}
