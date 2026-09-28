//! Data directory layout. Debug builds (`tauri dev`, `cargo test`) use a separate
//! "Pomelo OS Dev" folder so development never touches real data.
//! `POMELO_DATA_DIR` overrides the location (used for measurements / scratch runs).

use std::path::{Path, PathBuf};

pub const DATA_DIR_ENV: &str = "POMELO_DATA_DIR";

pub fn is_dev() -> bool {
    cfg!(debug_assertions)
}

pub fn data_dir_name() -> &'static str {
    if is_dev() {
        "Pomelo OS Dev"
    } else {
        "Pomelo OS"
    }
}

/// `base` = ~/Library/Application Support.
pub fn resolve_root(base: &Path, env_override: Option<PathBuf>) -> PathBuf {
    env_override.unwrap_or_else(|| base.join(data_dir_name()))
}

#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub backups: PathBuf,
}

impl DataPaths {
    pub fn new(root: PathBuf) -> Self {
        Self {
            db: root.join("pomelo.db"),
            backups: root.join("backups"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.backups)
            .map_err(|e| format!("无法创建数据目录 {}：{e}", self.root.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_builds_use_dev_dir() {
        assert_eq!(data_dir_name(), "Pomelo OS Dev");
        let base = Path::new("/base");
        assert_eq!(resolve_root(base, None), base.join("Pomelo OS Dev"));
        assert_eq!(resolve_root(base, Some("/x".into())), PathBuf::from("/x"));
    }

    #[test]
    fn layout() {
        let p = DataPaths::new("/r".into());
        assert_eq!(p.db, PathBuf::from("/r/pomelo.db"));
        assert_eq!(p.backups, PathBuf::from("/r/backups"));
    }
}
