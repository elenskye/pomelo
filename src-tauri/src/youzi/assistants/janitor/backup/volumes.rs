//! The external drives connected now, and which one is mine (by its volume UUID, so a renamed drive is still
//! it). Real: the mounts in `/Volumes` on another device than `/`, each asked once with `diskutil info -plist`
//! (UUID, file system, internal or not); free space from `df` each time. Sandbox: the folders in
//! `<data>/janitor-volumes`, each saying what it is in `.pomelo-fake-volume.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

pub const FAKE_FILE: &str = ".pomelo-fake-volume.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub uuid: String,
    /// As Finder says it (Mac OS Extended (Journaled), APFS, ExFAT…).
    pub fs: String,
    pub total: u64,
    pub free: u64,
    #[serde(skip)]
    pub path: PathBuf,
}

/// What a fake volume says about itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FakeVolume {
    pub uuid: String,
    pub fs: String,
    pub total: u64,
}

#[derive(Debug, Clone)]
struct Known {
    dev: u64,
    uuid: String,
    fs: String,
    total: u64,
    internal: bool,
}

pub struct Volumes {
    /// `/Volumes`, or the sandbox's folder of fake ones.
    pub dir: PathBuf,
    pub fake: bool,
    known: Mutex<HashMap<String, Known>>,
}

fn plist_value<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
    let at = plist.find(&format!("<key>{key}</key>"))? + key.len() + 11;
    let rest = plist[at..].trim_start();
    if let Some(r) = rest.strip_prefix("<string>") {
        return r.split("</string>").next();
    }
    if let Some(r) = rest.strip_prefix("<integer>") {
        return r.split("</integer>").next();
    }
    rest.starts_with("<true/>").then_some("true").or(rest.starts_with("<false/>").then_some("false"))
}

/// (total, free) in bytes from `df -k -P`.
fn df(path: &Path) -> Option<(u64, u64)> {
    let out = Command::new("/bin/df").args(["-k", "-P"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let cols: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    Some((cols.get(1)?.parse::<u64>().ok()? * 1024, cols.get(3)?.parse::<u64>().ok()? * 1024))
}

/// Bytes of the files under `dir` (a fake volume's used space).
fn used(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match std::fs::symlink_metadata(e.path()) {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

impl Volumes {
    pub fn new(dir: PathBuf, fake: bool) -> Self {
        Self { dir, fake, known: Mutex::new(HashMap::new()) }
    }

    /// The names in the folder now (cheap: what the watcher polls).
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| !n.starts_with('.')).collect();
        v.sort();
        v
    }

    /// The external drives connected now.
    pub fn list(&self) -> Vec<Volume> {
        if self.fake {
            return self
                .names()
                .into_iter()
                .filter_map(|name| {
                    let path = self.dir.join(&name);
                    let f: FakeVolume = serde_json::from_str(&std::fs::read_to_string(path.join(FAKE_FILE)).ok()?).ok()?;
                    let free = f.total.saturating_sub(used(&path));
                    Some(Volume { name, uuid: f.uuid, fs: f.fs, total: f.total, free, path })
                })
                .collect();
        }
        let root_dev = std::fs::metadata("/").map(|m| m.dev()).unwrap_or(0);
        let mut out = Vec::new();
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        for name in self.names() {
            let path = self.dir.join(&name);
            let Ok(m) = std::fs::metadata(&path) else { continue };
            if m.dev() == root_dev {
                continue; // the startup disk's link
            }
            let k = match known.get(&name).filter(|k| k.dev == m.dev()) {
                Some(k) => k.clone(),
                None => {
                    let Ok(o) = Command::new("/usr/sbin/diskutil").args(["info", "-plist"]).arg(&path).output() else { continue };
                    let p = String::from_utf8_lossy(&o.stdout).into_owned();
                    let k = Known {
                        dev: m.dev(),
                        uuid: plist_value(&p, "VolumeUUID").unwrap_or_default().to_string(),
                        fs: plist_value(&p, "FilesystemUserVisibleName").or(plist_value(&p, "FilesystemName")).unwrap_or_default().to_string(),
                        total: plist_value(&p, "TotalSize").and_then(|s| s.parse().ok()).unwrap_or(0),
                        internal: plist_value(&p, "Internal") == Some("true"),
                    };
                    known.insert(name.clone(), k.clone());
                    k
                }
            };
            if k.internal || k.uuid.is_empty() {
                continue;
            }
            let (total, free) = df(&path).unwrap_or((k.total, 0));
            out.push(Volume { name, uuid: k.uuid, fs: k.fs, total: if k.total > 0 { k.total } else { total }, free, path });
        }
        out
    }

    pub fn find(&self, uuid: &str) -> Option<Volume> {
        self.list().into_iter().find(|v| v.uuid == uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_diskutil_plists() {
        let p = "<dict>\n\t<key>FilesystemUserVisibleName</key>\n\t<string>Mac OS Extended (Journaled)</string>\n\t<key>Internal</key>\n\t<false/>\n\t<key>TotalSize</key>\n\t<integer>2000228974592</integer>\n\t<key>VolumeUUID</key>\n\t<string>AB-CD</string>\n</dict>";
        assert_eq!(plist_value(p, "VolumeUUID"), Some("AB-CD"));
        assert_eq!(plist_value(p, "Internal"), Some("false"));
        assert_eq!(plist_value(p, "TotalSize"), Some("2000228974592"));
        assert_eq!(plist_value(p, "FilesystemUserVisibleName"), Some("Mac OS Extended (Journaled)"));
        assert_eq!(plist_value(p, "Nope"), None);
    }
}
