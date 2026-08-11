use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use toml_edit::DocumentMut;

use super::state::State;

pub(super) fn read_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }

    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }

    serde_json::from_str(strip_bom(&text)).with_context(|| format!("parse {}", path.display()))
}

pub(super) fn write_json(path: &Path, value: &Value) -> Result<()> {
    write_bytes(path, &serde_json::to_vec_pretty(value)?)
}

pub(super) fn read_toml(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }

    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    strip_bom(&text)
        .parse()
        .with_context(|| format!("parse {}", path.display()))
}

#[cfg(test)]
pub(super) fn write_toml(path: &Path, document: &DocumentMut) -> Result<()> {
    write_bytes(path, document.to_string().as_bytes())
}

pub(super) fn write_bytes(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    let permissions = fs::metadata(path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.permissions());
    let (temporary, mut file) = create_temporary(path)?;
    let result = (|| {
        file.write_all(contents)
            .with_context(|| format!("write {}", temporary.display()))?;
        #[cfg(unix)]
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)
                .with_context(|| format!("set permissions on {}", temporary.display()))?;
        }
        file.sync_all()
            .with_context(|| format!("sync {}", temporary.display()))
    })();
    drop(file);
    let result = result.and_then(|()| replace_file(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn create_temporary(path: &Path) -> Result<(PathBuf, File)> {
    for _ in 0..32 {
        let temporary = temporary_path(path)?;
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("create {}", temporary.display()));
            }
        }
    }
    anyhow::bail!("cannot create unique temporary file for {}", path.display());
}

fn temporary_path(path: &Path) -> Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    Ok(path.with_file_name(format!(".{name}.{stamp}.{id}.tmp")))
}

fn replace_file(temporary: &Path, path: &Path) -> Result<()> {
    fs::rename(temporary, path).with_context(|| format!("replace {}", path.display()))?;

    #[cfg(unix)]
    {
        let parent = path.parent().context("file path has no parent directory")?;
        File::open(parent)
            .with_context(|| format!("open {}", parent.display()))?
            .sync_all()
            .with_context(|| format!("sync {}", parent.display()))?;
    }

    Ok(())
}

pub(super) fn json_object<'a>(
    value: &'a Value,
    description: &str,
) -> Result<&'a Map<String, Value>> {
    value
        .as_object()
        .with_context(|| format!("{description} must be a JSON object"))
}

pub(super) fn json_object_mut<'a>(
    value: &'a mut Value,
    description: &str,
) -> Result<&'a mut Map<String, Value>> {
    value
        .as_object_mut()
        .with_context(|| format!("{description} must be a JSON object"))
}

pub(super) fn restore_json_fields(
    path: &Path,
    state: &State,
    restore: impl FnOnce(&mut Value, &Value) -> Result<()>,
) -> Result<()> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }

    let mut current = read_json(path)?;
    let original = read_json(&baseline)?;
    restore(&mut current, &original)?;
    write_json(path, &current)
}

pub(super) fn restore_key(current: &mut Value, original: &Value, key: &str) -> Result<()> {
    let original_value = json_object(original, "baseline JSON root")?
        .get(key)
        .cloned();
    let current = json_object_mut(current, "current JSON root")?;

    match original_value {
        Some(value) => {
            current.insert(key.into(), value);
        }
        None => {
            current.remove(key);
        }
    }
    Ok(())
}

pub(super) fn restore_object_keys(
    current: &mut Value,
    original: &Value,
    object: &str,
    keys: &[&str],
) -> Result<()> {
    let original_root = json_object(original, "baseline JSON root")?;
    let Some(original_value) = original_root.get(object) else {
        return remove_object_keys(current, object, keys);
    };
    let Some(original_object) = original_value.as_object() else {
        json_object_mut(current, "current JSON root")?
            .insert(object.into(), original_value.clone());
        return Ok(());
    };

    let remove_empty = {
        let current_root = json_object_mut(current, "current JSON root")?;
        let current_object = current_root.entry(object).or_insert_with(|| json!({}));
        let current_object = json_object_mut(current_object, object)?;

        for key in keys {
            if let Some(value) = original_object.get(*key) {
                current_object.insert((*key).into(), value.clone());
            } else {
                current_object.remove(*key);
            }
        }
        current_object.is_empty()
    };

    if remove_empty {
        json_object_mut(current, "current JSON root")?.remove(object);
    }
    Ok(())
}

fn remove_object_keys(current: &mut Value, object: &str, keys: &[&str]) -> Result<()> {
    let remove_object = {
        let current_root = json_object_mut(current, "current JSON root")?;
        let Some(current_object) = current_root.get_mut(object) else {
            return Ok(());
        };
        let current_object = json_object_mut(current_object, object)?;
        for key in keys {
            current_object.remove(*key);
        }
        current_object.is_empty()
    };

    if remove_object {
        json_object_mut(current, "current JSON root")?.remove(object);
    }
    Ok(())
}

pub(super) fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-io-{name}-{}", std::process::id()))
    }

    #[test]
    fn write_bytes_replaces_existing_file_without_temporary_artifacts() {
        let dir = test_dir("write-bytes");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("config.json");

        write_bytes(&path, b"first").unwrap();
        write_bytes(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert!(fs::read_dir(path.parent().unwrap()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_bytes_preserves_existing_file_mode() {
        use std::os::unix::fs::{PermissionsExt, set_permissions};

        let dir = test_dir("write-mode");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, b"first").unwrap();
        set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_bytes(&path, b"second").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_bytes_keeps_existing_directory_on_replace_failure() {
        let dir = test_dir("replace-failure");
        let _ = fs::remove_dir_all(&dir);
        let target = dir.join("target");
        fs::create_dir_all(&target).unwrap();

        assert!(write_bytes(&target, b"replacement").is_err());
        assert!(target.is_dir());
        assert!(fs::read_dir(&dir).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn object_helpers_reject_non_object_roots() {
        for value in [json!([]), json!("text"), json!(42)] {
            assert!(json_object(&value, "test").is_err());
            let mut value = value;
            assert!(json_object_mut(&mut value, "test").is_err());
        }
    }

    #[test]
    fn restore_object_keys_removes_created_object_when_baseline_lacks_it() {
        let mut current = json!({"env": {"owned": "value"}});
        restore_object_keys(&mut current, &json!({}), "env", &["owned"]).unwrap();
        assert_eq!(current, json!({}));
    }

    #[test]
    fn restore_object_keys_restores_scalar_baseline() {
        let mut current = json!({"env": {"owned": "value"}});
        restore_object_keys(&mut current, &json!({"env": "original"}), "env", &["owned"]).unwrap();
        assert_eq!(current, json!({"env": "original"}));
    }
}
