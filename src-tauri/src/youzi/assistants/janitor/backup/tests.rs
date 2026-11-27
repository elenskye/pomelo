use super::*;
use crate::db::tests::{temp_dir, TempDir};
use crate::youzi::assistants::janitor::{agent, ensure_sandbox, fake};
use std::os::unix::fs::MetadataExt;

fn sandbox(name: &str) -> (TempDir, Janitor, Volume) {
    let dir = temp_dir(&format!("backup-{name}"));
    let j = Janitor::new(Mode::Sandbox, Path::new("/Users/nobody"), &dir);
    ensure_sandbox(&j.root).unwrap();
    fake::ensure_volumes(&j.backup.volumes.dir).unwrap();
    let v = j.backup.volumes.find(fake::DRIVE_UUID).expect("the fake drive");
    (dir, j, v)
}

fn task(id: i64, source: &str, target: &str) -> Task {
    Task { id, source: format!("/Users/me/Downloads/{source}"), target: target.into() }
}

fn paths<T>(v: &[T], f: impl Fn(&T) -> &str) -> Vec<String> {
    let mut p: Vec<String> = v.iter().map(|x| diff::nfc(f(x))).collect();
    p.sort();
    p
}

fn compared(j: &Janitor, v: &Volume, t: &Task) -> Diff {
    compare_task(j, v, t, &AtomicBool::new(false)).unwrap().0
}

#[test]
fn the_fake_drive_is_found_by_its_uuid() {
    let (_d, j, v) = sandbox("volumes");
    assert_eq!((v.name.as_str(), v.fs.as_str(), v.total), (fake::DRIVE, "Mac OS Extended (Journaled)", 2_000 * rules::GB));
    assert!(v.free < v.total);
    let mut want: Vec<String> = ["影音", "照片", "回忆", "回忆_2606", "资料"].map(String::from).to_vec();
    want.sort();
    assert_eq!(top_folders(&v), want);
    assert!(j.backup.volumes.find("nope").is_none());
}

#[test]
fn comparing_sorts_every_file_and_never_lists_vault() {
    let (_d, j, v) = sandbox("compare");
    // 资料: one new, one newer on the Mac (the drive's goes to ARCHIVE), two only on the drive; the Office lock file left out
    let d = compared(&j, &v, &task(1, "资料", "资料"));
    assert_eq!(paths(&d.new, |r| &r.path), ["2609 实习/合同.pdf"]);
    assert_eq!(paths(&d.changed, |r| &r.path), ["2609 实习/日程.xlsx"]);
    assert_eq!(paths(&d.archived, |r| &r.path), ["2302 证书.zip", "2507 课程/大作业.zip"]);
    assert_eq!((d.same, d.moved.len(), d.held.len(), d.recent.len()), (3, 0, 0, 0));
    // 照片: a renamed folder is two moves (not copied again), the decomposed name on the drive is the same file
    let d = compared(&j, &v, &task(2, "照片", "照片"));
    assert_eq!(paths(&d.moved, |m| &m.to), ["其他/IMG_0101.jpg", "其他/IMG_0102.jpg"]);
    assert_eq!(paths(&d.moved, |m| &m.from), ["其它/IMG_0101.jpg", "其它/IMG_0102.jpg"]);
    assert_eq!(paths(&d.new, |r| &r.path), ["截图/截屏 1.png", "截图/截屏 2.png"]);
    assert_eq!(paths(&d.archived, |r| &r.path), ["素材/纹理.zip"]);
    assert_eq!(d.same, 3, "Café in NFC and NFD is one file");
    // 小柚 → 回忆/小柚: a file newer on the drive is held
    let d = compared(&j, &v, &task(3, "小柚", "回忆/小柚"));
    assert_eq!(paths(&d.new, |r| &r.path), ["2026/c.jpg"]);
    assert_eq!(d.held.iter().map(|h| (h.path.as_str(), h.why.as_str())).collect::<Vec<_>>(), [("2026/d.jpg", "硬盘上的更新")]);
    // 作品 isn't on the drive: all new, but not what rebuilds
    let d = compared(&j, &v, &task(4, "作品", "作品"));
    assert_eq!(paths(&d.new, |r| &r.path), ["lab/train.ipynb", "web/index.html", "笔记/读书.md", "设计/海报.psd"]);
    // VAULT: on neither side, under no name
    for t in [task(5, "", "资料")] {
        let all = diff::list(&j.real_of(&t.source), &Skip::default(), &AtomicBool::new(false)).unwrap();
        assert!(all.files.keys().chain(all.dirs.keys()).all(|k| !k.to_lowercase().contains("vault")), "{:?}", all.dirs);
    }
    let drive_all = diff::list(&v.path, &Skip::default(), &AtomicBool::new(false)).unwrap();
    assert!(drive_all.files.keys().chain(drive_all.dirs.keys()).all(|k| !k.contains("VAULT") && !k.contains("秘密")));
}

#[test]
fn a_run_copies_only_what_changed_and_never_deletes() {
    let (_d, j, v) = sandbox("run");
    let tasks = [task(1, "资料", "资料"), task(2, "照片", "照片"), task(3, "小柚", "回忆/小柚"), task(4, "作品", "作品")];
    // a Finder tag on a new file comes along
    let mac_contract = j.real_of("/Users/me/Downloads/资料/2609 实习/合同.pdf");
    assert!(std::process::Command::new("/usr/bin/xattr").args(["-w", "com.apple.metadata:_kMDItemUserTags", "(\"红色\\n6\")"]).arg(&mac_contract).status().unwrap().success());
    let run = Run::default();
    let o = run_tasks(&j, &v, &tasks, &run, "2026-11-25");
    assert!(o.failed.is_none() && !o.stopped && o.errors.is_empty(), "{o:?}");
    assert_eq!(o.files, 1 + 1 + 2 + 1 + 4, "合同 + 日程, two screenshots, c.jpg, four of 作品");
    assert!(o.tasks.iter().all(|t| t.done));
    assert_eq!(o.tasks[0].updated, 1);
    assert_eq!((run.files_done.load(Ordering::Relaxed), run.bytes_done.load(Ordering::Relaxed)), (9, run.bytes_total.load(Ordering::Relaxed)));
    let on_drive = |rel: &str| v.path.join(rel);
    // the copy keeps the time and the tag
    let (m, c) = (std::fs::metadata(&mac_contract).unwrap(), std::fs::metadata(on_drive("资料/2609 实习/合同.pdf")).unwrap());
    assert_eq!((c.len(), c.mtime()), (m.len(), m.mtime()));
    let tag = std::process::Command::new("/usr/bin/xattr").args(["-p", "com.apple.metadata:_kMDItemUserTags"]).arg(on_drive("资料/2609 实习/合同.pdf")).output().unwrap();
    assert!(String::from_utf8_lossy(&tag.stdout).contains("红色"));
    // the old 日程 is in ARCHIVE, the new one in its place
    assert_eq!(std::fs::metadata(on_drive(&format!("{}/2026-11-25/资料/2609 实习/日程.xlsx", copy::OLD))).unwrap().len(), 16_000);
    assert_eq!(std::fs::metadata(on_drive("资料/2609 实习/日程.xlsx")).unwrap().len(), 15_000);
    // held, moved, archived and left-out ones untouched; nothing half-written; VAULT neither side
    assert_eq!(std::fs::metadata(on_drive("回忆/小柚/2026/d.jpg")).unwrap().mtime(), (fake_base() as i64));
    assert!(!on_drive("照片/其他").exists() && on_drive("照片/其它/IMG_0101.jpg").exists());
    assert!(on_drive("资料/2302 证书.zip").exists() && on_drive("影音/电影/片.mkv").exists());
    assert!(!on_drive("资料/~$周报.docx").exists() && !on_drive("作品/lab/.ipynb_checkpoints").exists() && !on_drive("作品/web/node_modules").exists());
    assert!(!on_drive("VAULT.sparsebundle").exists() && on_drive("VAULT/秘密.txt").exists());
    let parts = diff::list(&v.path, &Skip::default(), &AtomicBool::new(false)).unwrap();
    assert!(parts.files.keys().all(|k| !k.contains(diff::PART)));
    // compared again: nothing left to copy; what the Mac had is remembered
    for t in &tasks {
        let d = compared(&j, &v, t);
        assert!(d.new.is_empty() && d.changed.is_empty(), "{}: {d:?}", t.target);
    }
    assert_eq!(j.with_db(|c| store::seen(c, 3)).unwrap().len(), 4);
    // deleted on the Mac since: still on the drive, said as recent (not archived)
    std::fs::remove_file(j.real_of("/Users/me/Downloads/小柚/2026/c.jpg")).unwrap();
    let d = compared(&j, &v, &tasks[2]);
    assert_eq!(paths(&d.recent, |r| &r.path), ["2026/c.jpg"]);
    assert!(on_drive("回忆/小柚/2026/c.jpg").exists());
}

/// d.jpg on the fake drive is dated BASE (0 days before it).
fn fake_base() -> u64 {
    1_788_220_800
}

#[test]
fn moves_are_done_on_the_drive_when_picked() {
    let (_d, j, v) = sandbox("moves");
    let t = task(2, "照片", "照片");
    let (done, failed) = apply_moves(&j, &v, &t, &["其他/IMG_0101.jpg".into()]).unwrap();
    assert_eq!((done, failed.len()), (1, 0));
    let d = compared(&j, &v, &t);
    assert_eq!(paths(&d.moved, |m| &m.to), ["其他/IMG_0102.jpg"]);
    let (done, _) = apply_moves(&j, &v, &t, &["其他/IMG_0102.jpg".into()]).unwrap();
    assert_eq!(done, 1);
    // the old folder emptied goes; nothing copied twice
    assert!(!v.path.join("照片/其它").exists() && v.path.join("照片/其他/IMG_0102.jpg").exists());
    let d = compared(&j, &v, &t);
    assert!(d.moved.is_empty() && d.same == 5);
}

#[test]
fn stopping_leaves_no_half_file_and_a_full_drive_refuses() {
    let (_d, j, v) = sandbox("stop");
    let run = Run::default();
    run.stop.store(true, Ordering::Relaxed);
    let o = run_tasks(&j, &v, &[task(4, "作品", "作品")], &run, "2026-11-25");
    assert!(o.stopped && o.files == 0 && !o.tasks.iter().any(|t| t.done));
    assert!(j.with_db(|c| store::seen(c, 4)).unwrap().is_empty());
    let left = diff::list(&v.path, &Skip::default(), &AtomicBool::new(false)).unwrap();
    assert!(left.files.keys().all(|k| !k.starts_with("作品")));
    // a drive with no room: nothing starts
    let small = Volume { free: 1000, ..v.clone() };
    let o = run_tasks(&j, &small, &[task(4, "作品", "作品")], &Run::default(), "2026-11-25");
    assert!(o.failed.as_deref().is_some_and(|f| f.contains("空间不够")) && o.files == 0, "{o:?}");
}

fn listed(j: &Janitor, picked: &[&str]) -> Settings {
    let s = Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), picked: Some(picked.iter().map(|p| p.to_string()).collect()), ..Default::default() };
    settled(j, &s)
}

fn targets(s: &Settings) -> Vec<(i64, String, String)> {
    s.tasks.iter().map(|t| (t.id, t.source.trim_start_matches("/Users/me/Downloads/").to_string(), t.target.clone())).collect()
}

#[test]
fn the_list_makes_the_tasks() {
    let (_d, j, v) = sandbox("list");
    let drive = Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), ..Default::default() };
    // a fresh start ticks nothing
    let s = settled(&j, &drive);
    assert_eq!((s.picked.clone(), s.tasks.len()), (Some(vec![]), 0));
    // the list: the drive's top folders and Downloads' (VAULT, ARCHIVE and hidden ones never)
    std::fs::create_dir_all(v.path.join(copy::OLD)).unwrap();
    let rows = overview(&j, &s, Vec::new()).list;
    let row = |n: &str| rows.iter().find(|r| r.name == n).map(|r| (r.on_drive, r.on_mac, r.picked));
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["作品", "回忆", "回忆_2606", "小柚", "影音", "照片", "资料"]);
    assert!(rows.iter().all(|r| !r.name.to_lowercase().contains("vault") && r.name != copy::OLD));
    assert_eq!((row("资料"), row("回忆"), row("作品")), (Some((true, true, false)), Some((true, false, false)), Some((false, true, false))));
    // ticking: Downloads' folders become tasks to the drive folder of the same name; the drive's own just stay in
    let (s, gone) = set_picked(&j, &s, "资料", true).unwrap();
    let (s, _) = set_picked(&j, &s, "回忆", true).unwrap();
    let (s, _) = set_picked(&j, &s, "作品", true).unwrap();
    assert!(gone.is_empty());
    assert_eq!(s.picked.clone().unwrap(), ["作品", "回忆", "资料"]);
    assert_eq!(targets(&s), [(2, "作品".to_string(), "作品".to_string()), (1, "资料".into(), "资料".into())]);
    for (name, why) in [("VAULT", "都没有"), (copy::OLD, "都没有"), ("没有", "都没有")] {
        assert!(set_picked(&j, &s, name, true).unwrap_err().contains(why), "{name}");
    }
    assert!(set_picked(&j, &Settings { drive: None, ..s.clone() }, "资料", true).unwrap_err().contains("插上移动硬盘"));
    // a task keeps its id; unticked it goes (and is said, to forget what it held)
    let id = s.tasks.iter().find(|t| t.target == "资料").unwrap().id;
    let (s2, _) = set_picked(&j, &s, "回忆", false).unwrap();
    assert!(s2.tasks.iter().any(|t| t.id == id));
    let (s3, gone) = set_picked(&j, &s2, "资料", false).unwrap();
    assert_eq!((gone, s3.tasks.len(), s3.picked.clone().unwrap()), (vec![id], 1, vec!["作品".to_string()]));
    // the overview says what's ticked
    let rows = overview(&j, &s2, Vec::new()).list;
    assert_eq!(rows.iter().filter(|r| r.picked).map(|r| r.name.as_str()).collect::<Vec<_>>(), ["作品", "资料"]);
    // old settings (tasks, folders kept out) become a list once the drive is in: all but those kept out, and
    // the tasks' folders; a task whose drive folder had another name now goes to the one of its own name
    let old = Settings { tasks: vec![task(1, "小柚", "回忆/小柚"), task(2, "照片", "照片")], next_id: 2, skipped: vec!["回忆_2606".into()], ..drive.clone() };
    let (none, _) = settle(&j, &old, None, &mac_folders(&j));
    assert_eq!(none, old, "the drive out: as it was");
    let s = settled(&j, &old);
    assert_eq!(s.picked.clone().unwrap(), ["回忆", "小柚", "影音", "照片", "资料"]);
    assert!(s.skipped.is_empty());
    assert_eq!(targets(&s), [(1, "小柚".to_string(), "小柚".to_string()), (2, "照片".into(), "照片".into()), (3, "资料".into(), "资料".into())]);
    assert_eq!(settled(&j, &s), s, "settled stays");
    // settings from before 1.1.3 (a task's own names left out) still read; the names go
    let old: Settings = serde_json::from_str(r#"{"tasks":[{"id":4,"source":"/Users/me/Downloads/照片","target":"照片","excludes":["*.tmp"]}],"nextId":4,"picked":["照片"],"pikpak":{"account":null,"folder":"","limit":0}}"#).unwrap();
    assert_eq!((old.tasks[0].id, old.tasks[0].target.as_str()), (4, "照片"));
    assert!(!serde_json::to_string(&old).unwrap().contains("excludes"));
    // a picked name gone from both sides goes from the list
    std::fs::rename(j.real_of("/Users/me/Downloads/小柚"), j.real_of("/Users/me/Downloads/小柚 2")).unwrap();
    let s = settled(&j, &s);
    assert!(!s.picked.clone().unwrap().contains(&"小柚".to_string()) && !s.tasks.iter().any(|t| t.id == 1));
}

#[test]
fn the_page_gets_counts_and_the_first_rows() {
    let (_d, j, _v) = sandbox("overview");
    let s = Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), tasks: vec![task(1, "资料", "资料")], next_id: 1, ..Default::default() };
    compare_all(&j, &s);
    let o = overview(&j, &s, Vec::new());
    assert!(o.drive.as_ref().unwrap().connected);
    let row = o.list.iter().find(|r| r.name == "资料").unwrap();
    assert_eq!((row.on_drive, row.on_mac, row.picked, row.files), (true, true, true, Some(6)), "old settings: all in");
    assert!(!o.list.iter().any(|r| r.name.to_lowercase().contains("vault")));
    let t = &o.tasks[0];
    let diff = t.diff.as_ref().expect("compared");
    assert_eq!((diff.new.count, diff.changed.count, diff.archived.count, diff.same), (1, 1, 2, 3));
    assert!(t.problem.is_none() && !o.comparing);
    // tasks changed since: not fresh any more
    let s2 = Settings { tasks: vec![task(1, "资料", "资料"), task(2, "照片", "照片")], ..s.clone() };
    assert!(overview(&j, &s2, Vec::new()).comparing);
}

#[test]
fn runs_are_recorded() {
    let c = crate::youzi::store::tests::memory_db();
    let id = record_start(&c, "盘", &[task(1, "资料", "资料")]).unwrap();
    assert_eq!(runs(&c, 10).unwrap()[0].status, "running");
    settle_stale(&c, "drive").unwrap();
    assert_eq!(runs(&c, 10).unwrap()[0].note.as_deref(), Some("App 关掉时还没做完"));
    let o = Outcome { tasks: vec![RunTask { id: 1, source: "s".into(), target: "资料".into(), files: 2, bytes: 10, updated: 1, done: true }], files: 2, bytes: 10, errors: vec![("资料/x".into(), "没权限".into())], ..Default::default() };
    record_end(&c, id, &o).unwrap();
    let r = &runs(&c, 10).unwrap()[0];
    assert_eq!((r.status.as_str(), r.files, r.errors.len(), r.tasks[0].done), ("done", 2, 1, true));
    assert!(r.finished_at.is_some());
    assert_eq!(ended_text(&o), "备份好了：复制了 2 个文件（1 KB）；1 个没成");
}

#[test]
fn the_agent_never_sees_vault() {
    let (_d, j, _v) = sandbox("agent-vault");
    let js = crate::youzi::assistants::janitor::Settings::default();
    let site = "/Users/me/Documents/GitHub/personal-website";
    let look = |args: &str| agent::look_with(&j, &js, &[], site, args, 8000);
    let a = look(r#"{"view":"folder","path":"~/Downloads"}"#);
    assert!(a.text.contains("资料") && !a.text.to_lowercase().contains("vault"), "{}", a.text);
    let a = look(r#"{"view":"folder","path":"~/Downloads/VAULT.sparsebundle"}"#);
    assert!(a.state != "ok" && !a.text.contains("bands"), "{}", a.text);
    let a = look(r#"{"view":"find","path":"~/Downloads","ext":"sparsebundle"}"#);
    assert!(!a.text.to_lowercase().contains("vault"), "{}", a.text);
    let a = look(r#"{"view":"find","path":"~","name":"0"}"#);
    assert!(!a.text.contains("bands"), "{}", a.text);
    let card = agent::build_card(&j, site, agent::TRASH, &serde_json::json!({ "items": [{ "path": "~/Downloads/VAULT.sparsebundle", "why": "x" }] }).to_string());
    assert!(card.is_err());
    let a = agent::run_readonly_with(&j, r#"{"command":"ls -R ~/Downloads/vault.sparsebundle"}"#, 4000);
    assert!(a.state != "ok", "{}", a.text);
}

/// Real folders, read only (names and metadata; prints counts, no names; writes nothing):
/// POMELO_BACKUP_PAIRS="<Mac folder>=><drive folder>;…"
#[test]
#[ignore]
fn real_backup_compare() {
    let pairs = std::env::var("POMELO_BACKUP_PAIRS").expect("POMELO_BACKUP_PAIRS");
    for pair in pairs.split(';').filter(|p| !p.trim().is_empty()) {
        let (mac, drive) = pair.split_once("=>").expect("a=>b");
        let t = std::time::Instant::now();
        let stop = AtomicBool::new(false);
        let m = diff::list(Path::new(mac.trim()), &Skip::default(), &stop).unwrap();
        let d = diff::list(Path::new(drive.trim()), &Skip::default(), &stop).unwrap();
        let x = diff::compare(&m, &d, &HashMap::new());
        let mb = |v: u64| v / 1_000_000;
        println!(
            "{} files / {} on the drive ({} ms): same {} · new {} ({} MB) · changed {} · moved {} · held {} · only on the drive {} ({} MB) · folders to make {} · unreadable {}",
            m.files.len(), d.files.len(), t.elapsed().as_millis(), x.same, x.new.len(), mb(x.new.iter().map(|r| r.size).sum()), x.changed.len(), x.moved.len(), x.held.len(), x.archived.len(), mb(x.archived.iter().map(|r| r.size).sum()), x.dirs.len(), x.denied.len()
        );
        let vault = |l: &diff::Listing| l.files.keys().chain(l.dirs.keys()).any(|k| k.split('/').any(rules::sealed_name));
        assert!(!vault(&m) && !vault(&d));
    }
}

/// The drives connected to this Mac now (read only): `cargo test real_backup_volumes -- --ignored --nocapture`.
#[test]
#[ignore]
fn real_backup_volumes() {
    let v = Volumes::new(PathBuf::from("/Volumes"), false);
    let t = std::time::Instant::now();
    let list = v.list();
    let again = std::time::Instant::now();
    assert_eq!(v.list().len(), list.len());
    for x in &list {
        println!("{} · {} · {} GB, {} GB free · uuid {}…", x.name, x.fs, x.total / 1_000_000_000, x.free / 1_000_000_000, &x.uuid[..8.min(x.uuid.len())]);
    }
    println!("first {} ms, then {} ms", (again - t).as_millis(), again.elapsed().as_millis());
    assert!(list.iter().all(|x| x.name != "Macintosh HD" && !x.uuid.is_empty()));
}

// ---- Phase 2: 柚子's clues, cards and look ----

fn with_tasks(tasks: Vec<Task>) -> Settings {
    let next_id = tasks.iter().map(|t| t.id).max().unwrap_or(0);
    Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), tasks, next_id, ..Default::default() }
}

fn clues(j: &Janitor, s: &Settings) -> hints::Hints {
    compare_all(j, s);
    fresh_snapshot(j, s).and_then(|sn| sn.hints).expect("clues")
}

fn lines(h: &hints::Hints) -> Vec<String> {
    let mut v: Vec<String> = h.suggestions().iter().map(|s| s.line()).collect();
    v.sort();
    v
}

#[test]
fn clues_match_folders_and_renames() {
    let (_d, j, v) = sandbox("clues");
    let all = ["资料", "照片", "回忆", "回忆_2606", "影音"];
    // the drive's folders ticked, none of Downloads' but 资料 and 照片 (both sides): the others are only said
    let s = listed(&j, &all);
    let h = clues(&j, &s);
    assert_eq!(lines(&h), ["在硬盘上改名 照片/其它 → 照片/其他"]);
    let small = h.folders.iter().find(|f| f.mac.ends_with("小柚")).unwrap();
    assert_eq!(small.found.iter().map(|f| (f.drive.as_str(), f.files)).collect::<Vec<_>>(), [("回忆/小柚", 2), ("回忆_2606/小柚", 1)]);
    assert_eq!((small.task, small.note.as_deref()), (None, Some("硬盘上在 回忆/小柚")));
    assert_eq!(h.folders.iter().find(|f| f.mac.ends_with("作品")).unwrap().note, None);
    assert!(h.folders.iter().all(|f| !f.mac.to_lowercase().contains("vault")));
    assert_eq!((h.renames[0].task, h.renames[0].files), (s.tasks.iter().find(|t| t.target == "照片").unwrap().id, 2));
    // what only the drive has (影音, 回忆_2606) is archived: not said; duplicates aren't the backup's
    let text = h.text(&j.home, gb);
    assert!(text.contains("清单里没勾（不备份）") && !text.contains("影音") && !text.contains("回忆_2606 ") && !text.to_lowercase().contains("vault") && !text.contains(copy::OLD), "{text}");
    // 小柚 ticked: the drive has it under 回忆, not under its own name → rename it there
    let s = listed(&j, &[&all[..], &["小柚"]].concat());
    assert!(s.tasks.iter().any(|t| t.target == "小柚"));
    let h = clues(&j, &s);
    assert_eq!(lines(&h), ["在硬盘上改名 回忆/小柚 → 小柚", "在硬盘上改名 照片/其它 → 照片/其他"]);
    // the drive folder of its name is there but holds none of it: only said
    std::fs::create_dir_all(v.path.join("小柚")).unwrap();
    std::fs::write(v.path.join("小柚/别的.txt"), "x").unwrap();
    let h = clues(&j, &listed(&j, &[&all[..], &["小柚"]].concat()));
    let small = h.folders.iter().find(|f| f.mac.ends_with("小柚")).unwrap();
    assert!(small.suggest.is_none() && small.note.as_deref() == Some("硬盘上的 小柚 里一个都没有，它们在 回忆/小柚"), "{small:?}");
}

#[test]
fn a_folder_kept_out_is_left_alone() {
    use serde_json::json;
    let (_d, j, v) = sandbox("skipped");
    // 回忆_2606 not ticked
    let s = listed(&j, &["资料", "回忆", "影音"]);
    for (name, args) in [
        (cards::MOVE, json!({ "items": [{ "from": "回忆_2606/小柚", "to": "回忆/小柚_旧", "why": "x" }] })),
        (cards::MOVE, json!({ "items": [{ "from": "影音", "to": "回忆_2606/影音", "why": "x" }] })),
        (cards::MOVE, json!({ "items": [{ "from": "回忆/小柚", "to": "小柚", "why": "x" }] })),
    ] {
        let e = build(&j, &s, name, args.clone()).unwrap_err();
        assert!(e.contains("不备份"), "{args}: {e}");
    }
    let mv = build(&j, &listed(&j, &["资料", "回忆", "影音", "回忆_2606"]), cards::MOVE, json!({ "items": [{ "from": "回忆_2606/小柚", "to": "回忆_2606/小柚_旧", "why": "x" }] })).unwrap();
    assert!(cards::run(&j, &s, &mv, &[0]).unwrap().1["items"][0]["error"].as_str().unwrap_or("").contains("不备份"), "kept out after the card was made");
    assert!(v.path.join("回忆_2606/小柚").exists());
    // the clues leave it out, only a line saying it's out
    let h = clues(&j, &s);
    assert!(h.folders.iter().all(|f| f.found.iter().all(|x| !x.drive.starts_with("回忆_2606"))));
    assert!(h.skipped.contains(&"回忆_2606".to_string()) && h.skipped.contains(&"照片".to_string()));
    let text = h.text(&j.home, gb);
    assert!(text.contains("清单里没勾的文件夹") && text.contains("回忆_2606"), "{text}");
    // the cloud: only the ticked folders, both ways
    let f = rclone::only(&["回忆".into(), "a*[b]".into()]);
    assert_eq!(f, ["--filter", "+ /回忆/**", "--filter", "+ /a\\*\\[b\\]/**", "--filter", "- **"]);
    if !has_rclone() {
        return;
    }
    rclone::Rclone::new(&j, None).unwrap().login("xiaoyou@example.com", "x").unwrap();
    let cloud = rclone::fake_root(&j).join(fake::DRIVE);
    for f in ["回忆_2606/早先传的/a.txt", "ARCHIVE/以前的/b.txt"] {
        std::fs::create_dir_all(cloud.join(f).parent().unwrap()).unwrap();
        std::fs::write(cloud.join(f), "x").unwrap();
    }
    let ps = Settings { picked: s.picked.clone(), ..with_pikpak(0) };
    assert_eq!(pikpak::picked_on(&ps, &v), ["回忆", "影音", "资料"]);
    let (_, d) = pikpak::sync_for_test(&j, &ps, &["回忆", "影音", "资料"], 100);
    assert!(d.ok, "{d:?}");
    let up = cloud_files(&j, fake::DRIVE);
    assert!(up.contains(&"回忆_2606/早先传的/a.txt".to_string()) && up.contains(&"ARCHIVE/以前的/b.txt".to_string()) && !up.contains(&"回忆_2606/小柚/2024/a.mov".to_string()) && up.contains(&"回忆/小柚/2024/a.mov".to_string()), "{up:?}");
    assert!(!up.iter().any(|k| k.starts_with("照片/")), "not ticked: not up there: {up:?}");
}

#[test]
fn suggestion_lines_read_back() {
    let home = "/Users/me";
    for s in [hints::Suggest::Rename { from: "照片/其它".into(), to: "照片/其他".into() }] {
        assert_eq!(hints::Suggest::parse(&s.line()), Some(s.clone()));
        assert_eq!(hints::Suggest::all_in(&format!("- x（1 个）；{}{}\n- y", hints::CAN, s.line())), vec![s]);
    }
    assert_eq!(hints::tilde(home, "/Users/me/Downloads/a"), "~/Downloads/a");
    assert_eq!(hints::tilde(home, "/Users/meow/a"), "/Users/meow/a");
}

fn build(j: &Janitor, s: &Settings, name: &str, args: serde_json::Value) -> Result<agent::Card, String> {
    cards::build(j, s, name, &args.to_string()).map(|c| agent::Card { state: "pending".into(), ..c })
}

#[test]
fn backup_cards_are_checked_then_done_on_the_drive() {
    use serde_json::json;
    let (_d, j, v) = sandbox("cards");
    let s = with_tasks(vec![task(2, "照片", "照片"), task(3, "小柚", "回忆/小柚")]);
    // checked: paths there, nothing in VAULT or ARCHIVE, nothing taken; no dup or task cards any more
    std::fs::create_dir_all(v.path.join(copy::OLD).join("x")).unwrap();
    for (name, args, why) in [
        (cards::MOVE, json!({ "items": [{ "from": "照片/其它", "to": "照片/城市", "why": "x" }] }), "已经有"),
        (cards::MOVE, json!({ "items": [{ "from": "照片/没有", "to": "照片/x", "why": "x" }] }), "硬盘上没有"),
        (cards::MOVE, json!({ "items": [{ "from": format!("{}/x", copy::OLD), "to": "x", "why": "x" }] }), "归档"),
        (cards::MOVE, json!({ "items": [{ "from": "照片/其它", "to": format!("{}/其它", copy::OLD), "why": "x" }] }), "归档"),
        (cards::MOVE, json!({ "items": [{ "from": "照片", "to": "照片/里面", "why": "x" }] }), "自己里面"),
        (cards::MOVE, json!({ "items": [{ "from": "VAULT/秘密.txt", "to": "x", "why": "x" }] }), "不能动"),
        (cards::MOVE, json!({ "items": [{ "from": "../x", "to": "x", "why": "x" }] }), "不是硬盘上的路径"),
        (cards::TRASH, json!({ "items": [{ "path": "回忆_2606", "why": "x" }] }), "不归备份管"),
        (cards::TASK, json!({ "source": "~/Downloads/作品", "target": "作品", "why": "x" }), "清单"),
    ] {
        let e = build(&j, &s, name, args.clone()).unwrap_err();
        assert!(e.contains(why), "{args}: {e}");
    }
    // a whole folder renamed on the drive; nothing copied again
    let mv = build(&j, &s, cards::MOVE, json!({ "items": [{ "from": "照片/其它", "to": "照片/其他", "why": "电脑上改了名" }] })).unwrap();
    assert_eq!((mv.kind.as_str(), mv.items[0].kind.as_str(), mv.items[0].size, mv.drive.as_deref()), ("move", "文件夹", 250_000, Some(fake::DRIVE_UUID)));
    let (st, _, after) = cards::run(&j, &s, &mv, &[0]).unwrap();
    assert!(st == "done" && after.is_none());
    let d = compared(&j, &v, &s.tasks[0]);
    assert!(d.moved.is_empty() && d.same == 5 && !v.path.join("照片/其它").exists());
    assert!(cards::run(&j, &s, &agent::Card { state: "done".into(), ..mv.clone() }, &[0]).is_err());
    // a dup card from before 1.1.3 only says it's gone; the drive untouched
    let tr = agent::Card { kind: "drive_trash".into(), state: "pending".into(), drive: Some(fake::DRIVE_UUID.into()), items: vec![agent::TrashItem { path: "回忆_2606".into(), ..Default::default() }], ..Default::default() };
    let (st, res, _) = cards::run(&j, &s, &tr, &[0]).unwrap();
    assert!(st == "failed" && res["error"].as_str().unwrap().contains("不归备份管") && v.path.join("回忆_2606").exists());
    // a task card from before the 清单 only says where to go now
    let old = agent::Card { kind: "task".into(), state: "pending".into(), drive: Some(fake::DRIVE_UUID.into()), task: Some(cards::TaskPlan { source: "/Users/me/Downloads/作品".into(), target: "作品".into(), ..Default::default() }), ..Default::default() };
    let (st, res, after) = cards::run(&j, &s, &old, &[]).unwrap();
    assert!(st == "failed" && res["error"].as_str().unwrap().contains("清单") && after.is_none());
    let after = s.clone();
    // moving a task's drive folder takes the task along
    let mv = build(&j, &after, cards::MOVE, json!({ "items": [{ "from": "回忆/小柚", "to": "小柚", "why": "和电脑上同名" }] })).unwrap();
    assert_eq!(mv.items[0].note.as_deref(), Some("Downloads 里的 小柚 备份到这里，会跟着改"));
    let (_, _, moved) = cards::run(&j, &after, &mv, &[0]).unwrap();
    let moved = moved.unwrap();
    assert_eq!(moved.tasks.iter().find(|t| t.id == 3).unwrap().target, "小柚");
    assert!(!v.path.join("回忆").exists(), "the emptied folder goes");
    assert_eq!(compared(&j, &v, moved.tasks.iter().find(|t| t.id == 3).unwrap()).same, 2);
    // another drive's card isn't done here
    let other = agent::Card { drive: Some("OTHER".into()), ..mv };
    assert!(cards::run(&j, &moved, &other, &[0]).unwrap_err().contains("另一块"));
}

#[test]
fn the_agent_looks_at_the_backup_and_the_mock_proposes_what_the_clues_say() {
    let (_d, j, _v) = sandbox("look");
    let s = settled(&j, &with_tasks(vec![task(1, "资料", "资料"), task(2, "照片", "照片"), task(3, "小柚", "回忆/小柚")]));
    assert_eq!(targets(&s), [(3, "小柚".to_string(), "小柚".to_string()), (2, "照片".into(), "照片".into()), (1, "资料".into(), "资料".into())]);
    compare_all(&j, &s);
    let look = |args: &str| look::tool_with(&j, &s, args, 8000);
    let a = look(r#"{"view":"overview"}"#);
    assert!(a.text.contains("#2 ~/Downloads/照片：一样 3 · 新加 2 · 改名或挪了 2\n") && a.text.contains("可以：在硬盘上改名 回忆/小柚 → 小柚") && a.text.contains("没勾的（不备份）：作品"), "{}", a.text);
    assert!(a.text.contains("勾了、只在硬盘上的（归档）：回忆、回忆_2606、影音") && a.text.contains("都是归档") && !a.text.contains("只在硬盘上 1"), "{}", a.text);
    let a = look(r#"{"view":"diff","task":2,"group":"moved"}"#);
    assert!(a.text.contains("其它/IMG_0101.jpg → 其他/IMG_0101.jpg"), "{}", a.text);
    let a = look(r#"{"view":"drive","path":""}"#);
    assert!(a.text.contains("回忆_2606/") && a.text.contains("影音/") && !a.text.to_lowercase().contains("vault"), "{}", a.text);
    let a = look(r#"{"view":"compare","mac":"~/Downloads/作品","drive":"资料"}"#);
    assert!(a.state == "ok" && a.text.contains("新加 4"), "{}", a.text);
    assert_eq!(look(r#"{"view":"drive","path":"VAULT"}"#).state, "failed");
    assert_eq!(look(r#"{"view":"compare","mac":"~/Downloads/VAULT.sparsebundle","drive":"资料"}"#).state, "failed");
    // the mock Agent on the automatic look: what the clues say, each a card that checks out
    let system = look::situation(&j, &s);
    let calls = agent::mock_calls(&look::question(fake::DRIVE), &system);
    assert_eq!(calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), [look::BACKUP, cards::MOVE]);
    for c in &calls[1..] {
        let card = cards::build(&j, &s, &c.name, &c.arguments).unwrap_or_else(|e| panic!("{}: {e}", c.arguments));
        assert!(!card.items.is_empty());
    }
    // what a look is about: the same clues the same, none nothing
    let sn = fresh_snapshot(&j, &s).unwrap();
    let fp = sn.hints.as_ref().unwrap().fingerprint(&[(3, 1)]);
    assert!(!fp.is_empty() && fp == sn.hints.as_ref().unwrap().fingerprint(&[(3, 1)]) && fp != sn.hints.as_ref().unwrap().fingerprint(&[]));
    assert!(hints::Hints::default().fingerprint(&[]).is_empty());
    // the cards' results go back to the model
    let mut done = cards::build(&j, &s, cards::MOVE, &serde_json::json!({ "items": [{ "from": "照片/其它", "to": "照片/其他", "why": "x" }] }).to_string()).unwrap();
    done.state = "done".into();
    done.items[0].done = true;
    let task_card = agent::Card { kind: "task".into(), title: "新任务".into(), state: "failed".into(), error: Some("电脑上没有这个文件夹了".into()), task: Some(cards::TaskPlan { source: "/Users/me/Downloads/作品".into(), target: "作品".into(), ..Default::default() }), ..Default::default() };
    let h = agent::in_history(&[done, task_card]);
    assert!(h.contains("在硬盘上改名或挪「在硬盘上跟着改」：改了 1 项") && h.contains("新任务 /Users/me/Downloads/作品 → 作品：电脑上没有这个文件夹了"), "{h}");
}

#[test]
fn the_page_reads_the_last_look_from_its_reply() {
    use crate::youzi::chat;
    let c = crate::youzi::store::tests::memory_db();
    assert!(look::view(&c, None).unwrap().is_none(), "never looked");
    let session = agent::open_chat(&c, &Default::default()).unwrap();
    let b = chat::begin_run_in(&c, Some(session), &Default::default(), &look::question(fake::DRIVE), "mock", "模拟模型", &Default::default()).unwrap();
    crate::youzi::store::pref_set(&c, look::PREF, &look::LookState { fingerprint: "a".into(), reply: Some(b.reply_id), at: 100, error: None }).unwrap();
    agent::save_card(&c, b.run_id, &agent::Card { kind: "drive_trash".into(), title: "硬盘里重复的".into(), ..Default::default() }).unwrap();
    let v = look::view(&c, Some("a")).unwrap().unwrap();
    assert_eq!((v.state.as_str(), v.cards.len(), v.stale), ("running", 1, false));
    c.execute("UPDATE messages SET content = '看完了', state = 'complete' WHERE id = ?1", [b.reply_id]).unwrap();
    let v = look::view(&c, Some("b")).unwrap().unwrap();
    assert_eq!((v.state.as_str(), v.text.as_str(), v.stale), ("done", "看完了", true));
    // a failed reply says why (its run's error)
    c.execute("UPDATE messages SET state = 'failed' WHERE id = ?1", [b.reply_id]).unwrap();
    c.execute("UPDATE runs SET state = 'failed', error = '模型没回' WHERE id = ?1", [b.run_id]).unwrap();
    let v = look::view(&c, None).unwrap().unwrap();
    assert_eq!((v.state.as_str(), v.error.as_deref()), ("failed", Some("模型没回")));
    // a look that couldn't start; a reply gone with an old conversation
    crate::youzi::store::pref_set(&c, look::PREF, &look::LookState { fingerprint: "a".into(), reply: None, at: 100, error: Some("没有模型".into()) }).unwrap();
    assert_eq!(look::view(&c, None).unwrap().unwrap().error.as_deref(), Some("没有模型"));
    crate::youzi::store::pref_set(&c, look::PREF, &look::LookState { fingerprint: "a".into(), reply: Some(99_999), at: 100, error: None }).unwrap();
    assert!(look::view(&c, None).unwrap().is_none());
}

// ---- Phase 3: PikPak (rclone into a local folder standing in for it) ----

fn with_pikpak(limit: u32) -> Settings {
    Settings { pikpak: pikpak::PikPak { account: Some("xiaoyou@example.com".into()), folder: String::new(), limit }, ..with_tasks(vec![]) }
}

fn cloud_files(j: &Janitor, folder: &str) -> Vec<String> {
    let l = diff::list(&rclone::fake_root(j).join(folder), &Skip { mirror: true }, &AtomicBool::new(false)).unwrap();
    l.files.keys().cloned().collect()
}

fn has_rclone() -> bool {
    if rclone::binary().is_none() {
        eprintln!("rclone isn't installed (brew install rclone): the PikPak tests didn't run");
        return false;
    }
    true
}

#[test]
fn pikpak_logs_in_through_an_encrypted_config() {
    if !has_rclone() {
        return;
    }
    let (_d, j, _v) = sandbox("pikpak-login");
    let rc = rclone::Rclone::new(&j, None).unwrap();
    let (used, total) = rc.login("xiaoyou@example.com", "not-a-real-password").unwrap();
    assert_eq!((used, total), (0, 6_000 * rules::GB));
    let conf = std::fs::read_to_string(j.backup.data.join("rclone/rclone.conf")).unwrap();
    assert!(conf.contains("RCLONE_ENCRYPT_V0") && !conf.contains("pikpak") && !conf.contains("type"), "{conf}");
    rc.logout();
    assert!(!j.backup.data.join("rclone/rclone.conf").exists());
    assert!(rc.about().is_ok(), "the fake cloud's size needs no login");
    assert!(rclone::folder_of("../x").is_err() && rclone::folder_of("备份/VAULT").is_err() && rclone::folder_of(" ").is_err() && rclone::folder_of(".x").is_err());
    assert_eq!(rclone::folder_of("/备份/小柚的硬盘/").unwrap(), "备份/小柚的硬盘");
}

#[test]
fn the_cloud_compares_like_a_sync_goes() {
    let m = |v: &[(&str, u64)]| v.iter().map(|(k, s)| (k.to_string(), *s)).collect::<std::collections::BTreeMap<String, u64>>();
    let drive = m(&[("A/x", 1), ("A/y", 2), ("A/sub/z", 3), ("A/新", 9), ("B/w", 4), ("B/moved.txt", 8), ("C/q", 7)]);
    let cloud = m(&[("A/x", 1), ("A/y", 5), ("A/old/z", 3), ("A/gone", 6), ("D/moved.txt", 8), ("C/q", 7), ("E/e", 1)]);
    let per = pikpak::cloud_diff(&drive, &cloud, &["A".into(), "B".into()]);
    assert_eq!(per.keys().collect::<Vec<_>>(), ["A", "B"], "only the folders asked for");
    let a = &per["A"];
    assert_eq!((a.same, a.same_bytes), (1, 1));
    assert_eq!(a.changed, [pikpak::Item { path: "y".into(), size: 2 }]);
    assert_eq!(a.moved, [diff::Move { from: "old/z".into(), to: "sub/z".into(), size: 3 }]);
    assert_eq!(a.new, [pikpak::Item { path: "新".into(), size: 9 }]);
    assert_eq!(a.extra, [pikpak::Item { path: "gone".into(), size: 6 }]);
    // D isn't asked for: its file stays there, B's is uploaded
    assert_eq!((per["B"].new.len(), per["B"].moved.len(), per["B"].extra.len()), (2, 0, 0));
}

#[test]
fn pikpak_compares_then_mirrors_the_ticked_folders() {
    if !has_rclone() {
        return;
    }
    let (_d, j, v) = sandbox("pikpak-sync");
    let tops = ["回忆", "回忆_2606", "影音", "照片", "资料"];
    let s = Settings { picked: Some(tops.map(String::from).to_vec()), ..with_pikpak(0) };
    rclone::Rclone::new(&j, None).unwrap().login("xiaoyou@example.com", "x").unwrap();
    std::fs::create_dir_all(v.path.join(".Trashes/501")).unwrap();
    std::fs::write(v.path.join(".Trashes/501/扔掉的.txt"), "x").unwrap();
    std::fs::write(v.path.join("照片/.DS_Store"), "x").unwrap();
    let on_drive: Vec<String> = diff::list(&v.path, &Skip { mirror: true }, &AtomicBool::new(false)).unwrap().files.into_keys().filter(|k| k.contains('/')).collect();
    // before: the folder isn't on PikPak yet; everything is new
    let sn = pikpak::compare_for_test(&j, &s);
    assert!(sn.missing() && sn.error().is_none(), "{sn:?}");
    assert_eq!(sn.per().values().map(|d| d.new.len()).sum::<usize>(), on_drive.len());
    // the ticked folders but VAULT and the junk, names in NFC; ARCHIVE up there never read
    let (p, d) = pikpak::sync_for_test(&j, &s, &tops, 100);
    assert!(d.ok && d.errors.is_empty(), "{d:?}");
    let up = cloud_files(&j, fake::DRIVE);
    assert_eq!(up, on_drive, "a mirror of the ticked folders");
    assert!(up.iter().all(|k| !k.to_lowercase().contains("vault") && !k.contains(".Trashes") && !k.contains("DS_Store") && !k.contains(".pomelo")), "{up:?}");
    let raw: Vec<String> = std::fs::read_dir(rclone::fake_root(&j).join(fake::DRIVE).join("照片")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(raw.contains(&"Caf\u{e9}.jpg".to_string()), "uploaded in NFC: {raw:?}");
    assert_eq!((p.files.load(Ordering::Relaxed), p.total_files.load(Ordering::Relaxed)), (up.len() as u64, up.len() as u64));
    assert_eq!(pikpak::ended_text(&p, &d), format!("同步好了：传了 {} 个（{}）", up.len(), gb(p.bytes.load(Ordering::Relaxed))));
    let cloud = rclone::fake_root(&j).join(fake::DRIVE);
    std::fs::create_dir_all(cloud.join("ARCHIVE/以前的")).unwrap();
    std::fs::write(cloud.join("ARCHIVE/以前的/b.txt"), "x").unwrap();
    // compared again: all the same; a sync moves nothing
    let sn = pikpak::compare_for_test(&j, &s);
    assert!(!sn.missing() && sn.per().values().all(|d| d.new.is_empty() && d.changed.is_empty() && d.moved.is_empty() && d.extra.is_empty()), "{sn:?}");
    assert_eq!(sn.per()["照片"].same, 6);
    let (p, d) = pikpak::sync_for_test(&j, &s, &tops, 100);
    assert!(d.ok && p.files.load(Ordering::Relaxed) == 0);
    assert_eq!(pikpak::ended_text(&p, &d), "和硬盘上一样");
    // a folder renamed on the drive, a changed file, one gone, one only up there: compared, then done as said
    std::fs::rename(v.path.join("照片/其它"), v.path.join("照片/其他")).unwrap();
    std::fs::write(v.path.join("资料/2609 实习/日程.xlsx"), vec![1u8; 17_000]).unwrap();
    std::fs::remove_file(v.path.join("影音/电影/片.mkv")).unwrap();
    std::fs::write(cloud.join("资料/手传的.txt"), "hand").unwrap();
    let sn = pikpak::compare_for_test(&j, &s);
    let per = sn.per();
    assert_eq!(per["照片"].moved.iter().map(|m| (m.from.as_str(), m.to.as_str())).collect::<Vec<_>>(), [("其它/IMG_0101.jpg", "其他/IMG_0101.jpg"), ("其它/IMG_0102.jpg", "其他/IMG_0102.jpg")]);
    assert_eq!(per["资料"].changed.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["2609 实习/日程.xlsx"]);
    assert_eq!(per["资料"].extra.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["手传的.txt"]);
    assert_eq!(per["影音"].extra.iter().map(|x| x.path.as_str()).collect::<Vec<_>>(), ["电影/片.mkv"]);
    let (p, d) = pikpak::sync_for_test(&j, &s, &["照片", "资料", "影音"], 100);
    assert!(d.ok, "{d:?}");
    assert_eq!((p.files.load(Ordering::Relaxed), p.deletes.load(Ordering::Relaxed)), (1, 2), "{}", pikpak::ended_text(&p, &d));
    assert_eq!(pikpak::ended_text(&p, &d), "同步好了：传了 1 个（17 KB），在 PikPak 上挪了 2 个，挪进 ARCHIVE 2 个");
    let up = cloud_files(&j, fake::DRIVE);
    assert!(up.contains(&"照片/其他/IMG_0101.jpg".to_string()) && !up.iter().any(|k| k.starts_with("照片/其它")), "{up:?}");
    let archived = |rel: &str| cloud.join("ARCHIVE/2026-11-25").join(rel);
    assert!(archived("影音/电影/片.mkv").exists() && archived("资料/手传的.txt").exists(), "into ARCHIVE, never deleted");
    assert_eq!(std::fs::metadata(archived("资料/2609 实习/日程.xlsx")).unwrap().len(), 16_000, "the old copy kept");
    assert_eq!(std::fs::metadata(cloud.join("资料/2609 实习/日程.xlsx")).unwrap().len(), 17_000);
    assert!(cloud.join("ARCHIVE/以前的/b.txt").exists());
    let sn = pikpak::compare_for_test(&j, &s);
    assert!(sn.per().values().all(|d| d.new.is_empty() && d.changed.is_empty() && d.moved.is_empty() && d.extra.is_empty()), "{sn:?}");
    // the records: its folders, what moved; an empty sync leaves none
    let c = crate::youzi::store::tests::memory_db();
    let done = ["照片".to_string(), "资料".into(), "影音".into()];
    let id = pikpak::record_start(&c, fake::DRIVE, &done).unwrap();
    pikpak::record_end(&c, id, &done, &p, &d).unwrap();
    let r = &runs(&c, 10).unwrap()[0];
    assert_eq!((r.dest.as_str(), r.status.as_str(), r.tasks.iter().map(|t| (t.target.as_str(), t.done)).collect::<Vec<_>>()), ("pikpak", "done", vec![("照片", true), ("资料", true), ("影音", true)]));
    assert!(r.note.as_deref().unwrap_or("").contains("挪进 PikPak 的 ARCHIVE 2 个"), "{:?}", r.note);
    let v2 = pikpak::view(&j, &s, &runs(&c, 10).unwrap());
    assert!(v2.folders.iter().find(|f| f.name == "照片").unwrap().last.is_some() && v2.folders.iter().find(|f| f.name == "回忆").unwrap().last.is_none());
    let id = pikpak::record_start(&c, fake::DRIVE, &done).unwrap();
    pikpak::record_end(&c, id, &done, &rclone::Progress::new(), &rclone::Done { ok: true, ..Default::default() }).unwrap();
    assert_eq!(runs(&c, 10).unwrap().len(), 1);
    // a run left running: each kind settles on its own
    let id = pikpak::record_start(&c, fake::DRIVE, &done).unwrap();
    settle_stale(&c, "drive").unwrap();
    assert_eq!(runs(&c, 10).unwrap()[0].status, "running");
    settle_stale(&c, "pikpak").unwrap();
    assert_eq!(runs(&c, 10).unwrap().iter().find(|r| r.id == id).unwrap().status, "stopped");
}

#[test]
fn pikpak_holds_back_more_moves_than_compared_and_stops_without_half_files() {
    if !has_rclone() {
        return;
    }
    let (_d, j, v) = sandbox("pikpak-guard");
    rclone::Rclone::new(&j, None).unwrap().login("xiaoyou@example.com", "x").unwrap();
    let many = v.path.join("影音/零碎");
    std::fs::create_dir_all(&many).unwrap();
    for i in 0..(rclone::MAX_DELETE + 20) {
        std::fs::write(many.join(format!("{i}.txt")), format!("{i}")).unwrap();
    }
    let s = Settings { picked: Some(vec!["影音".into()]), ..with_pikpak(0) };
    assert!(pikpak::sync_for_test(&j, &s, &["影音"], 100).1.ok);
    // the drive suddenly misses a lot: more than allowed stops it
    std::fs::remove_dir_all(&many).unwrap();
    let (p, d) = pikpak::sync_for_test(&j, &s, &["影音"], rclone::MAX_DELETE as u64);
    assert!(!d.ok && d.too_many_deletes, "{d:?}");
    assert!(pikpak::ended_text(&p, &d).starts_with("要挪进 ARCHIVE 的比对出来的多"));
    assert!(cloud_files(&j, fake::DRIVE).iter().any(|k| k.starts_with("影音/零碎/")));
    // the first ones went into ARCHIVE before it stopped; compared, the rest count: it goes through
    assert_eq!(pikpak::compare_for_test(&j, &s).per()["影音"].extra.len(), 20);
    assert_eq!(std::fs::read_dir(rclone::fake_root(&j).join(fake::DRIVE).join("ARCHIVE/2026-11-25/影音/零碎")).unwrap().count(), rclone::MAX_DELETE as usize);
    let (_, d) = pikpak::sync_for_test(&j, &s, &["影音"], 20 + rclone::MAX_DELETE as u64);
    assert!(d.ok, "{d:?}");
    assert!(!cloud_files(&j, fake::DRIVE).iter().any(|k| k.starts_with("影音/零碎/")));
    // stopped halfway (a slow line): no half file up there; the next sync carries on
    let big = v.path.join("影音/长片.mov");
    std::fs::write(&big, vec![7u8; 3_000_000]).unwrap();
    let drive = drive_of(&j, &s).unwrap();
    let p = std::sync::Arc::new(rclone::Progress::new());
    let stopper = {
        let p = p.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            p.kill();
        })
    };
    let d = rclone::Rclone::new(&j, None).unwrap().sync(&drive.path, fake::DRIVE, &["影音".into()], 1, 100, "2026-11-25", &p);
    stopper.join().unwrap();
    assert!(d.stopped && !d.ok, "{d:?}");
    let up = cloud_files(&j, fake::DRIVE);
    assert!(!up.contains(&"影音/长片.mov".to_string()) && up.iter().all(|k| !k.contains("partial")), "{up:?}");
    let (p, d) = pikpak::sync_for_test(&j, &s, &["影音"], 100);
    assert!(d.ok && p.files.load(Ordering::Relaxed) == 1, "{d:?}");
}

#[test]
fn rclone_lines_are_read_into_progress() {
    let p = rclone::Progress::new();
    let mut d = rclone::Done::default();
    p.read_line(r#"{"level":"notice","msg":"x","stats":{"bytes":0,"totalBytes":0,"transfers":0,"totalTransfers":0,"checks":40,"totalChecks":120}}"#, &mut d);
    assert!(p.checking.load(Ordering::Relaxed), "still comparing");
    p.read_line(r#"{"level":"notice","msg":"x","stats":{"bytes":500,"totalBytes":2000,"transfers":1,"totalTransfers":4,"deletes":2,"renames":1,"speed":250.5,"eta":6,"transferring":[{"name":"照片/a.jpg"}]}}"#, &mut d);
    p.read_line(r#"{"level":"error","msg":"Failed to copy: permission denied","object":"资料/x.pdf"}"#, &mut d);
    p.read_line(r#"{"level":"error","msg":"Failed to copy: denied","object":"VAULT/秘密.txt"}"#, &mut d);
    p.read_line(r#"{"level":"error","msg":"Cancelling sync due to fatal error: failed to delete 1 files"}"#, &mut d);
    p.read_line(r#"{"level":"error","msg":"Got fatal error on delete: --max-delete threshold reached","object":"a.txt"}"#, &mut d);
    p.read_line("not json", &mut d);
    assert!(!p.checking.load(Ordering::Relaxed));
    assert_eq!((p.bytes.load(Ordering::Relaxed), p.total_files.load(Ordering::Relaxed), p.speed.load(Ordering::Relaxed), p.eta.load(Ordering::Relaxed)), (500, 4, 250, 6));
    assert_eq!(*p.current.lock().unwrap(), ["照片/a.jpg"]);
    assert!(d.too_many_deletes);
    assert_eq!(d.errors.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(), ["资料/x.pdf", "a.txt"], "VAULT never named, summaries left out");
    let f = rclone::filters();
    for rule in ["- vault/**", "- vault.*", "- .Trashes/**", "- .DS_Store", "- ._*", "- .pomelo-part-*"] {
        assert!(f.iter().any(|x| x == rule), "{rule}: {f:?}");
    }
    assert!(f.contains(&"--ignore-case".to_string()));
    // PikPak names as typed: no full-width stand-ins for : ? * …
    for flag in ["Colon", "Question", "Asterisk", "Pipe", "LtGt", "DoubleQuote", "BackSlash"] {
        assert!(!rclone::PIKPAK_ENCODING.contains(flag), "{flag}");
    }
}
