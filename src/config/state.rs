use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    integration::Integration,
    io::{strip_bom, write_bytes},
};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct State {
    pub(super) active: Vec<Integration>,
    pub(super) baselines: BTreeMap<String, Baseline>,
    pub(super) generated: BTreeMap<String, PathBuf>,
    pub(super) desktop_config: Option<PathBuf>,
    pub(super) desktop_owned_meta: Option<PathBuf>,
    pub(super) desktop_consumer_config: Option<PathBuf>,
    pub(super) startup_artifact: Option<PathBuf>,
    pub(super) codex_home: Option<PathBuf>,
    codex_transaction: Option<CodexTransaction>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Baseline {
    backup: PathBuf,
    existed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CodexTransaction {
    #[serde(default)]
    completed: bool,
    #[serde(default)]
    rolled_back: bool,
    #[serde(default)]
    selected_codex_home: Option<PathBuf>,
    #[serde(default)]
    selected_codex_integrations: Vec<Integration>,
    active: Vec<Integration>,
    codex_home: Option<PathBuf>,
    #[serde(default)]
    baselines: BTreeMap<String, Baseline>,
    #[serde(default)]
    generated: BTreeMap<String, PathBuf>,
    files: Vec<TransactionFile>,
    #[serde(default)]
    created: Vec<PathBuf>,
    #[serde(default)]
    removed: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TransactionFile {
    path: PathBuf,
    before: PathBuf,
    expected: Vec<ExpectedFile>,
    existed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum ExpectedFile {
    Present(PathBuf),
    Absent,
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
        write_bytes(&path, &serde_json::to_vec_pretty(self)?)
    }

    pub(super) fn begin_codex_transaction(
        &mut self,
        data_dir: &Path,
        paths: &[&Path],
    ) -> Result<()> {
        if self.codex_transaction.is_some() {
            bail!("Codex transaction is already active");
        }
        let active = self.active.clone();
        let codex_home = self.codex_home.clone();
        let baselines = self.baselines.clone();
        let generated = self.generated.clone();
        let mut files = Vec::with_capacity(paths.len());
        let mut seen = BTreeSet::new();
        for path in paths {
            if !seen.insert(path_key(path)) {
                continue;
            }
            let existed = path.exists();
            let stamp = transaction_stamp()?;
            let before = data_dir
                .join("transactions")
                .join(format!("{stamp}-before-{}", safe_name(path)));
            let parent = before
                .parent()
                .context("transaction snapshot path has no parent directory")?;
            fs::create_dir_all(parent)?;
            if existed {
                fs::copy(path, &before).with_context(|| format!("snapshot {}", path.display()))?;
            } else {
                write_bytes(&before, b"")?;
            }
            files.push(TransactionFile {
                path: (*path).to_path_buf(),
                before,
                expected: Vec::new(),
                existed,
            });
        }
        self.codex_transaction = Some(CodexTransaction {
            completed: false,
            rolled_back: false,
            selected_codex_home: None,
            selected_codex_integrations: Vec::new(),
            active,
            codex_home,
            baselines,
            generated,
            files,
            created: Vec::new(),
            removed: Vec::new(),
        });
        Ok(())
    }

    pub(super) fn record_codex_expected(
        &mut self,
        data_dir: &Path,
        path: &Path,
        contents: &[u8],
    ) -> Result<()> {
        let Some(transaction) = self.codex_transaction.as_ref() else {
            bail!("Codex transaction is not active");
        };
        if !transaction.files.iter().any(|file| file.path == path) {
            bail!(
                "{} is not part of the active Codex transaction",
                path.display()
            );
        }

        let stamp = transaction_stamp()?;
        let expected = data_dir
            .join("transactions")
            .join(format!("{stamp}-expected-{}", safe_name(path)));
        write_bytes(&expected, contents)?;
        let transaction = self
            .codex_transaction
            .as_mut()
            .expect("active Codex transaction");
        let file = transaction
            .files
            .iter_mut()
            .find(|file| file.path == path)
            .expect("transaction path validated");
        transaction.created.push(expected.clone());
        file.expected.push(ExpectedFile::Present(expected));
        Ok(())
    }

    pub(super) fn record_codex_absent(&mut self, path: &Path) -> Result<()> {
        let Some(transaction) = self.codex_transaction.as_mut() else {
            bail!("Codex transaction is not active");
        };
        let Some(file) = transaction.files.iter_mut().find(|file| file.path == path) else {
            bail!(
                "{} is not part of the active Codex transaction",
                path.display()
            );
        };
        file.expected.push(ExpectedFile::Absent);
        Ok(())
    }

    pub(super) fn set_codex_transaction_selection(
        &mut self,
        home: Option<PathBuf>,
        integrations: &[Integration],
    ) {
        if let Some(transaction) = self.codex_transaction.as_mut() {
            transaction.selected_codex_home = home;
            transaction.selected_codex_integrations = integrations
                .iter()
                .copied()
                .filter(|integration| integration.is_codex())
                .collect();
        }
    }

    pub(super) fn finish_codex_transaction(&mut self) {
        if let Some(transaction) = self.codex_transaction.as_mut() {
            transaction.completed = true;
        }
    }

    pub(super) fn complete_codex_transaction(&mut self) -> Result<()> {
        let Some(transaction) = self.codex_transaction.as_ref() else {
            return Ok(());
        };
        if !transaction.completed {
            return Ok(());
        }
        cleanup_codex_transaction(transaction)?;
        self.codex_transaction = None;
        Ok(())
    }

    pub(super) fn recover_codex_transaction(&mut self) -> Result<bool> {
        let Some(transaction) = self.codex_transaction.clone() else {
            return Ok(false);
        };
        if transaction.completed {
            return Ok(true);
        }

        let preserve_selection = preserves_selected_output(&transaction, &self.generated);
        if preserve_selection {
            self.active = transaction.active.clone();
            if transaction.selected_codex_integrations.is_empty() {
                self.active.push(Integration::CodexCli);
            } else {
                for integration in &transaction.selected_codex_integrations {
                    if !self.active.contains(integration) {
                        self.active.push(*integration);
                    }
                }
            }
            self.codex_home = transaction.selected_codex_home.clone();
        } else {
            for file in &transaction.files {
                if file_matches_expected(file) {
                    restore_transaction_file(file)?;
                }
            }
            self.active = transaction.active.clone();
            self.codex_home = transaction.codex_home.clone();
            self.baselines = transaction.baselines.clone();
            self.generated = transaction.generated.clone();
        }
        if let Some(transaction) = self.codex_transaction.as_mut() {
            transaction.completed = true;
            transaction.rolled_back = !preserve_selection;
        }
        Ok(true)
    }

    pub(super) fn backup(&mut self, data_dir: &Path, path: &Path) -> Result<()> {
        let key = path_key(path);
        if self.baselines.contains_key(&key) {
            return Ok(());
        }

        let existed = path.exists();
        let stamp = transaction_stamp()?;
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
            write_bytes(&backup, b"")?;
        }

        if let Some(transaction) = self.codex_transaction.as_mut() {
            transaction.created.push(backup.clone());
        }
        self.baselines.insert(key, Baseline { backup, existed });
        Ok(())
    }

    pub(super) fn baseline_for(&self, path: &Path) -> Option<PathBuf> {
        self.baselines
            .get(&path_key(path))
            .map(|baseline| baseline.backup.clone())
    }

    pub(super) fn baseline_existed(&self, path: &Path) -> Option<bool> {
        self.baselines
            .get(&path_key(path))
            .map(|baseline| baseline.existed)
    }

    pub(super) fn forget_baseline(&mut self, path: &Path) {
        if let Some(baseline) = self.baselines.remove(&path_key(path)) {
            self.remove_snapshot(baseline.backup);
        }
    }

    #[cfg(test)]
    pub(super) fn record_generated(&mut self, data_dir: &Path, path: &Path) -> Result<()> {
        let contents = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        self.record_generated_bytes(data_dir, path, &contents)
    }

    pub(super) fn record_generated_bytes(
        &mut self,
        data_dir: &Path,
        path: &Path,
        contents: &[u8],
    ) -> Result<()> {
        let key = path_key(path);
        let stamp = transaction_stamp()?;
        let snapshot = data_dir
            .join("generated")
            .join(format!("{stamp}-{}", safe_name(path)));
        let parent = snapshot
            .parent()
            .context("generated snapshot path has no parent directory")?;
        fs::create_dir_all(parent)?;
        write_bytes(&snapshot, contents)?;
        let replaced = self.generated.insert(key, snapshot.clone());
        if let Some(replaced) = replaced {
            self.remove_snapshot(replaced);
        }
        if let Some(transaction) = self.codex_transaction.as_mut()
            && transaction.files.iter().any(|file| file.path == path)
        {
            transaction.created.push(snapshot);
        }
        Ok(())
    }

    pub(super) fn generated_for(&self, path: &Path) -> Option<PathBuf> {
        self.generated.get(&path_key(path)).cloned()
    }

    pub(super) fn forget_generated(&mut self, path: &Path) {
        if let Some(snapshot) = self.generated.remove(&path_key(path)) {
            self.remove_snapshot(snapshot);
        }
    }

    fn remove_snapshot(&mut self, snapshot: PathBuf) {
        if let Some(transaction) = self.codex_transaction.as_mut() {
            transaction.removed.push(snapshot);
        } else {
            let _ = fs::remove_file(snapshot);
        }
    }
}

fn preserves_selected_output(
    transaction: &CodexTransaction,
    generated: &BTreeMap<String, PathBuf>,
) -> bool {
    let Some(home) = &transaction.selected_codex_home else {
        return false;
    };
    let config = home.join("config.toml");
    let catalog = home.join("model-catalogs").join("auranion.json");
    let Some(config_snapshot) = generated.get(&path_key(&config)) else {
        return false;
    };
    let Some(catalog_snapshot) = generated.get(&path_key(&catalog)) else {
        return false;
    };

    let (Ok(current_catalog), Ok(generated_catalog)) =
        (fs::read(&catalog), fs::read(catalog_snapshot))
    else {
        return false;
    };
    if current_catalog != generated_catalog {
        return false;
    }

    let (Ok(current_config), Ok(generated_config)) = (fs::read(&config), fs::read(config_snapshot))
    else {
        return false;
    };
    if current_config == generated_config {
        return false;
    }

    if !transaction
        .selected_codex_integrations
        .contains(&Integration::CodexDesktop)
    {
        return true;
    }
    let providers = home.join("desktop-model-providers.json");
    let Some(providers_snapshot) = generated.get(&path_key(&providers)) else {
        return false;
    };
    fs::read(&providers).ok().as_deref() == fs::read(providers_snapshot).ok().as_deref()
}

fn cleanup_codex_transaction(transaction: &CodexTransaction) -> Result<()> {
    for file in &transaction.files {
        remove_snapshot(&file.before)?;
        for expected in &file.expected {
            if let ExpectedFile::Present(expected) = expected {
                remove_snapshot(expected)?;
            }
        }
    }
    let snapshots = if transaction.rolled_back {
        &transaction.created
    } else {
        &transaction.removed
    };
    for path in snapshots {
        remove_snapshot(path)?;
    }
    Ok(())
}

fn remove_snapshot(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn file_matches_expected(file: &TransactionFile) -> bool {
    file.expected.iter().any(|expected| match expected {
        ExpectedFile::Present(expected) => {
            file.path.exists()
                && fs::read(&file.path).ok().as_deref() == fs::read(expected).ok().as_deref()
        }
        ExpectedFile::Absent => !file.path.exists(),
    })
}

fn restore_transaction_file(file: &TransactionFile) -> Result<()> {
    if file.existed {
        let contents = fs::read(&file.before)
            .with_context(|| format!("read transaction snapshot {}", file.before.display()))?;
        write_bytes(&file.path, &contents)
            .with_context(|| format!("restore {}", file.path.display()))?;
    } else if file.path.exists() {
        fs::remove_file(&file.path).with_context(|| format!("remove {}", file.path.display()))?;
    }
    Ok(())
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn transaction_stamp() -> Result<u128> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(now + u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
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
    fn backup_is_idempotent_and_keeps_original_contents() {
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
        assert_eq!(fs::read_to_string(&first).unwrap(), "original");
        assert!(state.baseline_for(&path).is_some());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_transaction_restores_only_expected_output() {
        let dir = test_dir("transaction-recovery");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let catalog = dir.join("auranion.json");
        fs::write(&config, "original config").unwrap();
        fs::write(&catalog, "original catalog").unwrap();
        let mut state = State::default();
        state
            .begin_codex_transaction(&dir, &[&config, &catalog])
            .unwrap();
        state
            .record_codex_expected(&dir, &config, b"generated config")
            .unwrap();
        state
            .record_codex_expected(&dir, &catalog, b"generated catalog")
            .unwrap();
        state.save(&dir).unwrap();
        fs::write(&config, "generated config").unwrap();
        fs::write(&catalog, "user catalog edit").unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "original config");
        assert_eq!(fs::read_to_string(&catalog).unwrap(), "user catalog edit");
        recovered.save(&dir).unwrap();
        recovered.complete_codex_transaction().unwrap();
        assert!(
            !dir.join("transactions")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_deselect_preserves_user_edit_after_first_write() {
        let dir = test_dir("transaction-deselect-user-edit");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let catalog = dir.join("auranion.json");
        fs::write(&config, "original config").unwrap();
        fs::write(&catalog, "original catalog").unwrap();
        let mut state = State::default();
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &catalog).unwrap();
        fs::write(&config, "generated config").unwrap();
        fs::write(&catalog, "generated catalog").unwrap();
        state.record_generated(&dir, &config).unwrap();
        state.record_generated(&dir, &catalog).unwrap();
        state
            .begin_codex_transaction(&dir, &[&config, &catalog])
            .unwrap();
        state
            .record_codex_expected(&dir, &config, b"original config")
            .unwrap();
        state.record_codex_absent(&catalog).unwrap();
        state.forget_baseline(&config);
        state.forget_generated(&config);
        state.forget_baseline(&catalog);
        state.forget_generated(&catalog);
        state.save(&dir).unwrap();

        fs::write(&config, "original config").unwrap();
        fs::write(&config, "user config edit").unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "user config edit");
        assert_eq!(fs::read_to_string(&catalog).unwrap(), "generated catalog");
        assert!(recovered.baseline_for(&config).is_some());
        assert!(recovered.generated_for(&config).is_some());
        assert!(recovered.baseline_for(&catalog).is_some());
        assert!(recovered.generated_for(&catalog).is_some());
        recovered.save(&dir).unwrap();
        recovered.complete_codex_transaction().unwrap();
        recovered.save(&dir).unwrap();

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn completed_transaction_keeps_outputs_and_cleans_journal() {
        let dir = test_dir("completed-transaction");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        fs::write(&config, "original config").unwrap();
        let mut state = State::default();
        state.begin_codex_transaction(&dir, &[&config]).unwrap();
        state
            .record_codex_expected(&dir, &config, b"generated config")
            .unwrap();
        fs::write(&config, "generated config").unwrap();
        state.finish_codex_transaction();
        state.save(&dir).unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "generated config");
        recovered.save(&dir).unwrap();
        recovered.complete_codex_transaction().unwrap();
        recovered.save(&dir).unwrap();
        assert!(State::load(&dir).unwrap().codex_transaction.is_none());
        assert!(
            !dir.join("transactions")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_select_keeps_user_edited_complete_output() {
        let dir = test_dir("transaction-select-user-edit");
        let _ = fs::remove_dir_all(&dir);
        let home = dir.join(".codex");
        let config = home.join("config.toml");
        let catalog = home.join("model-catalogs").join("auranion.json");
        fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        fs::write(&config, "original config").unwrap();
        fs::write(&catalog, "original catalog").unwrap();
        let mut state = State::default();
        state
            .begin_codex_transaction(&dir, &[&config, &catalog])
            .unwrap();
        state.set_codex_transaction_selection(Some(home.clone()), &[Integration::CodexCli]);
        state.backup(&dir, &config).unwrap();
        state.backup(&dir, &catalog).unwrap();
        state
            .record_codex_expected(&dir, &config, b"generated config")
            .unwrap();
        state
            .record_codex_expected(&dir, &catalog, b"generated catalog")
            .unwrap();
        state
            .record_generated_bytes(&dir, &config, b"generated config")
            .unwrap();
        state
            .record_generated_bytes(&dir, &catalog, b"generated catalog")
            .unwrap();
        state.codex_home = Some(home.clone());
        state.save(&dir).unwrap();
        fs::write(&config, "user config edit").unwrap();
        fs::write(&catalog, "generated catalog").unwrap();

        let mut recovered = State::load(&dir).unwrap();
        assert!(recovered.recover_codex_transaction().unwrap());
        assert_eq!(fs::read_to_string(&config).unwrap(), "user config edit");
        assert_eq!(fs::read_to_string(&catalog).unwrap(), "generated catalog");
        assert_eq!(recovered.codex_home, Some(home));
        assert!(recovered.active.contains(&Integration::CodexCli));
        assert!(recovered.baseline_for(&config).is_some());
        assert!(recovered.generated_for(&config).is_some());
        assert!(recovered.baseline_for(&catalog).is_some());
        assert!(recovered.generated_for(&catalog).is_some());
        recovered.save(&dir).unwrap();
        recovered.complete_codex_transaction().unwrap();
        recovered.save(&dir).unwrap();

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn interrupted_first_apply_discards_new_baseline() {
        let dir = test_dir("transaction-first-apply");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let mut state = State::default();
        state.begin_codex_transaction(&dir, &[&config]).unwrap();
        state.backup(&dir, &config).unwrap();
        state
            .record_codex_expected(&dir, &config, b"generated config")
            .unwrap();
        state.save(&dir).unwrap();
        fs::write(&config, "generated config").unwrap();

        let mut recovered = State::load(&dir).unwrap();
        recovered.recover_codex_transaction().unwrap();
        assert!(!config.exists());
        assert!(recovered.baseline_for(&config).is_none());
        recovered.save(&dir).unwrap();
        recovered.complete_codex_transaction().unwrap();

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn state_persists_codex_home() {
        let dir = test_dir("codex-home-state");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let home = dir.join("custom-codex-home");
        let state = State {
            codex_home: Some(home.clone()),
            ..State::default()
        };

        state.save(&dir).unwrap();
        assert_eq!(State::load(&dir).unwrap().codex_home, Some(home));

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

        assert_eq!(state.active, vec![Integration::CodexCli]);
        assert_eq!(
            state.desktop_config,
            Some(PathBuf::from("C:\\desktop.json"))
        );
        assert_eq!(state.codex_home, None);
    }
}
