use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::{fs, path::Path};
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
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

pub(super) fn write_toml(path: &Path, document: &DocumentMut) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, document.to_string()).with_context(|| format!("write {}", path.display()))?;
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
