use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_yaml::Value as YamlValue;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::catalog::MODELS;

use super::super::{
    BASE_URL,
    io::{strip_bom, write_bytes},
    state::State,
};

const PROVIDER_KEY: &str = "auranion";
const LEGACY_PROVIDER_NAMES: [&str; 2] = ["auranion", "Auranion"];

pub(super) fn detect(dirs: &BaseDirs) -> bool {
    hermes_home(dirs).join("config.yaml").exists()
}

pub(super) fn diagnostics(dirs: &BaseDirs) -> Vec<String> {
    let path = hermes_home(dirs).join("config.yaml");
    if path.exists() {
        Vec::new()
    } else {
        vec!["hermes config.yaml not found".into()]
    }
}

pub(super) fn select(
    dirs: &BaseDirs,
    data_dir: &Path,
    state: &mut State,
    api_key: &str,
) -> Result<()> {
    let path = hermes_home(dirs).join("config.yaml");
    state.backup(data_dir, &path)?;
    merge(&path, api_key)
}

pub(super) fn deselect(dirs: &BaseDirs, state: &mut State) -> Result<()> {
    let path = hermes_home(dirs).join("config.yaml");
    if !path.exists() {
        state.forget_baseline(&path);
        return Ok(());
    }
    if state.baseline_for(&path).is_none() {
        remove_provider(&path)?;
        return Ok(());
    }
    restore(&path, state)
}

fn hermes_home(dirs: &BaseDirs) -> PathBuf {
    if let Ok(val) = std::env::var("HERMES_HOME") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if cfg!(windows) {
        if let Ok(val) = std::env::var("LOCALAPPDATA") {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("hermes");
            }
        }
        dirs.data_local_dir().join("hermes")
    } else {
        dirs.home_dir().join(".hermes")
    }
}

fn merge(path: &Path, api_key: &str) -> Result<()> {
    let mut root = read_yaml(path)?;
    if !root.is_mapping() {
        anyhow::bail!("hermes config root must be a mapping");
    }
    let provider = auranion_provider(api_key);
    upsert_provider(&mut root, provider);
    remove_legacy_custom_providers(&mut root);
    write_yaml(path, &root)
}

fn remove_provider(path: &Path) -> Result<()> {
    let mut root = read_yaml(path)?;
    if !root.is_mapping() {
        return Ok(());
    }
    let mut changed = false;
    if remove_provider_entry(&mut root) {
        changed = true;
    }
    if remove_legacy_custom_providers(&mut root) {
        changed = true;
    }
    if changed {
        write_yaml(path, &root)?;
    }
    Ok(())
}

fn restore(path: &Path, state: &State) -> Result<()> {
    let Some(baseline) = state.baseline_for(path) else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let existed = state.baseline_existed(path).unwrap_or(true);
    if !existed {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
        return Ok(());
    }
    let baseline_text =
        fs::read_to_string(&baseline).with_context(|| format!("read {}", baseline.display()))?;
    let original =
        parse_yaml_text(&baseline_text).with_context(|| format!("parse {}", baseline.display()))?;
    let mut current = read_yaml(path)?;
    if !current.is_mapping() {
        current = YamlValue::Mapping(Default::default());
    }
    restore_provider(&mut current, &original);
    write_yaml(path, &current)
}

fn restore_provider(current: &mut YamlValue, original: &YamlValue) {
    let original_provider = provider_entry(original);
    if let Some(provider) = original_provider {
        upsert_provider(current, provider.clone());
    } else {
        remove_provider_entry(current);
    }

    let original_legacy = legacy_entries(original);
    let current_non_legacy = current
        .as_mapping()
        .and_then(|m| m.get(YamlValue::String("custom_providers".into())))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter(|entry| {
                    entry
                        .as_mapping()
                        .and_then(|m| m.get(YamlValue::String("name".into())))
                        .and_then(|v| v.as_str())
                        .is_none_or(|name| !LEGACY_PROVIDER_NAMES.contains(&name))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let merged = if original_legacy.is_empty() {
        current_non_legacy
    } else {
        let mut v = current_non_legacy;
        v.extend(original_legacy);
        v
    };

    if let Some(map) = current.as_mapping_mut() {
        if merged.is_empty() {
            map.remove(YamlValue::String("custom_providers".into()));
        } else {
            map.insert(
                YamlValue::String("custom_providers".into()),
                YamlValue::Sequence(merged),
            );
        }
    }
}

fn provider_entry(value: &YamlValue) -> Option<YamlValue> {
    value
        .as_mapping()?
        .get(YamlValue::String("providers".into()))?
        .as_mapping()?
        .get(YamlValue::String(PROVIDER_KEY.into()))
        .cloned()
}

fn legacy_entries(value: &YamlValue) -> Vec<YamlValue> {
    let Some(seq) = value
        .as_mapping()
        .and_then(|m| m.get(YamlValue::String("custom_providers".into())))
        .and_then(|v| v.as_sequence())
    else {
        return Vec::new();
    };
    seq.iter()
        .filter(|entry| {
            entry
                .as_mapping()
                .and_then(|m| m.get(YamlValue::String("name".into())))
                .and_then(|v| v.as_str())
                .is_some_and(|name| LEGACY_PROVIDER_NAMES.contains(&name))
        })
        .cloned()
        .collect()
}

fn upsert_provider(root: &mut YamlValue, provider: YamlValue) {
    let map = root.as_mapping_mut().expect("caller ensures mapping");
    let key = YamlValue::String("providers".into());
    if map.get(&key).is_none_or(|value| !value.is_mapping()) {
        map.insert(key.clone(), YamlValue::Mapping(Default::default()));
    }
    let providers = map.get_mut(&key).expect("just inserted mapping");
    let providers_map = providers.as_mapping_mut().expect("providers is mapping");
    providers_map.insert(YamlValue::String(PROVIDER_KEY.into()), provider);
}

fn remove_provider_entry(root: &mut YamlValue) -> bool {
    let Some(map) = root.as_mapping_mut() else {
        return false;
    };
    let Some(providers) = map.get_mut(YamlValue::String("providers".into())) else {
        return false;
    };
    let Some(providers_map) = providers.as_mapping_mut() else {
        return false;
    };
    let removed = providers_map
        .remove(YamlValue::String(PROVIDER_KEY.into()))
        .is_some();
    if providers_map.is_empty() {
        map.remove(YamlValue::String("providers".into()));
    }
    removed
}

fn remove_legacy_custom_providers(root: &mut YamlValue) -> bool {
    let Some(map) = root.as_mapping_mut() else {
        return false;
    };
    let key = YamlValue::String("custom_providers".into());
    let (before, after, empty) = {
        let Some(seq) = map.get_mut(&key).and_then(|v| v.as_sequence_mut()) else {
            return false;
        };
        let before = seq.len();
        seq.retain(|entry| {
            entry
                .as_mapping()
                .and_then(|m| m.get(YamlValue::String("name".into())))
                .and_then(|v| v.as_str())
                .is_none_or(|name| !LEGACY_PROVIDER_NAMES.contains(&name))
        });
        (before, seq.len(), seq.is_empty())
    };
    if empty {
        map.remove(key);
    }
    after != before
}

fn auranion_provider(api_key: &str) -> YamlValue {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        YamlValue::String("base_url".into()),
        YamlValue::String(BASE_URL.into()),
    );
    map.insert(
        YamlValue::String("api_key".into()),
        YamlValue::String(api_key.into()),
    );
    map.insert(
        YamlValue::String("api_mode".into()),
        YamlValue::String("chat_completions".into()),
    );
    let mut models = serde_yaml::Mapping::new();
    for model in MODELS {
        let mut meta = serde_yaml::Mapping::new();
        if let Some(ctx) = model.context {
            meta.insert(
                YamlValue::String("context_length".into()),
                YamlValue::Number(serde_yaml::Number::from(ctx)),
            );
        }
        if model.reasoning {
            meta.insert(YamlValue::String("reasoning".into()), YamlValue::Bool(true));
        }
        models.insert(
            YamlValue::String(model.upstream.into()),
            YamlValue::Mapping(meta),
        );
    }
    map.insert(
        YamlValue::String("models".into()),
        YamlValue::Mapping(models),
    );
    YamlValue::Mapping(map)
}

fn read_yaml(path: &Path) -> Result<YamlValue> {
    if !path.exists() {
        return Ok(YamlValue::Mapping(Default::default()));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_yaml_text(&text).with_context(|| format!("parse {}", path.display()))
}

fn parse_yaml_text(text: &str) -> Result<YamlValue> {
    if text.trim().is_empty() {
        return Ok(YamlValue::Mapping(Default::default()));
    }
    let stripped = strip_bom(text);
    let value: YamlValue = serde_yaml::from_str(stripped).context("parse yaml")?;
    normalize_yaml_value(value)
}

fn normalize_yaml_value(value: YamlValue) -> Result<YamlValue> {
    let json = serde_json::to_value(&value).context("convert yaml to json")?;
    let yaml = serde_yaml::to_value(&json).context("convert json to yaml")?;
    Ok(yaml)
}

fn write_yaml(path: &Path, value: &YamlValue) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(value).context("serialize yaml")?;
    write_bytes(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("auranion-hermes-{name}-{}", std::process::id()))
    }

    #[test]
    fn merge_is_idempotent_and_preserves_other_providers() {
        let dir = tmp_path("idempotent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  other:
    base_url: https://other.example.com/v1
    api_key: other-key
"#,
        )
        .unwrap();

        merge(&path, "test-key").unwrap();
        let first = read_yaml(&path).unwrap();
        merge(&path, "test-key").unwrap();
        let second = read_yaml(&path).unwrap();
        assert_eq!(second, first);

        let root = read_yaml(&path).unwrap();
        let providers = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("providers".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert!(providers.contains_key(YamlValue::String("other".into())));
        let auranion = providers
            .get(YamlValue::String(PROVIDER_KEY.into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert_eq!(
            auranion
                .get(YamlValue::String("base_url".into()))
                .and_then(|v| v.as_str()),
            Some(BASE_URL)
        );
        assert_eq!(
            auranion
                .get(YamlValue::String("api_key".into()))
                .and_then(|v| v.as_str()),
            Some("test-key")
        );
        assert!(
            auranion
                .get(YamlValue::String("models".into()))
                .and_then(|v| v.as_mapping())
                .is_some_and(|m| m.len() == MODELS.len())
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_removes_legacy_custom_providers() {
        let dir = tmp_path("legacy");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"custom_providers:
  - name: auranion
    base_url: https://agent.auranion.com/v1
    api_key: old
  - name: other
    base_url: https://other.example.com/v1
"#,
        )
        .unwrap();

        merge(&path, "new-key").unwrap();
        let root = read_yaml(&path).unwrap();
        let seq = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("custom_providers".into()))
            .and_then(|v| v.as_sequence())
            .unwrap();
        assert_eq!(seq.len(), 1);
        assert_eq!(
            seq[0]
                .as_mapping()
                .unwrap()
                .get(YamlValue::String("name".into()))
                .and_then(|v| v.as_str()),
            Some("other")
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_provider_preserves_other_providers() {
        let dir = tmp_path("remove");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(
            &path,
            r#"providers:
  other:
    base_url: https://other.example.com/v1
  auranion:
    base_url: https://agent.auranion.com/v1
    api_key: key
"#,
        )
        .unwrap();

        remove_provider(&path).unwrap();
        let root = read_yaml(&path).unwrap();
        let providers = root
            .as_mapping()
            .unwrap()
            .get(YamlValue::String("providers".into()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert!(!providers.contains_key(YamlValue::String(PROVIDER_KEY.into())));
        assert!(providers.contains_key(YamlValue::String("other".into())));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_rejects_non_mapping_root() {
        let dir = tmp_path("non-mapping");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml");
        fs::write(&path, "[]\n").unwrap();
        let result = merge(&path, "key");
        assert!(result.is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
