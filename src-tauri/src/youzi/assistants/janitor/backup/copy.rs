//! Copying onto the drive. A file is written beside its place as `.pomelo-part-<name>` in 1 MB chunks (the
//! run can stop between two), then gets the original's extended attributes (Finder tags…), flags and times
//! (`fcopyfile`, then the times again explicitly), then is renamed into place — the drive never shows half
//! a file. Reading the bytes to copy them is the backup itself; nothing else looks inside.
//! An overwritten file first moves to `<drive>/旧版/<date>/<task folder>/<path>` (same volume: a rename).

use super::diff::PART;
use std::ffi::c_void;
use std::fs::{File, FileTimes};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::macos::fs::FileTimesExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Where overwritten files go, at the drive's root.
pub const OLD: &str = "旧版";

const CHUNK: usize = 1 << 20;
const COPYFILE_STAT: u32 = 1 << 1;
const COPYFILE_XATTR: u32 = 1 << 2;

extern "C" {
    fn fcopyfile(from: i32, to: i32, state: *mut c_void, flags: u32) -> i32;
    fn sync();
}

#[derive(Debug, PartialEq)]
pub enum Fail {
    Stopped,
    Error(String),
}

fn why(e: std::io::Error) -> Fail {
    Fail::Error(match e.kind() {
        std::io::ErrorKind::NotFound => "不在了".into(),
        std::io::ErrorKind::PermissionDenied => "没权限".into(),
        std::io::ErrorKind::StorageFull => "移动硬盘满了".into(),
        _ => e.to_string(),
    })
}

/// Copies `src` to `dst` (its folder must exist); `done` counts the bytes written. `ready` runs once the copy
/// is whole, right before it takes `dst`'s place (moving the old one into 旧版).
pub fn copy_file(src: &Path, dst: &Path, stop: &AtomicBool, done: &AtomicU64, ready: impl FnOnce() -> Result<(), String>) -> Result<(), Fail> {
    let name = dst.file_name().ok_or_else(|| Fail::Error("没有名字".into()))?.to_string_lossy();
    let part = dst.with_file_name(format!("{PART}{name}"));
    let r = (|| {
        let mut from = File::open(src).map_err(why)?;
        let meta = from.metadata().map_err(why)?;
        let mut to = File::create(&part).map_err(why)?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(Fail::Stopped);
            }
            let n = from.read(&mut buf).map_err(why)?;
            if n == 0 {
                break;
            }
            to.write_all(&buf[..n]).map_err(why)?;
            done.fetch_add(n as u64, Ordering::Relaxed);
        }
        // attributes (tags…), flags and times; the times again, plainly
        unsafe { fcopyfile(from.as_raw_fd(), to.as_raw_fd(), std::ptr::null_mut(), COPYFILE_STAT | COPYFILE_XATTR) };
        let mut t = FileTimes::new();
        if let Ok(m) = meta.modified() {
            t = t.set_modified(m);
        }
        if let Ok(a) = meta.accessed() {
            t = t.set_accessed(a);
        }
        if let Ok(c) = meta.created() {
            t = t.set_created(c);
        }
        to.set_times(t).map_err(why)?;
        drop(to);
        ready().map_err(Fail::Error)?;
        std::fs::rename(&part, dst).map_err(why)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r
}

/// `name (2).ext`, `name (3).ext`… for a taken path.
fn free_path(p: PathBuf) -> PathBuf {
    if std::fs::symlink_metadata(&p).is_err() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|i| p.with_file_name(format!("{stem} ({i}){ext}")))
        .find(|c| std::fs::symlink_metadata(c).is_err())
        .expect("a free name")
}

/// Moves the drive's `<target>/<rel>` into 旧版 (`day`: YYYY-MM-DD) before it's overwritten.
pub fn keep_old(drive: &Path, target: &str, rel: &str, day: &str) -> Result<PathBuf, String> {
    let from = drive.join(target).join(rel);
    let to = free_path(drive.join(OLD).join(day).join(target).join(rel));
    std::fs::create_dir_all(to.parent().ok_or("没有上一层")?).map_err(|e| e.to_string())?;
    std::fs::rename(&from, &to).map_err(|e| format!("挪不进旧版：{e}"))?;
    Ok(to)
}

/// Writes what's cached to the disks (end of a run).
pub fn flush() {
    unsafe { sync() };
}
