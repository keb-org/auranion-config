use anyhow::{Context, Result, bail, ensure};
use directories::BaseDirs;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use crate::schedule;

const REPO: &str = "keb-org/auranion-config";
const DISABLED: &str = "auto-update-disabled";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

pub(super) fn run(background: bool) -> Result<()> {
    let data = data_dir()?;
    // self_replace can move the running executable; retain its installed path.
    let executable = env::current_exe().context("Failed to locate installed binary")?;
    let Some(_lock) = try_lock(&data.join("update.lock"))? else {
        if !background {
            println!("Another Auranion update is already running.");
        }
        return Ok(());
    };
    if background && data.join(DISABLED).try_exists()? {
        return Ok(());
    }
    let result = update_and_reapply(
        &executable,
        |path| {
            let _config = config_lock()?;
            update_binary(path, background)
        },
        |path| reapply_saved_config(path, background),
    );
    // Enable scheduler after successful update for users upgrading from old versions.
    // Already holds update.lock; check disabled marker and install directly.
    if result.is_ok() && !data.join(DISABLED).try_exists()? {
        if let Err(error) = schedule::install(&executable) {
            if !background {
                eprintln!("Auto-update scheduler setup failed: {error:#}");
            }
        }
    }
    result
}

pub(crate) fn config_lock() -> Result<File> {
    try_lock(&data_dir()?.join("config.lock"))?
        .context("Another Auranion configuration or update is running; retry when it finishes")
}

pub(crate) fn ensure_schedule(force: bool) -> Result<()> {
    let data = data_dir()?;
    let _lock =
        try_lock(&data.join("update.lock"))?.context("Another update is running; retry later")?;
    let disabled = data.join(DISABLED);
    if !force && disabled.try_exists()? {
        return Ok(());
    }
    let executable = env::current_exe().context("Failed to locate installed binary")?;
    schedule::install(&executable)?;
    remove_if_exists(&disabled)?;
    println!("Daily auto-update enabled. Disable with `auranion schedule disable`.");
    Ok(())
}

pub(crate) fn disable_schedule() -> Result<()> {
    let data = data_dir()?;
    let _lock =
        try_lock(&data.join("update.lock"))?.context("Another update is running; retry later")?;
    // Keep opt-out even if OS cleanup fails or setup is run again.
    fs::write(data.join(DISABLED), b"")?;
    schedule::uninstall().context("Auto-update opted out, but OS task cleanup failed")?;
    println!("Daily auto-update disabled.");
    Ok(())
}

fn data_dir() -> Result<PathBuf> {
    let path = BaseDirs::new()
        .context("cannot determine user directories")?
        .data_local_dir()
        .join("auranion");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn try_lock(path: &Path) -> Result<Option<File>> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn update_and_reapply(
    executable: &Path,
    update: impl FnOnce(&Path) -> Result<()>,
    reapply: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    // Preserve manual-update behavior; a failed reapply is retried next day too.
    let update = update(executable);
    let reapply = reapply(executable);
    match (update, reapply) {
        (Err(update), Err(reapply)) => bail!("{update:#}; {reapply:#}"),
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn update_binary(executable: &Path, quiet: bool) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    if !quiet {
        println!("Checking for updates (current: v{current_version})...");
    }
    let agent = ureq::AgentBuilder::new().https_only(true).build();
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = agent
        .get(&url)
        .timeout(Duration::from_secs(30))
        .set("User-Agent", "auranion-cli")
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("Failed to check GitHub releases")?;
    let release: Release = serde_json::from_reader(response.into_reader().take(1024 * 1024))
        .context("Failed to parse release information")?;
    let latest = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    if !is_newer(current_version, latest)? {
        if !quiet {
            println!("Already up to date (v{current_version}).");
        }
        return Ok(());
    }
    let asset_name = target_asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| format!("Release v{latest} has no asset for target {asset_name}"))?;
    ensure!(
        asset
            .browser_download_url
            .starts_with(&format!("https://github.com/{REPO}/releases/download/")),
        "Release asset URL is outside the expected repository"
    );
    let expected_hash = sha256_digest(asset.digest.as_deref())?;
    ensure!(asset.size > 0, "Release asset is empty");
    let limit = asset
        .size
        .checked_add(1)
        .context("Release asset size is invalid")?;
    #[cfg(windows)]
    check_windows_helper_paths(executable)?;

    let directory = executable
        .parent()
        .context("Failed to get executable directory")?;
    let staging = tempfile::Builder::new()
        .prefix(".auranion-update-")
        .tempdir_in(directory)
        .context(
            "Install directory is not writable; reinstall Auranion in a user-owned directory",
        )?;
    let staged = staging.path().join(asset_name);
    if !quiet {
        println!("Downloading {} (v{latest})...", asset.name);
    }
    let response = agent
        .get(&asset.browser_download_url)
        .timeout(Duration::from_secs(300))
        .set("User-Agent", "auranion-cli")
        .call()
        .context("Failed to download binary")?;
    let mut file = File::create(&staged)?;
    let size = std::io::copy(&mut response.into_reader().take(limit), &mut file)
        .context("Failed to read downloaded binary")?;
    ensure!(size == asset.size, "Downloaded binary size mismatch");
    file.sync_all()?;
    drop(file);
    verify_hash(&staged, expected_hash)?;
    #[cfg(unix)]
    fs::set_permissions(&staged, fs::metadata(executable)?.permissions())?;
    let output = Command::new(&staged)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .context("Downloaded binary cannot run on this machine; installed binary unchanged")?;
    ensure!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == format!("auranion {latest}"),
        "Downloaded binary failed version check; installed binary unchanged"
    );
    replace_with_backup(executable, &staged, staging, |path| {
        self_replace::self_replace(path).context("Failed to replace current binary")
    })?;
    if !quiet {
        println!("Successfully updated to v{latest}!");
    }
    Ok(())
}

fn sha256_digest(digest: Option<&str>) -> Result<&str> {
    let hash = digest
        .and_then(|value| value.strip_prefix("sha256:"))
        .context("Release asset lacks a SHA-256 digest; refusing unverified update")?;
    ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid SHA-256 release digest"
    );
    Ok(hash)
}

fn verify_hash(path: &Path, expected: &str) -> Result<()> {
    let mut hash = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hash)?;
    ensure!(
        format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected),
        "Downloaded binary SHA-256 mismatch; installed binary unchanged"
    );
    Ok(())
}

fn replace_with_backup(
    executable: &Path,
    staged: &Path,
    staging: tempfile::TempDir,
    replace: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    // self-replace on Windows renames the old exe before copying the new one.
    let backup = staging.path().join("previous-binary");
    fs::copy(executable, &backup).context("Failed to back up installed binary")?;
    OpenOptions::new().write(true).open(&backup)?.sync_all()?;
    if let Err(error) = replace(staged) {
        let restore = match executable.try_exists() {
            Ok(true) => Ok(()),
            Ok(false) => fs::copy(&backup, executable).map(|_| ()),
            Err(error) => Err(error),
        };
        if let Err(restore) = restore {
            let saved = staging.keep();
            bail!(
                "{error:#}; recovery failed: {restore}; previous binary retained in {}",
                saved.display()
            );
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn check_windows_helper_paths(executable: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    let stem = executable
        .file_stem()
        .context("Executable has no name")?
        .to_string_lossy();
    let helper = format!(".{stem}.{}.__selfdelete__.exe", "x".repeat(32));
    for directory in [
        executable
            .parent()
            .context("Executable has no directory")?
            .to_path_buf(),
        env::temp_dir(),
    ] {
        ensure!(
            directory.join(&helper).as_os_str().encode_wide().count() < 260,
            "Install or temp path exceeds self-replace's Windows helper limit; use a shorter path"
        );
    }
    Ok(())
}

fn reapply_saved_config(executable: &Path, quiet: bool) -> Result<()> {
    // Start installed binary, not old code still running in this process.
    let mut command = Command::new(executable);
    command.args(["config", "--apply"]).stdin(Stdio::null());
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = command
        .status()
        .context("Failed to reapply saved configs; retry `auranion config --apply`")?;
    ensure!(
        status.success(),
        "Config reapply failed ({status}); retry `auranion config --apply`"
    );
    Ok(())
}

fn target_asset_name() -> Result<&'static str> {
    asset_name(env::consts::OS, env::consts::ARCH).with_context(|| {
        format!(
            "Unsupported platform architecture: {}-{}",
            env::consts::OS,
            env::consts::ARCH
        )
    })
}

fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("auranion-windows-amd64.exe"),
        ("windows", "aarch64") => Some("auranion-windows-arm64.exe"),
        ("macos", "x86_64") => Some("auranion-macos-amd64"),
        ("macos", "aarch64") => Some("auranion-macos-arm64"),
        ("linux", "x86_64") => Some("auranion-linux-amd64"),
        ("linux", "aarch64") => Some("auranion-linux-arm64"),
        _ => None,
    }
}

fn is_newer(current: &str, latest: &str) -> Result<bool> {
    let current = Version::parse(current).context("Invalid installed version")?;
    let latest = Version::parse(latest).context("Invalid release version")?;
    Ok(latest.pre.is_empty() && latest.cmp_precedence(&current).is_gt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_always_reapplies_installed_path_and_reports_both_errors() {
        use std::cell::RefCell;
        let installed = Path::new("installed/auranion");
        for update_fails in [false, true] {
            for reapply_fails in [false, true] {
                let calls = RefCell::new(Vec::new());
                let result = update_and_reapply(
                    installed,
                    |path| {
                        assert_eq!(path, installed);
                        calls.borrow_mut().push("update");
                        if update_fails {
                            bail!("update failed");
                        }
                        Ok(())
                    },
                    |path| {
                        assert_eq!(path, installed);
                        calls.borrow_mut().push("reapply");
                        if reapply_fails {
                            bail!("reapply failed");
                        }
                        Ok(())
                    },
                );
                assert_eq!(*calls.borrow(), ["update", "reapply"]);
                assert_eq!(result.is_err(), update_fails || reapply_fails);
                if let Err(error) = result {
                    assert_eq!(error.to_string().contains("update failed"), update_fails);
                    assert_eq!(error.to_string().contains("reapply failed"), reapply_fails);
                }
            }
        }
    }

    #[test]
    fn all_release_targets_have_assets() {
        for (os, arch, expected) in [
            ("windows", "x86_64", "auranion-windows-amd64.exe"),
            ("windows", "aarch64", "auranion-windows-arm64.exe"),
            ("macos", "x86_64", "auranion-macos-amd64"),
            ("macos", "aarch64", "auranion-macos-arm64"),
            ("linux", "x86_64", "auranion-linux-amd64"),
            ("linux", "aarch64", "auranion-linux-arm64"),
        ] {
            assert_eq!(asset_name(os, arch), Some(expected));
        }
        assert_eq!(asset_name("freebsd", "x86_64"), None);
        assert_eq!(asset_name("linux", "x86"), None);
    }

    #[test]
    fn validates_version_and_digest_before_replacement() {
        assert!(is_newer("0.1.0", "0.1.1").unwrap());
        for version in ["0.1.0", "0.0.9", "0.1.0+build", "9.0.0-beta.1"] {
            assert!(!is_newer("0.1.0", version).unwrap());
        }
        assert!(is_newer("0.1.0", "2.invalid.0").is_err());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("download");
        fs::write(&file, b"abc").unwrap();
        let digest = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        verify_hash(&file, sha256_digest(Some(digest)).unwrap()).unwrap();
        assert!(verify_hash(&file, &"0".repeat(64)).is_err());
        for value in [None, Some("md5:abc"), Some("sha256:abc")] {
            assert!(sha256_digest(value).is_err());
        }
    }

    #[test]
    fn locks_release_on_drop_and_replacement_failure_restores_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let lock = try_lock(&path).unwrap().unwrap();
        assert!(try_lock(&path).unwrap().is_none());
        drop(lock);
        assert!(try_lock(&path).unwrap().is_some());
        let installed = dir.path().join("auranion");
        fs::write(&installed, b"old").unwrap();
        let stage = tempfile::tempdir_in(dir.path()).unwrap();
        let new = stage.path().join("new");
        fs::write(&new, b"new").unwrap();
        let result = replace_with_backup(&installed, &new, stage, |_| {
            fs::remove_file(&installed)?;
            bail!("simulated replacement failure");
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("simulated replacement failure")
        );
        assert_eq!(fs::read(&installed).unwrap(), b"old");
    }
}
