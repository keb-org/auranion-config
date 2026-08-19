use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::env;
use std::io::Read;

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

    let temp_file = tempfile_path()?;
    std::fs::write(&temp_file, &binary_bytes).context("Failed to write temporary binary")?;

    self_replace::self_replace(&temp_file).context("Failed to replace current binary")?;
    let _ = std::fs::remove_file(&temp_file);

    println!("Successfully updated to v{latest_version}!");

    if let Err(error) = crate::config::config_apply_saved() {
        eprintln!("Update succeeded but reapplying saved configs failed: {error:#}");
        eprintln!("Run `auranion config --apply` or `auranion config` to retry.");
        return Ok(());
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

fn tempfile_path() -> Result<std::path::PathBuf> {
    let current_exe = env::current_exe()?;
    let dir = current_exe
        .parent()
        .context("Failed to get executable directory")?;
    Ok(dir.join(format!(".auranion-update-{}", std::process::id())))
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
    fn test_version_comparison() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2.0", "0.1.9"));
    }
}
