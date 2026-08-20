use anyhow::{Context, Result};
use keyring::{Entry, Error};
#[cfg(target_os = "linux")]
use std::{fs, io::Write, path::PathBuf};
#[cfg(all(test, not(target_os = "linux")))]
use std::{fs, io::Write};

const SERVICE: &str = "auranion";
const ACCOUNT: &str = "api-key";

/// On Linux the secret-service prompt (GNOME Keyring / KWallet) is a modal
/// dialog that can be dismissed or missing entirely (headless/WSL), which makes
/// every keyring call fail. The fallback file keeps configure/apply/update
/// working without a GUI. It is stored 0600 in the user data dir.
#[cfg(target_os = "linux")]
fn fallback_path() -> PathBuf {
    let dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.data_local_dir().to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    dir.join("auranion").join("credentials")
}

pub(super) fn load() -> Result<Option<String>> {
    #[cfg(target_os = "linux")]
    {
        let keyring_key = entry()
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .filter(|key| !key.trim().is_empty());
        let file_key = fallback_load().filter(|key| !key.trim().is_empty());
        // Prefer the keyring credential; fall back to the file when the
        // secret-service prompt was dismissed or is unavailable.
        return Ok(keyring_key.or(file_key));
    }

    #[cfg(not(target_os = "linux"))]
    match entry()?.get_password() {
        Ok(key) if key.trim().is_empty() => Ok(None),
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("read Auranion API key from secure credential store"),
    }
}

pub(super) fn save(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    #[cfg(target_os = "linux")]
    {
        match entry().ok().and_then(|entry| entry.set_password(key).ok()) {
            Some(()) => {
                // Keyring is authoritative; drop any stale fallback file.
                let _ = fallback_delete();
                Ok(())
            }
            None => fallback_save(key),
        }
    }

    #[cfg(not(target_os = "linux"))]
    entry()?
        .set_password(key)
        .context("save Auranion API key in secure credential store")
}

pub(super) fn delete() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        match entry().ok().and_then(|entry| entry.delete_credential().ok()) {
            Some(()) => fallback_delete(),
            None => fallback_delete(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("delete Auranion API key from secure credential store"),
    }
}

fn entry() -> Result<Entry> {
    Entry::new(SERVICE, ACCOUNT).context("access Auranion secure credential store")
}

#[cfg(target_os = "linux")]
fn fallback_load() -> Option<String> {
    fs::read_to_string(fallback_path()).ok()
}

#[cfg(target_os = "linux")]
fn fallback_save(key: &str) -> Result<()> {
    fallback_save_to(&fallback_path(), key)
}

#[cfg(target_os = "linux")]
fn fallback_delete() -> Result<()> {
    match fs::remove_file(fallback_path()) {
        Ok(()) | Err(std::io::ErrorKind::NotFound) => Ok(()),
        Err(error) => Err(error).context("delete Auranion API key fallback file"),
    }
}

/// Writes the credential to a file at `path`, restricted to the owner where the
/// platform supports it. Kept outside the Linux gate so the logic is testable
/// on any host.
#[cfg(any(test, target_os = "linux"))]
fn fallback_save_to(path: &std::path::Path, key: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    file.write_all(key.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, path::PathBuf};

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-keyring-{}-{}", name, std::process::id()))
    }

    #[test]
    fn fallback_roundtrip_writes_and_reads_credential() {
        let dir = temp_dir("roundtrip");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("credentials");

        fallback_save_to(&path, "test-key").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "test-key\n");
        assert_eq!(fallback_load_at(&path).as_deref(), Some("test-key"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fallback_absent_file_reads_none() {
        let dir = temp_dir("absent");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("credentials");
        assert_eq!(fallback_load_at(&path), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fallback_replaces_existing_contents() {
        let dir = temp_dir("replaces");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("credentials");
        fallback_save_to(&path, "old-key").unwrap();
        fallback_save_to(&path, "new-key").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new-key\n");
        fs::remove_dir_all(&dir).unwrap();
    }

    fn fallback_load_at(path: &Path) -> Option<String> {
        fs::read_to_string(path)
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
    }
}
