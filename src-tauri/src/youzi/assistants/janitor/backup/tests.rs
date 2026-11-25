use super::*;
use crate::db::tests::{temp_dir, TempDir};
use crate::youzi::assistants::janitor::{ensure_sandbox, fake};
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
    Task { id, source: format!("/Users/me/Downloads/{source}"), target: target.into(), excludes: Vec::new() }
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
    // 资料: one new, one newer on the Mac (the drive's goes to 旧版), two only on the drive; the Office lock file left out
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
    // the old 日程 is in 旧版, the new one in its place
    assert_eq!(std::fs::metadata(on_drive("旧版/2026-11-25/资料/2609 实习/日程.xlsx")).unwrap().len(), 16_000);
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

#[test]
fn tasks_are_checked() {
    let (_d, j, _v) = sandbox("tasks");
    let s = Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), ..Default::default() };
    let input = |source: &str, target: &str| TaskInput { id: None, source: source.into(), target: target.into(), excludes: vec![" *.tmp ".into(), "".into()] };
    let s = save_task(&j, s, input("/Users/me/Downloads/照片", "照片")).unwrap();
    assert_eq!((s.tasks[0].id, s.tasks[0].excludes.clone()), (1, vec!["*.tmp".to_string()]));
    // a real path from the folder dialog (inside the fake Mac, inside the drive) works too
    let real_src = j.real_of("/Users/me/Downloads/资料").to_string_lossy().into_owned();
    let real_dst = j.backup.volumes.dir.join(fake::DRIVE).join("资料").to_string_lossy().into_owned();
    let s = save_task(&j, s, input(&real_src, &real_dst)).unwrap();
    assert_eq!((s.tasks[1].source.as_str(), s.tasks[1].target.as_str()), ("/Users/me/Downloads/资料", "资料"));
    for (src, dst, why) in [
        ("/Users/me", "x", "个人文件夹里的一个"),
        ("/opt/homebrew", "x", "个人文件夹里的一个"),
        ("/Users/me/Downloads/VAULT.sparsebundle", "x", "不能备份"),
        ("/Users/me/Documents/LLM", "x", "受保护"),
        ("/Users/me/Downloads/没有", "x", "没有这个文件夹"),
        ("/Users/me/Downloads/作品", "旧版", "放旧版"),
        ("/Users/me/Downloads/作品", "VAULT", "不能备份"),
        ("/Users/me/Downloads/作品", "../x", "名字不对"),
        ("/Users/me/Downloads/作品", "/", "移动硬盘里的文件夹"),
        ("/Users/me/Downloads/作品", " ", "整块硬盘"),
        ("/Users/me/Downloads/作品", "照片/作品", "别的任务"),
    ] {
        let e = save_task(&j, s.clone(), input(src, dst)).unwrap_err();
        assert!(e.contains(why), "{src} → {dst}: {e}");
    }
}

#[test]
fn the_page_gets_counts_and_the_first_rows() {
    let (_d, j, _v) = sandbox("overview");
    let s = Settings { drive: Some(DriveId { uuid: fake::DRIVE_UUID.into(), name: fake::DRIVE.into() }), tasks: vec![task(1, "资料", "资料")], next_id: 1 };
    compare_all(&j, &s);
    let o = overview(&j, &s, Vec::new());
    let d = o.drive.as_ref().unwrap();
    assert!(d.connected && d.folders.contains(&"资料".to_string()) && !d.folders.iter().any(|f| f.contains("VAULT")));
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
    settle_stale(&c).unwrap();
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
    use crate::youzi::assistants::janitor::agent;
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
