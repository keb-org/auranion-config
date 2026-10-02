use anyhow::{Context, Result};
use std::{fs, io::Write, path::PathBuf};


/// Plain file storage for API key on all platforms. Stored mode 0600 in user data dir.
/// Trade security for simplicity: no system keyring, no password prompts.
fn credentials_path() -> PathBuf {
    let dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.data_local_dir().to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    dir.join("auranion").join("credentials")
}

pub(super) fn load() -> Result<Option<String>> {
    match fs::read_to_string(credentials_path()) {
        Ok(key) => {
            let trimmed = key.trim().to_string();
            Ok(if trimmed.is_empty() { None } else { Some(trimmed) })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("read Auranion API key from credentials file"),
    }
}

pub(super) fn save(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("API key cannot be empty");
    }
    save_to(&credentials_path(), key)
}

pub(super) fn delete() -> Result<()> {
    match fs::remove_file(credentials_path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("delete Auranion API key file"),
    }
}

fn save_to(path: &std::path::Path, key: &str) -> Result<()> {
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
    use std::{fs, path::PathBuf};

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-creds-{}-{}", name, std::process::id()))
    }

    #[test]
    fn roundtrip_writes_and_reads_credential() {
        let path = temp_path("roundtrip");
        let _ = fs::remove_file(&path);
        save_to(&path, "test-key").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "test-key\n");
        assert_eq!(load_at(&path).as_deref(), Some("test-key"));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn absent_file_reads_none() {
        let path = temp_path("absent");
        let _ = fs::remove_file(&path);
        assert_eq!(load_at(&path), None);
    }

    #[test]
    fn replaces_existing_contents() {
        let path = temp_path("replaces");
        let _ = fs::remove_file(&path);
        save_to(&path, "old-key").unwrap();
        save_to(&path, "new-key").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new-key\n");
        fs::remove_file(&path).unwrap();
    }

    fn load_at(path: &std::path::Path) -> Option<String> {
        fs::read_to_string(path)
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
    }
}
