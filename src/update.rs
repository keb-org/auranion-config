use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::{io::Read, path::Path, process::Command};

const REPO: &str = "keb-org/auranion-config";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub(super) fn run() -> Result<()> {
    // self_replace can move the running executable; retain its installed path.
    let executable = env::current_exe().context("Failed to locate installed binary")?;
    update_and_reapply(&executable, update_binary, reapply_saved_config)
}

fn update_and_reapply(
    executable: &Path,
    update: impl FnOnce(&Path) -> Result<()>,
    reapply: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let update = update(executable);
    let reapply = reapply(executable);
    match (update, reapply) {
        (Err(update), Err(reapply)) => bail!("{update:#}; {reapply:#}"),
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn update_binary(executable: &Path) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    println!("Checking for updates (current: v{current_version})...");

    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response: Release = ureq::get(&url)
        .set("User-Agent", "auranion-cli")
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("Failed to check GitHub releases")?
        .into_json()
        .context("Failed to parse release information")?;

    let latest_version = response.tag_name.trim_start_matches('v');

    if !is_newer(current_version, latest_version) {
        println!("Already up to date (v{current_version}).");
        return Ok(());
    }

    println!("Found new version: v{latest_version}");

    let asset_name = target_asset_name()?;
    let asset = response
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .with_context(|| {
            format!("Release v{latest_version} has no asset for target {asset_name}")
        })?;

    println!("Downloading {}...", asset.name);
    let resp = ureq::get(&asset.browser_download_url)
        .set("User-Agent", "auranion-cli")
        .call()
        .context("Failed to download binary")?;

    let mut binary_bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut binary_bytes)
        .context("Failed to read binary stream")?;

    let temp_file = executable
        .parent()
        .context("Failed to get executable directory")?
        .join(format!(".auranion-update-{}", std::process::id()));
    std::fs::write(&temp_file, &binary_bytes).context("Failed to write temporary binary")?;

    self_replace::self_replace(&temp_file).context("Failed to replace current binary")?;
    let _ = std::fs::remove_file(&temp_file);

    println!("Successfully updated to v{latest_version}!");

    Ok(())
}

fn reapply_saved_config(executable: &Path) -> Result<()> {
    // Start the installed binary, not the old code still running in this process.
    let status = Command::new(executable)
        .args(["config", "--apply"])
        .status()
        .context("Failed to reapply saved configs; retry `auranion config --apply`")?;
    if !status.success() {
        bail!("Config reapply failed ({status}); retry `auranion config --apply`");
    }
    Ok(())
}

fn target_asset_name() -> Result<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Ok("auranion-windows-amd64.exe")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Ok("auranion-macos-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Ok("auranion-linux-amd64")
    } else {
        bail!("Unsupported platform architecture for self-update")
    }
}

fn is_newer(current: &str, latest: &str) -> bool {
    let parse =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse::<u64>().ok()).collect() };
    let c = parse(current);
    let l = parse(latest);
    l > c
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
    fn test_version_comparison() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
    }
}
