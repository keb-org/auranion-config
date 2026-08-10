use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{integration::Integration, io::strip_bom};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct State {
    pub(super) active: Vec<Integration>,
    pub(super) baselines: BTreeMap<String, Baseline>,
    pub(super) desktop_config: Option<PathBuf>,
    pub(super) startup_artifact: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct Baseline {
    backup: PathBuf,
    existed: bool,
}

impl State {
    pub(super) fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("state.json");
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }

        serde_json::from_str(strip_bom(&text)).with_context(|| format!("parse {}", path.display()))
    }

    pub(super) fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join("state.json");
        fs::write(&path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub(super) fn backup(&mut self, data_dir: &Path, path: &Path) -> Result<()> {
        let key = path_key(path);
        if self.baselines.contains_key(&key) {
            return Ok(());
        }

        let existed = path.exists();
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let backup = data_dir
            .join("backups")
            .join(format!("{stamp}-{}", safe_name(path)));
        let parent = backup
            .parent()
            .context("backup path has no parent directory")?;
        fs::create_dir_all(parent)?;

        if existed {
            fs::copy(path, &backup).with_context(|| format!("backup {}", path.display()))?;
        } else {
            fs::write(&backup, b"")
                .with_context(|| format!("create backup placeholder for {}", path.display()))?;
        }

        self.baselines.insert(key, Baseline { backup, existed });
        Ok(())
    }

    pub(super) fn baseline_for(&self, path: &Path) -> Option<PathBuf> {
        self.baselines
            .get(&path_key(path))
            .map(|baseline| baseline.backup.clone())
    }

    pub(super) fn restore_file(&self, path: &Path) -> Result<()> {
        let Some(baseline) = self.baselines.get(&path_key(path)) else {
            return Ok(());
        };

        if baseline.existed {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&baseline.backup, path)
                .with_context(|| format!("restore {}", path.display()))?;
        } else if path.exists() {
            fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
        }
        Ok(())
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn safe_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .replace([':', '/', '\\'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-{name}-{}", std::process::id()))
    }

    #[test]
    fn backup_is_idempotent_and_restores_original() {
        let dir = test_dir("backup-state");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, "original").unwrap();
        let mut state = State::default();

        state.backup(&dir, &path).unwrap();
        let first = state.baseline_for(&path).unwrap();
        state.backup(&dir, &path).unwrap();
        assert_eq!(state.baseline_for(&path), Some(first.clone()));
        assert_eq!(fs::read_dir(dir.join("backups")).unwrap().count(), 1);

        fs::write(&path, "changed").unwrap();
        state.restore_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert!(first.exists());
        assert!(state.baseline_for(&path).is_some());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn state_accepts_legacy_string_paths() {
        let state: State = serde_json::from_str(
            r#"{
                "active": ["Codex"],
                "baselines": {
                    "C:\\example": { "backup": "C:\\backup", "existed": true }
                },
                "desktop_config": "C:\\desktop.json",
                "startup_artifact": null
            }"#,
        )
        .unwrap();

        assert_eq!(state.active, vec![Integration::Codex]);
        assert_eq!(
            state.desktop_config,
            Some(PathBuf::from("C:\\desktop.json"))
        );
    }
}
